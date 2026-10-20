// Package config validates host-owned configuration before opening a listener.
package config

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/persistence"
	"github.com/aexhq/brain/internal/tools"
)

type Token struct {
	Env string `json:"token_env"`
	core.Grant
}
type Provider struct {
	APIKey    string `json:"-"`
	APIKeyEnv string `json:"api_key_env,omitempty"`
	BaseURL   string `json:"base_url,omitempty"`
	API       string `json:"api,omitempty"`
}
type Runner struct {
	CredentialHosts bool     `json:"credential_hosts,omitempty"`
	Command         []string `json:"command"`
	MaxShared       int      `json:"max_shared,omitempty"`
	MaxActiveTurns  int      `json:"max_active_turns,omitempty"`
	MaxHosts        int      `json:"max_hosts,omitempty"`
	MaxStarting     int      `json:"max_starting,omitempty"`
}

// KeyConfig names deployment environment variables, never encryption key values.
type KeyConfig struct {
	ActiveID     string            `json:"active"`
	Environments map[string]string `json:"keys"`
}

func (k *KeyConfig) Active(ctx context.Context) (string, []byte, error) {
	key, err := k.Get(ctx, k.ActiveID)
	return k.ActiveID, key, err
}
func (k *KeyConfig) Get(_ context.Context, id string) ([]byte, error) {
	env := k.Environments[id]
	if env == "" {
		return nil, errors.New("credential key is not configured")
	}
	key, err := base64.StdEncoding.DecodeString(os.Getenv(env))
	if err != nil || len(key) != 32 {
		return nil, errors.New("credential key must be base64-encoded 32 bytes")
	}
	return key, nil
}

type Limits struct {
	Active           int   `json:"active"`
	Loaded           int   `json:"loaded"`
	Queue            int   `json:"queue"`
	InputBytes       int   `json:"input_bytes"`
	SessionBytes     int64 `json:"session_bytes"`
	Subscribers      int   `json:"subscribers"`
	SubscriberEvents int   `json:"subscriber_events"`
	SubscriberBytes  int64 `json:"subscriber_bytes"`
}
type Config struct {
	MediaOrigins   []string              `json:"media_origins,omitempty"`
	HostPool       harness.PoolLimits    `json:"host_pool,omitempty"`
	CredentialKeys *KeyConfig            `json:"credential_keys,omitempty"`
	Listen         string                `json:"listen"`
	PublicURL      string                `json:"public_url"`
	DataDir        string                `json:"data_dir"`
	Tokens         []Token               `json:"tokens"`
	Harnesses      map[string]Runner     `json:"harnesses"`
	Providers      map[string]Provider   `json:"providers"`
	Tools          []tools.Binding       `json:"tools"`
	Limits         Limits                `json:"limits"`
	Persistence    *persistence.Settings `json:"persistence,omitempty"`
}

func Default() Config {
	return Config{Listen: "127.0.0.1:8080", PublicURL: "http://127.0.0.1:8080", DataDir: ".brain", Limits: Limits{Active: 16, Loaded: 1024, Queue: 16, InputBytes: 64 << 10, SessionBytes: 1 << 30, Subscribers: 128, SubscriberEvents: 128, SubscriberBytes: 2 << 20}}
}

func Load(file string) (Config, error) {
	c := Default()
	f, err := os.Open(file)
	if err != nil {
		return c, err
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil {
		return c, err
	}
	if info.Size() > 4<<20 {
		return c, errors.New("configuration exceeds limit")
	}
	d := json.NewDecoder(io.LimitReader(f, 4<<20))
	d.DisallowUnknownFields()
	if err := d.Decode(&c); err != nil {
		return c, err
	}
	if d.Decode(new(any)) != io.EOF {
		return c, errors.New("trailing configuration data")
	}
	if err := c.Validate(); err != nil {
		return c, err
	}
	return c, nil
}

func (c *Config) Validate() error { return c.ValidateFactories(nil) }

func (c *Config) ValidateFactories(factories map[string]harness.Factory) error {
	return c.ValidateApplication(factories, false)
}

func (c *Config) ValidateApplication(factories map[string]harness.Factory, externalAuth bool) error {
	for _, origin := range c.MediaOrigins {
		u, err := url.Parse(origin)
		if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil || u.Path != "" || u.RawQuery != "" || u.Fragment != "" {
			return errors.New("media_origins must contain exact HTTP origins")
		}
	}
	if err := c.HostPool.Validate(); err != nil {
		return err
	}
	if c.CredentialKeys != nil {
		if _, _, err := c.CredentialKeys.Active(context.Background()); err != nil {
			return err
		}
		for id := range c.CredentialKeys.Environments {
			if _, err := c.CredentialKeys.Get(context.Background(), id); err != nil {
				return err
			}
		}
	}
	if c.Persistence != nil {
		if err := c.Persistence.Validate(); err != nil {
			return err
		}
	}
	if _, _, err := net.SplitHostPort(c.Listen); err != nil {
		return fmt.Errorf("listen: %w", err)
	}
	if c.DataDir == "" {
		return errors.New("data_dir is required")
	}
	abs, err := filepath.Abs(c.DataDir)
	if err != nil {
		return err
	}
	c.DataDir = abs
	u, err := url.Parse(c.PublicURL)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" {
		return errors.New("invalid public_url")
	}
	if len(c.Tokens) == 0 && !externalAuth {
		return errors.New("at least one authentication token is required")
	}
	seen := make(map[string]bool)
	for _, t := range c.Tokens {
		value := os.Getenv(t.Env)
		if t.Env == "" || len(value) < 24 || t.Tenant == "" || len(t.Tenant) > 128 || (t.Session != "" && !core.ValidID(t.Session)) {
			return fmt.Errorf("invalid grant or missing token environment variable %q (minimum 24 bytes)", t.Env)
		}
		if seen[value] {
			return errors.New("duplicate authentication token")
		}
		seen[value] = true
	}
	if len(c.Harnesses) == 0 {
		return errors.New("at least one harness is required")
	}
	for name, r := range c.Harnesses {
		if r.MaxShared == 0 {
			r.MaxShared = 8
		}
		if r.MaxActiveTurns == 0 {
			r.MaxActiveTurns = r.MaxShared
		}
		if r.MaxHosts == 0 {
			r.MaxHosts = 16
		}
		if r.MaxStarting == 0 {
			r.MaxStarting = 4
		}
		if r.MaxShared < 1 || r.MaxShared > 1024 || r.MaxActiveTurns < 1 || r.MaxActiveTurns > r.MaxShared || r.MaxHosts < 1 || r.MaxStarting < 1 {
			return fmt.Errorf("invalid host limits for %s", name)
		}
		c.Harnesses[name] = r
		if name == "" || len(name) > 64 {
			return fmt.Errorf("invalid harness name %q", name)
		}
		if factories[name] == nil && (len(r.Command) == 0 || r.Command[0] == "") {
			return fmt.Errorf("harness %s has no command", name)
		}
	}
	for name, p := range c.Providers {
		if name == "" || (p.APIKeyEnv != "" && os.Getenv(p.APIKeyEnv) == "") || (p.APIKeyEnv != "" && p.APIKey != "") {
			return fmt.Errorf("provider %q requires its configured API key", name)
		}
		if p.BaseURL != "" {
			u, err := url.Parse(p.BaseURL)
			if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil {
				return fmt.Errorf("provider %q has an invalid base URL", name)
			}
		}
	}
	l := c.Limits
	if l.Active < 1 || l.Loaded < l.Active || l.Queue < 1 || l.Queue > 1024 || l.InputBytes < 1 || l.InputBytes > 1<<20 || l.SessionBytes < 16<<20 || l.Subscribers < 1 || l.SubscriberEvents < 1 || l.SubscriberBytes < 4096 {
		return errors.New("invalid resource limits")
	}
	return nil
}

func (c Config) Provider(model string) (harness.Provider, error) {
	return c.ModelProvider(model, nil)
}

func (c Config) ModelProvider(model string, override *string) (harness.Provider, error) {
	name, id, ok := strings.Cut(model, "/")
	if !ok || id == "" {
		return harness.Provider{}, core.E(core.Invalid, "model must be provider/model")
	}
	p, ok := c.Providers[name]
	if !ok {
		return harness.Provider{}, core.E(core.Invalid, "model provider is not configured")
	}
	key := p.APIKey
	if p.APIKeyEnv != "" {
		key = os.Getenv(p.APIKeyEnv)
	}
	if override != nil {
		key = *override
	}
	if key == "" {
		return harness.Provider{}, core.E(core.Invalid, "model credentials are unavailable")
	}
	return harness.Provider{APIKey: key, BaseURL: p.BaseURL, API: p.API}, nil
}
