package brain

import (
	"context"
	"errors"
	"github.com/aexhq/brain/internal/api"
	"github.com/aexhq/brain/internal/config"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/persistence"
	"github.com/aexhq/brain/internal/session"
	"github.com/aexhq/brain/internal/store"
	"github.com/aexhq/brain/internal/telemetry"
	"github.com/aexhq/brain/internal/tools"
	"io"
	"log/slog"
	"net"
	"net/http"
	"os"
	"sync"
	"time"
)

type Options struct {
	Config       Config
	Models       map[string]ModelProvider
	HostPool     *HostPool
	Tools        []ManagedTool
	Authenticate func(*http.Request) (Grant, bool)
	Harnesses    map[string]HarnessFactory
	Providers    map[string]ToolProvider
	Keys         CredentialKeys
	Logger       *slog.Logger
}

// Server owns opened resources. Call Shutdown even when serving through Handler.
type Server struct {
	engine *session.Engine
	db     *store.Store
	http   *http.Server
	close  []func() error
	once   sync.Once
	err    error
	keys   CredentialKeys
	bridge *tools.Bridge
}

func validate(o *Options) error {
	poolLimits := o.Config.HostPool
	if o.HostPool != nil {
		if poolLimits != (harness.PoolLimits{}) {
			return errors.New("host pool is declared twice")
		}
		poolLimits = o.HostPool.Limits()
	}
	if poolLimits != (harness.PoolLimits{}) {
		for _, factory := range o.Harnesses {
			if _, ok := factory.(*harness.ProcessFactory); !ok {
				return errors.New("aggregate host limits require a process factory")
			}
		}
	}
	providers := make(map[string]config.Provider, len(o.Config.Providers)+len(o.Models))
	for name, provider := range o.Config.Providers {
		providers[name] = provider
	}
	for name, provider := range o.Models {
		if _, exists := providers[name]; exists {
			return errors.New("model provider is declared twice: " + name)
		}
		providers[name] = config.Provider{APIKey: provider.APIKey, BaseURL: provider.BaseURL, API: provider.API}
	}
	o.Config.Providers = providers
	o.Config.Tools = append(append([]ManagedTool{}, o.Config.Tools...), o.Tools...)
	if o.Config.Harnesses == nil {
		o.Config.Harnesses = map[string]ProcessConfig{}
	}
	for name := range o.Harnesses {
		if _, ok := o.Config.Harnesses[name]; !ok {
			o.Config.Harnesses[name] = ProcessConfig{}
		}
	}
	if err := o.Config.ValidateApplication(o.Harnesses, o.Authenticate != nil); err != nil {
		return err
	}
	return tools.ValidateProviders(o.Providers)
}

func Validate(o Options) error {
	if err := validate(&o); err != nil {
		return err
	}
	r, err := tools.New(o.Config.Tools)
	if err != nil {
		return err
	}
	r.Close()
	return nil
}

func New(ctx context.Context, o Options) (_ *Server, err error) {
	if o.Keys == nil && o.Config.CredentialKeys != nil {
		o.Keys = o.Config.CredentialKeys
	}
	if err = validate(&o); err != nil {
		return nil, err
	}
	logger := o.Logger
	if logger == nil {
		logger = slog.Default()
	}
	s := &Server{keys: o.Keys}
	defer func() {
		if err != nil {
			for i := len(s.close) - 1; i >= 0; i-- {
				err = errors.Join(err, s.close[i]())
			}
		}
	}()
	registry, err := tools.New(o.Config.Tools)
	if err != nil {
		return nil, err
	}
	s.close = append(s.close, func() error { registry.Close(); return nil })
	bridge := tools.NewBridge(o.Config.Limits.Loaded)
	s.bridge = bridge
	s.close = append(s.close, func() error { bridge.Close(); return nil })
	registry.Runtime = bridge
	registry.Providers = o.Providers
	registry.Keys = o.Keys
	metrics, shutdownTelemetry, err := telemetry.Setup(ctx)
	if err != nil {
		return nil, err
	}
	s.close = append(s.close, func() error {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		return shutdownTelemetry(ctx)
	})
	db, err := store.Open(o.Config.DataDir)
	if err != nil {
		return nil, err
	}
	s.db = db
	s.close = append(s.close, db.Close)
	var archive session.Checkpoints
	var queue int
	var interval time.Duration
	if p := o.Config.Persistence; p != nil {
		objects, err := persistence.NewObjects(ctx, *p)
		if err != nil {
			return nil, err
		}
		if c, ok := objects.(io.Closer); ok {
			s.close = append(s.close, c.Close)
		}
		a := &persistence.Archive{Objects: objects, MaxSessionBytes: o.Config.Limits.SessionBytes}
		archive = a
		if p.PostgresEnv != "" {
			metadata, err := persistence.NewPostgres(os.Getenv(p.PostgresEnv))
			if err != nil {
				return nil, err
			}
			a.Metadata = metadata
			s.close = append(s.close, metadata.Close)
		}
		queue = p.Queue
		interval = time.Duration(p.IntervalSeconds) * time.Second
	}
	auth := api.NewAuth(o.Config.Tokens)
	auth.Application = o.Authenticate
	pool := o.HostPool
	if pool == nil {
		pool, err = harness.NewHostPool(o.Config.HostPool)
		if err != nil {
			return nil, err
		}
	}
	s.close = append(s.close, pool.Close)
	factories := map[string]harness.Factory{}
	for name, r := range o.Config.Harnesses {
		if f := o.Harnesses[name]; f != nil {
			if process, ok := f.(*harness.ProcessFactory); ok {
				if err := process.UseHostPool(pool); err != nil {
					return nil, err
				}
			}
			factories[name] = f
		} else {
			factory := &harness.ProcessFactory{}
			if name == "codex" {
				factory = harness.Codex(r.Command)
			} else if name == "opencode" {
				factory = harness.OpenCode(r.Command)
			}
			factory.Pool, factory.Command, factory.Steering, factory.Logger = pool, r.Command, name == "pi", logger
			factory.CredentialHosts = factory.CredentialHosts || r.CredentialHosts
			factory.MaxShared, factory.MaxActiveTurns, factory.MaxHosts, factory.MaxStarting = r.MaxShared, r.MaxActiveTurns, r.MaxHosts, r.MaxStarting
			factories[name] = factory
		}
	}
	l := o.Config.Limits
	s.engine, err = session.New(session.Options{MediaOrigins: o.Config.MediaOrigins, Store: db, Keys: o.Keys, Tools: registry, Factories: factories, Provider: o.resolveModel, MaxActive: l.Active, MaxLoaded: l.Loaded, MaxQueue: l.Queue, MaxInputBytes: l.InputBytes, MaxLogBytes: l.SessionBytes, Logger: logger, Metrics: metrics, Archive: archive, SyncQueue: queue, SyncInterval: interval, Inspection: func(s core.Session) (string, string, func()) {
		token, revoke := auth.Delegate(s)
		return token, o.Config.PublicURL, revoke
	}})
	if err != nil {
		return nil, err
	}
	apiServer := &api.Server{Bridge: bridge, Engine: s.engine, Tools: registry, Auth: auth, Limits: l, Metrics: metrics, Logger: logger}
	s.http = &http.Server{Addr: o.Config.Listen, Handler: apiServer.Handler(), ReadHeaderTimeout: 10 * time.Second, ReadTimeout: 30 * time.Second, IdleTimeout: 90 * time.Second, MaxHeaderBytes: 16 << 10}
	return s, nil
}

func (s *Server) Handler() http.Handler { return s.http.Handler }

func (s *Server) ReadSession(ctx context.Context, tenant, id string) (Session, error) {
	value, err := s.engine.Get(ctx, core.Grant{Tenant: tenant, ReadOnly: true}, id)
	value.Credentials = nil
	return value, err
}

func (s *Server) DeleteSession(ctx context.Context, tenant, id string) error {
	return s.engine.Delete(ctx, core.Grant{Tenant: tenant, Manage: true}, id)
}
func (s *Server) ReadEvents(ctx context.Context, tenant, id, after string, limit int) ([]Event, error) {
	log, release, err := s.engine.Log(ctx, core.Grant{Tenant: tenant, ReadOnly: true}, id)
	if err != nil {
		return nil, err
	}
	defer release()
	return log.Read(after, limit, -1)
}

func (s *Server) Run(ctx context.Context) error {
	listener, err := net.Listen("tcp", s.http.Addr)
	if err != nil {
		drain, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		defer cancel()
		return errors.Join(err, s.Shutdown(drain))
	}
	return s.Serve(ctx, listener)
}

// Serve takes ownership of the listener and shuts down when ctx is cancelled.
func (s *Server) Serve(ctx context.Context, listener net.Listener) error {
	done := make(chan error, 1)
	go func() { done <- s.http.Serve(listener) }()
	var err error
	select {
	case <-ctx.Done():
	case err = <-done:
		if errors.Is(err, http.ErrServerClosed) {
			err = nil
		}
	}
	drain, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	return errors.Join(err, s.Shutdown(drain))
}

func (s *Server) Shutdown(ctx context.Context) error {
	s.once.Do(func() {
		s.err = s.engine.Shutdown(ctx)
		s.bridge.Close()
		if err := s.http.Shutdown(ctx); err != nil {
			s.err = errors.Join(s.err, err, s.http.Close())
		}
		for i := len(s.close) - 1; i >= 0; i-- {
			s.err = errors.Join(s.err, s.close[i]())
		}
	})
	return s.err
}

func (s *Server) RevokeCredential(ctx context.Context, tenant, sessionID, binding string) error {
	return s.engine.ChangeCredentials(ctx, tenant, sessionID, func(current core.Session) error {
		return s.db.Metadata.(*store.SQLite).RevokeCredential(ctx, tenant, sessionID, binding)
	})
}

// RotateCredentials re-encrypts live overrides using the application's active key.
// Keep old keys until every retained checkpoint that needs them has expired.
func (s *Server) RotateCredentials(ctx context.Context, tenant, sessionID string) error {
	return s.engine.ChangeCredentials(ctx, tenant, sessionID, func(current core.Session) error {
		if err := credentials.Validate(ctx, s.keys, current); err != nil {
			return err
		}
		inputs := []core.CredentialInput{}
		for _, record := range current.Credentials {
			if record.Revoked {
				continue
			}
			value, err := credentials.Open(ctx, s.keys, current, record.Binding)
			if err != nil {
				return err
			}
			inputs = append(inputs, core.CredentialInput{Binding: record.Binding, Value: value})
		}
		sealed, err := credentials.Seal(ctx, s.keys, current, inputs)
		if err != nil {
			return err
		}
		current.Credentials = sealed
		return s.db.Metadata.(*store.SQLite).RotateCredentials(ctx, current)
	})
}
