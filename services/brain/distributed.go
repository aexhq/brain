package brain

import (
	"context"
	"errors"
	"github.com/aexhq/brain/internal/volume"
	"log/slog"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/aexhq/brain/runtimes/host"
	"github.com/aexhq/brain/runtimes/pi"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/brain/internal/http"
	"github.com/aexhq/brain/services/brain/internal/sessions"
	workspace "github.com/aexhq/brain/services/brain/storage"
	sandbox "github.com/aexhq/brain/services/sandbox/api"
	storage "github.com/aexhq/brain/services/storage/api"
	tools "github.com/aexhq/brain/services/tools/api"
	"github.com/aexhq/brain/telemetry"
)

func newDistributed(ctx context.Context, o Options) (_ *Server, err error) {
	if len(o.Tools) > 0 || len(o.Providers) > 0 || len(o.Harnesses) > 0 || o.HostPool != nil {
		return nil, errors.New("distributed composition owns runtime and service clients")
	}
	c := o.Config
	deps := c.Services
	token := os.Getenv(deps.TokenEnv)
	logger := o.Logger
	if logger == nil {
		logger = slog.Default()
	}
	if err = os.MkdirAll(c.DataDir, 0700); err != nil {
		return nil, err
	}
	s := &Server{keys: o.Keys}
	defer func() {
		if err != nil {
			for i := len(s.close) - 1; i >= 0; i-- {
				err = errors.Join(err, s.close[i]())
			}
		}
	}()
	owner, err := volume.Lock(c.DataDir)
	if err != nil {
		return nil, err
	}
	s.close = append(s.close, owner.Close)
	// This namespace contains only materialized copies of Storage-owned data.
	// Discarding it after a crash bounds abandoned working files.
	working := filepath.Join(c.DataDir, "work")
	if err = os.RemoveAll(working); err != nil {
		return nil, err
	}
	if err = os.MkdirAll(working, 0700); err != nil {
		return nil, err
	}
	metrics, shutdown, err := telemetry.SetupService(ctx, "brain")
	if err != nil {
		return nil, err
	}
	s.close = append(s.close, func() error {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		return shutdown(ctx)
	})
	storeClient := &storage.Client{URL: deps.Storage, Token: token}
	s.remote = &workspace.Workspace{Client: storeClient, Owner: deps.Owner, Epoch: deps.Epoch, Directory: working, MaxBytes: c.Limits.SessionBytes, MediaOrigins: c.MediaOrigins}
	toolClient := &tools.Client{URL: deps.Tools, Token: token, Owner: deps.Owner, Epoch: deps.Epoch}
	factory := pi.Factory(&sandbox.Client{URL: deps.Sandbox, Token: token}, c.Limits.SessionBytes)
	factory.MaxHosts = c.Limits.Active
	factory.MaxStarting = c.Harnesses["pi"].MaxStarting
	factory.Plugins = c.Harnesses["pi"].Plugins
	pool, err := harness.NewHostPool(harness.PoolLimits{Hosts: c.Limits.Active, Starting: factory.MaxStarting, ActiveTurns: c.Limits.Active})
	if err != nil {
		return nil, err
	}
	factory.Pool = pool
	s.close = append(s.close, pool.Close)
	auth := core.NewAuth(c.Tokens)
	auth.Application = o.Authenticate
	s.engine, err = session.New(session.Options{IdleTTL: time.Duration(deps.IdleTTLSeconds) * time.Second, Inputs: s.remote, Store: s.remote, Archive: &workspace.Archive{Workspace: s.remote}, Tools: toolClient, Keys: o.Keys, Factories: map[string]harness.Factory{"pi": factory}, Provider: o.resolveModel, MaxActive: c.Limits.Active, MaxLoaded: c.Limits.Loaded, MaxQueue: c.Limits.Queue, MaxInputBytes: c.Limits.InputBytes, MaxLogBytes: c.Limits.SessionBytes, Logger: logger, Metrics: metrics, MediaOrigins: c.MediaOrigins, Inspection: func(v core.Session) (string, string, func()) {
		token, revoke := auth.Delegate(v)
		return token, c.PublicURL, revoke
	}})
	if err != nil {
		return nil, err
	}
	base := (&api.Server{Ready: dependenciesReady(deps.Storage, deps.Tools, deps.Sandbox), Transcripts: s.remote, Attachments: s.remote, Engine: s.engine, Tools: toolClient, Auth: auth, Limits: c.Limits, Metrics: metrics, Logger: logger}).Handler()
	u, err := url.Parse(deps.Tools)
	if err != nil {
		return nil, err
	}
	proxy := httputil.NewSingleHostReverseProxy(u)
	proxy.ErrorHandler = func(w http.ResponseWriter, r *http.Request, e error) {
		logger.Error("tools service proxy failed", "error", e)
		http.Error(w, "tools service unavailable", http.StatusBadGateway)
	}
	h := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/executors" || strings.HasPrefix(r.URL.Path, "/executors/") || r.URL.Path == "/tools/packages" {
			proxy.ServeHTTP(w, r)
			return
		}
		base.ServeHTTP(w, r)
	})
	s.http = &http.Server{Addr: c.Listen, Handler: h, ReadHeaderTimeout: 10 * time.Second, IdleTimeout: 90 * time.Second, MaxHeaderBytes: 32 << 10}
	return s, nil
}
