package sessiontools

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/tools"
	"strings"
	"testing"
)

type reader struct{ reads int }

func (r *reader) ReadSession(_ context.Context, tenant, id string) (brain.Session, error) {
	if tenant != "a" || id == strings.Repeat("f", 32) {
		return brain.Session{}, errors.New("foreign")
	}
	r.reads++
	return brain.Session{ID: id, Credentials: []core.CredentialRecord{{Ciphertext: []byte("private")}}}, nil
}
func (r *reader) ReadEvents(context.Context, string, string, string, int) ([]brain.Event, error) {
	return []brain.Event{}, nil
}

func TestSessionsProviderScopesTargetsAndExcludesPrivateState(t *testing.T) {
	ctx := context.Background()
	reader := &reader{}
	allowed := strings.Repeat("a", 32)
	foreign := strings.Repeat("f", 32)
	p := Sessions(reader, func(_ context.Context, b brain.BindingContext, target string) error {
		if target != allowed && target != foreign {
			return errors.New("forbidden target")
		}
		return nil
	})
	registry, err := tools.New(nil)
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	registry.Providers = map[string]tools.Provider{"sessions": p}
	s := brain.Session{ID: core.ID(), Tenant: "a", Config: brain.SessionConfig{Bindings: []brain.ToolBinding{{Alias: "history", Provider: "sessions", Operations: []string{"read"}}}}}
	if err := registry.Bind(ctx, &s); err != nil {
		t.Fatal(err)
	}
	for _, target := range []string{allowed, strings.Repeat("b", 32), foreign} {
		args, _ := json.Marshal(map[string]string{"session": target})
		result, err := registry.Invoke(ctx, s, brain.ToolScope{Tenant: "a", Session: s.ID, Call: core.ID()}, brain.Call{Name: "history_read", Arguments: args}, nil)
		if err != nil {
			t.Fatal(err)
		}
		if result.IsError != (target != allowed) {
			t.Fatalf("target %s: %+v", target, result)
		}
		if strings.Contains(string(result.StructuredContent), "private") || strings.Contains(string(result.StructuredContent), "credentials") {
			t.Fatal("private state leaked")
		}
	}
	if reader.reads != 1 {
		t.Fatal("unauthorized read executed")
	}
}
