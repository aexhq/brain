package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"io"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/aexhq/brain/credentials"
	public "github.com/aexhq/brain/services/brain/api"
	sandbox "github.com/aexhq/brain/services/sandbox/api"
	storage "github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/tools/internal/hosted"
	"github.com/aexhq/brain/services/tools/internal/server"
	"github.com/aexhq/brain/services/tools/registry"
	"github.com/aexhq/brain/telemetry"
	"github.com/aexhq/brain/tools/integrations"
)

type settings struct {
	ProvidersFile string                 `json:"providers_file,omitempty"`
	Listen        string                 `json:"listen"`
	Storage       string                 `json:"storage"`
	Sandbox       string                 `json:"sandbox"`
	TokenEnv      string                 `json:"token_env"`
	Tokens        []public.Token         `json:"tokens"`
	Tools         []tools.Binding        `json:"tools"`
	Keys          *credentials.KeyConfig `json:"credential_keys,omitempty"`
	Executors     int                    `json:"executors"`
}

func main() {
	if e := run(); e != nil {
		slog.Error("tools stopped", "error", e)
		os.Exit(1)
	}
}
func run() error {
	file := flag.String("config", "tools.json", "tools configuration")
	flag.Parse()
	f, e := os.Open(*file)
	if e != nil {
		return e
	}
	defer f.Close()
	c := settings{Listen: ":8082", TokenEnv: "BRAIN_SERVICE_TOKEN", Executors: 1024}
	d := json.NewDecoder(io.LimitReader(f, 4<<20))
	d.DisallowUnknownFields()
	if e = d.Decode(&c); e != nil {
		return e
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing configuration data")
	}
	if c.Storage == "" || c.Sandbox == "" || c.Executors < 1 {
		return errors.New("tools service dependencies and executor capacity are required")
	}
	for _, t := range c.Tokens {
		if len(os.Getenv(t.Env)) < 24 || t.Tenant == "" {
			return errors.New("invalid tools authentication grant")
		}
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	_, shutdown, e := telemetry.SetupService(ctx, "tools")
	if e != nil {
		return e
	}
	defer func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = shutdown(ctx)
	}()
	r, e := tools.New(c.Tools)
	if e != nil {
		return e
	}
	defer r.Close()
	bridge := tools.NewBridge(c.Executors)
	defer bridge.Close()
	r.Runtime = bridge
	if c.Keys != nil {
		if _, _, e = c.Keys.Active(ctx); e != nil {
			return e
		}
		r.Keys = c.Keys
	}
	token := os.Getenv(c.TokenEnv)
	st := &storage.Client{URL: c.Storage, Token: token}
	hosted := &hosted.Service{Storage: st, Sandbox: &sandbox.Client{URL: c.Sandbox, Token: token}}
	r.Providers = map[string]tools.Provider{"hosted": hosted.Provider()}
	if c.ProvidersFile != "" {
		providers, e := integrations.LoadBuiltins(c.ProvidersFile)
		if e != nil {
			return e
		}
		for name, provider := range providers {
			if _, exists := r.Providers[name]; exists {
				return errors.New("duplicate tool provider")
			}
			r.Providers[name] = provider
		}
	}
	h, e := (&server.Server{Registry: r, Bridge: bridge, Hosted: hosted, Storage: st, Auth: public.NewAuth(c.Tokens), Token: token}).Handler()
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
