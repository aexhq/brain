package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"github.com/aexhq/brain/services/sandbox/internal/environments"
	"github.com/aexhq/brain/services/sandbox/internal/server"
	"github.com/aexhq/brain/telemetry"
	"io"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"
)

type config struct {
	Listen          string                          `json:"listen"`
	Instance        string                          `json:"instance"`
	TokenEnv        string                          `json:"token_env"`
	Capacity        int                             `json:"capacity"`
	MaxArchiveBytes int64                           `json:"max_archive_bytes"`
	Profiles        map[string]environments.Profile `json:"profiles"`
}

func main() {
	if e := run(); e != nil {
		slog.Error("sandbox stopped", "error", e)
		os.Exit(1)
	}
}
func run() error {
	file := flag.String("config", "sandbox.json", "sandbox configuration")
	flag.Parse()
	f, e := os.Open(*file)
	if e != nil {
		return e
	}
	defer f.Close()
	c := config{Listen: ":8083", Instance: "local", TokenEnv: "BRAIN_SERVICE_TOKEN", Capacity: 64, MaxArchiveBytes: 1 << 30}
	d := json.NewDecoder(io.LimitReader(f, 1<<20))
	d.DisallowUnknownFields()
	if e = d.Decode(&c); e != nil {
		return e
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing configuration data")
	}
	m := &environments.Manager{Profiles: c.Profiles, Instance: c.Instance, Capacity: c.Capacity}
	if e = m.Validate(); e != nil {
		return e
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	_, shutdown, e := telemetry.SetupService(ctx, "sandbox")
	if e != nil {
		return e
	}
	defer func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = shutdown(ctx)
	}()
	if e = m.Reap(ctx); e != nil {
		return e
	}
	defer func() {
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		_ = m.Close(ctx)
	}()
	go m.Run(ctx)
	h, e := (&server.Server{Manager: m, Token: os.Getenv(c.TokenEnv), MaxArchiveBytes: c.MaxArchiveBytes}).Handler()
	if e != nil {
		return e
	}
	s := &http.Server{Addr: c.Listen, Handler: h, ReadHeaderTimeout: 10 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 32 << 10}
	done := make(chan error, 1)
	go func() { done <- s.ListenAndServe() }()
	select {
	case e := <-done:
		if errors.Is(e, http.ErrServerClosed) {
			return nil
		}
		return e
	case <-ctx.Done():
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	return s.Shutdown(ctx)
}
