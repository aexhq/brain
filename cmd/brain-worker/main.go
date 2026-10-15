package main

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/aexhq/brain/adapters/compute"
	"github.com/aexhq/brain/internal/worker"
)

func main() {
	if err := run(); err != nil {
		slog.Error("worker_startup_failed", "message", err.Error())
		os.Exit(1)
	}
}

func env(name, fallback string) string {
	if value := os.Getenv(name); value != "" {
		return value
	}
	return fallback
}

func run() error {
	key := []byte(os.Getenv("BRAIN_WORKER_KEY"))
	development := os.Getenv("BRAIN_WORKER_TRUSTED_DEVELOPMENT") == "1"
	provider := &compute.Docker{Key: key, Trusted: development,
		NodeImage: os.Getenv("BRAIN_WORKER_NODE_IMAGE"), PythonImage: os.Getenv("BRAIN_WORKER_PYTHON_IMAGE")}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	err := provider.Ready(ctx)
	cancel()
	if err != nil {
		return err
	}
	concurrency, err := strconv.Atoi(env("BRAIN_WORKER_CONCURRENCY", "2"))
	if err != nil {
		return errors.New("invalid worker concurrency")
	}
	app, err := worker.Open(worker.Config{Directory: env("BRAIN_WORKER_DATA", ".brain-worker"), Key: key,
		Concurrency: concurrency, CallbackOrigins: strings.Fields(os.Getenv("BRAIN_WORKER_CALLBACK_ORIGINS")), Development: development}, provider)
	if err != nil {
		return err
	}
	defer app.Close()
	listener, err := net.Listen("tcp", env("BRAIN_WORKER_LISTEN", "127.0.0.1:8090"))
	if err != nil {
		return err
	}
	httpServer := &http.Server{Handler: app, ReadHeaderTimeout: 10 * time.Second, ReadTimeout: 30 * time.Second, IdleTimeout: time.Minute, MaxHeaderBytes: 16384}
	finished := make(chan error, 1)
	go func() { finished <- httpServer.Serve(listener) }()
	slog.Info("worker_ready", "listen", listener.Addr().String(), "profile", "trusted-local-development")
	signals := make(chan os.Signal, 1)
	signal.Notify(signals, os.Interrupt, syscall.SIGTERM)
	defer signal.Stop(signals)
	select {
	case <-signals:
	case err := <-finished:
		if !errors.Is(err, http.ErrServerClosed) {
			return err
		}
	}
	stop, stopped := context.WithTimeout(context.Background(), 310*time.Second)
	defer stopped()
	return httpServer.Shutdown(stop)
}
