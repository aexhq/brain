package brain

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
)

type modelKeys struct{}

func (modelKeys) Active(context.Context) (string, []byte, error) {
	return "test", bytes.Repeat([]byte{7}, 32), nil
}
func (modelKeys) Get(context.Context, string) ([]byte, error) {
	return bytes.Repeat([]byte{7}, 32), nil
}

type modelFactory struct{ opened chan HarnessOptions }

func (*modelFactory) SupportsSteering() bool { return false }
func (f *modelFactory) Open(_ context.Context, o HarnessOptions) (Harness, error) {
	f.opened <- o
	return &modelRunner{o: o}, nil
}

type modelRunner struct{ o HarnessOptions }

func (r *modelRunner) Prompt(context.Context, Message) error {
	return r.o.Event("assistant.message", json.RawMessage(`{"text":"done"}`))
}
func (*modelRunner) Steer(context.Context, Message) error             { return errors.New("unsupported") }
func (*modelRunner) Interrupt(context.Context) error                  { return nil }
func (*modelRunner) Checkpoint(context.Context) ([]NativeFile, error) { return []NativeFile{}, nil }
func (*modelRunner) Close() error                                     { return nil }

func modelOptions(t *testing.T) (Options, *modelFactory) {
	t.Helper()
	cfg := DefaultConfig()
	cfg.DataDir = t.TempDir()
	cfg.Persistence = &Persistence{Provider: "filesystem", Directory: t.TempDir(), IntervalSeconds: 3600, Queue: 32}
	f := &modelFactory{opened: make(chan HarnessOptions, 16)}
	return Options{Config: cfg, Keys: modelKeys{}, Models: map[string]ModelProvider{"test": NewModelProvider("startup-key")},
		Harnesses: map[string]HarnessFactory{"test": f}, Authenticate: func(r *http.Request) (Grant, bool) {
			return Grant{Tenant: "one", Manage: true}, r.Header.Get("Authorization") == "Bearer app-token"
		}}, f
}

func openModelServer(t *testing.T, o Options) *Server {
	t.Helper()
	s, err := New(context.Background(), o)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := s.Shutdown(context.Background()); err != nil {
			t.Error(err)
		}
	})
	deadline := time.Now().Add(5 * time.Second)
	for !s.engine.Ready() {
		if time.Now().After(deadline) {
			t.Fatal("server did not become ready")
		}
		time.Sleep(time.Millisecond)
	}
	return s
}

func modelTurn(t *testing.T, s *Server, id, status string) {
	t.Helper()
	g := Grant{Tenant: "one", Manage: true}
	if _, err := s.engine.Submit(context.Background(), g, id, core.Input{Type: "user.message", Text: "run"}); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		got, err := s.engine.Get(context.Background(), g, id)
		if err != nil {
			t.Fatal(err)
		}
		if got.Status == status {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatalf("session did not settle as %s", status)
}

func TestPrivateModelAdmissionRestartColdRestoreAndRevocation(t *testing.T) {
	o, f := modelOptions(t)
	s := openModelServer(t, o)
	ctx := context.Background()
	g := Grant{Tenant: "one", Manage: true}
	create := func(credential *ModelCredential) Session {
		t.Helper()
		value, err := s.engine.Create(ctx, g, CreateSession{Harness: "test", Config: SessionConfig{Model: "test/model"}, ModelCredential: credential})
		if err != nil {
			t.Fatal(err)
		}
		return value
	}
	defaultSession := create(nil)
	override := create(APIKey("private-customer-model-key"))
	for _, test := range []struct {
		s   Session
		key string
	}{{defaultSession, "startup-key"}, {override, "private-customer-model-key"}} {
		modelTurn(t, s, test.s.ID, "idle")
		if got := <-f.opened; got.Provider.APIKey != test.key {
			t.Fatalf("wrong key: %q", got.Provider.APIKey)
		}
	}
	if _, err := s.engine.Flush(ctx, g, override.ID); err != nil {
		t.Fatal(err)
	}
	if err := s.Shutdown(ctx); err != nil {
		t.Fatal(err)
	}
	for _, root := range []string{o.Config.DataDir, o.Config.Persistence.Directory} {
		if err := filepath.WalkDir(root, func(p string, d os.DirEntry, err error) error {
			if err != nil || d.IsDir() {
				return err
			}
			data, err := os.ReadFile(p)
			if bytes.Contains(data, []byte("private-customer-model-key")) {
				t.Errorf("plaintext key in %s", p)
			}
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	s = openModelServer(t, o)
	modelTurn(t, s, override.ID, "idle")
	if got := <-f.opened; got.Provider.APIKey != "private-customer-model-key" {
		t.Fatal("restart used default")
	}
	if err := s.Shutdown(ctx); err != nil {
		t.Fatal(err)
	}
	o.Config.DataDir = t.TempDir()
	s = openModelServer(t, o)
	public, err := s.ReadSession(ctx, "one", override.ID)
	if err != nil {
		t.Fatal(err)
	}
	data, _ := json.Marshal(public)
	if bytes.Contains(data, []byte("private-customer-model-key")) || bytes.Contains(data, []byte("ciphertext")) {
		t.Fatal("private model material in public session")
	}
	if len(f.opened) != 0 {
		t.Fatal("restore activated a harness")
	}
	if _, err := s.engine.Resume(ctx, g, override.ID); err != nil {
		t.Fatal(err)
	}
	modelTurn(t, s, override.ID, "idle")
	if got := <-f.opened; got.Provider.APIKey != "private-customer-model-key" {
		t.Fatal("cold restore used default")
	}
	if err := s.RotateCredentials(ctx, "one", override.ID); err != nil {
		t.Fatal(err)
	}
	if err := s.RevokeModelCredential(ctx, "one", override.ID); err != nil {
		t.Fatal(err)
	}
	modelTurn(t, s, override.ID, "failed")
	if len(f.opened) != 0 {
		t.Fatal("revocation opened a harness")
	}
}

func TestModelOverridesFailClosedAndReferencesAreTenantScoped(t *testing.T) {
	o, f := modelOptions(t)
	provider := o.Models["test"]
	provider.Authorize = func(_ context.Context, scope ModelContext) error {
		if scope.Tenant != "one" || (scope.Credential != nil && scope.Credential.APIKey == "denied") {
			return errors.New("private detail")
		}
		return nil
	}
	provider.Resolve = func(_ context.Context, scope ModelContext) (string, error) {
		if scope.Tenant != "one" || scope.Credential.Reference != "customer/key" {
			return "", errors.New("private detail")
		}
		return "resolved-key", nil
	}
	o.Models["test"] = provider
	s := openModelServer(t, o)
	ctx := context.Background()
	g := Grant{Tenant: "one", Manage: true}
	for _, credential := range []*ModelCredential{{}, {APIKey: "a", Reference: "b"}, APIKey("denied"), CredentialReference("missing")} {
		_, err := s.engine.Create(ctx, g, CreateSession{Harness: "test", Config: SessionConfig{Model: "test/model"}, ModelCredential: credential})
		if err == nil || strings.Contains(err.Error(), "private detail") {
			t.Fatalf("override rejection: %v", err)
		}
	}
	for _, tenant := range []string{"one", "other"} {
		value, err := s.engine.Create(ctx, Grant{Tenant: tenant, Manage: true}, CreateSession{Harness: "test", Config: SessionConfig{Model: "test/model"}, ModelCredential: CredentialReference("customer/key")})
		if tenant == "other" {
			if core.ErrorCode(err) != core.Forbidden {
				t.Fatalf("foreign reference: %v", err)
			}
			continue
		}
		if err != nil {
			t.Fatal(err)
		}
		modelTurn(t, s, value.ID, "idle")
		if got := <-f.opened; got.Provider.APIKey != "resolved-key" {
			t.Fatal("reference used default")
		}
		provider, err := s.engine.Get(ctx, g, value.ID)
		if err != nil {
			t.Fatal(err)
		}
		provider.Credentials = nil
		// A missing encrypted row must fail instead of selecting the startup key.
		if _, err := credentials.Open(ctx, o.Keys, provider, credentials.ModelBinding); err == nil {
			t.Fatal("missing model record accepted")
		}
	}
	// Client-only providers accept keys without a startup default.
	o, f = modelOptions(t)
	o.Models["test"] = NewModelProvider("")
	s = openModelServer(t, o)
	value, err := s.engine.Create(ctx, g, CreateSession{Harness: "test", Config: SessionConfig{Model: "test/model"}, ModelCredential: APIKey("client-only-key")})
	if err != nil {
		t.Fatal(err)
	}
	modelTurn(t, s, value.ID, "idle")
	if got := <-f.opened; got.Provider.APIKey != "client-only-key" {
		t.Fatal("client-only provider used wrong key")
	}
	if _, err := s.engine.Create(ctx, g, CreateSession{Harness: "test", Config: SessionConfig{Model: "test/model"}}); err == nil {
		t.Fatal("missing default accepted")
	}
}

func TestApplicationAuthenticatorScopes(t *testing.T) {
	o, _ := modelOptions(t)
	for _, test := range []struct {
		name    string
		grant   Grant
		allowed bool
		status  int
	}{
		{"manager", Grant{Tenant: "one", Manage: true}, true, 201},
		{"reader", Grant{Tenant: "one", Manage: true, ReadOnly: true}, true, 403},
		{"participant", Grant{Tenant: "one"}, true, 403},
		{"session", Grant{Tenant: "one", Session: core.ID(), Manage: true}, true, 403},
		{"expired", Grant{Tenant: "one", Manage: true}, false, 401},
		{"empty tenant", Grant{Manage: true}, true, 401},
	} {
		t.Run(test.name, func(t *testing.T) {
			o.Config.DataDir = t.TempDir()
			o.Authenticate = func(*http.Request) (Grant, bool) { return test.grant, test.allowed }
			s := openModelServer(t, o)
			r := httptest.NewRequest("POST", "/sessions", strings.NewReader(`{"harness":"test","config":{"model":"test/model","tools":[]}}`))
			w := httptest.NewRecorder()
			s.Handler().ServeHTTP(w, r)
			if w.Code != test.status {
				t.Fatalf("status %d: %s", w.Code, w.Body)
			}
		})
	}
}
