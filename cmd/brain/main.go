package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/aexhq/brain/internal/api"
	"github.com/aexhq/brain/internal/config"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/persistence"
	"github.com/aexhq/brain/internal/session"
	"github.com/aexhq/brain/internal/store"
	"github.com/aexhq/brain/internal/telemetry"
	"github.com/aexhq/brain/internal/tools"
)

func main() {
	if err := run(); err != nil {
		slog.Error("brain stopped", "error", err)
		os.Exit(1)
	}
}
func run() error {
	file := flag.String("config", "brain.json", "host configuration JSON")
	check := flag.Bool("check", false, "validate configuration without serving")
	flag.Parse()
	if flag.NArg() != 0 {
		return errors.New("unexpected arguments")
	}
	logger := slog.New(slog.NewJSONHandler(os.Stderr, nil))
	slog.SetDefault(logger)
	cfg, err := config.Load(*file)
	if err != nil {
		return err
	}
	registry, err := tools.New(cfg.Tools)
	if err != nil {
		return err
	}
	defer registry.Close()
	if *check {
		fmt.Println("configuration valid")
		return nil
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	metrics, shutdownTelemetry, err := telemetry.Setup(ctx)
	if err != nil {
		return err
	}
	defer func() {
		drain, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := shutdownTelemetry(drain); err != nil {
			logger.Error("telemetry shutdown", "error", err)
		}
	}()
	db, err := store.Open(cfg.DataDir)
	if err != nil {
		return err
	}
	defer db.Close()
	var archive *persistence.Archive
	var syncQueue int
	var syncInterval time.Duration
	if cfg.Persistence != nil {
		objects, err := persistence.NewObjects(ctx, *cfg.Persistence)
		if err != nil {
			return err
		}
		if closer, ok := objects.(io.Closer); ok {
			defer closer.Close()
		}
		archive = &persistence.Archive{Objects: objects, MaxSessionBytes: cfg.Limits.SessionBytes}
		if cfg.Persistence.PostgresEnv != "" {
			metadata, err := persistence.NewPostgres(os.Getenv(cfg.Persistence.PostgresEnv))
			if err != nil {
				return err
			}
			defer metadata.Close()
			archive.Metadata = metadata
		}
		syncQueue = cfg.Persistence.Queue
		syncInterval = time.Duration(cfg.Persistence.IntervalSeconds) * time.Second
	}
	auth := api.NewAuth(cfg.Tokens)
	factories := make(map[string]harness.Factory)
	for name, r := range cfg.Harnesses {
		factories[name] = &harness.ProcessFactory{Command: r.Command, Steering: name == "pi", Logger: logger}
	}
	engine, err := session.New(session.Options{Store: db, Tools: registry, Factories: factories, Provider: cfg.Provider, MaxActive: cfg.Limits.Active, MaxLoaded: cfg.Limits.Loaded, MaxQueue: cfg.Limits.Queue, MaxInputBytes: cfg.Limits.InputBytes, MaxLogBytes: cfg.Limits.SessionBytes, Logger: logger, Metrics: metrics, Inspection: func(s core.Session) (string, string, func()) {
		token, revoke := auth.Delegate(s)
		return token, cfg.PublicURL, revoke
	}, Archive: archive, SyncQueue: syncQueue, SyncInterval: syncInterval})
	if err != nil {
		return err
	}
	server := &api.Server{Engine: engine, Tools: registry, Auth: auth, Limits: cfg.Limits, Metrics: metrics, Logger: logger}
	httpServer := &http.Server{Addr: cfg.Listen, Handler: server.Handler(), ReadHeaderTimeout: 10 * time.Second, ReadTimeout: 30 * time.Second, IdleTimeout: 90 * time.Second, MaxHeaderBytes: 16 << 10}
	listener, err := net.Listen("tcp", cfg.Listen)
	if err != nil {
		drain, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		engine.Shutdown(drain)
		return err
	}
	errorsCh := make(chan error, 1)
	go func() { errorsCh <- httpServer.Serve(listener) }()
	logger.Info("brain listening", "address", listener.Addr().String(), "data_dir", cfg.DataDir)
	select {
	case <-ctx.Done():
	case err := <-errorsCh:
		if !errors.Is(err, http.ErrServerClosed) {
			stop()
			logger.Error("HTTP listener stopped", "error", err)
		}
	}
	drain, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	// Cancel execution first; closing event logs releases long-lived observers.
	engineErr := engine.Shutdown(drain)
	httpErr := httpServer.Shutdown(drain)
	if httpErr != nil {
		_ = httpServer.Close()
	}
	return errors.Join(engineErr, httpErr)
}
