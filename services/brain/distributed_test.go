package brain

import (
	"context"
	"github.com/aexhq/brain/services/brain/internal/config"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDistributedStartupDoesNotOwnOrCleanLocalStorage(t *testing.T) {
	t.Setenv("TEST_SERVICE_TOKEN", strings.Repeat("x", 32))
	cfg := DefaultConfig()
	cfg.Services = &config.Services{Storage: "http://127.0.0.1:1", Tools: "http://127.0.0.1:2", Sandbox: "http://127.0.0.1:3", TokenEnv: "TEST_SERVICE_TOKEN", Owner: "brain", Epoch: 1}
	cfg.Harnesses = map[string]ProcessConfig{"pi": {}}
	dir := t.TempDir()
	working := filepath.Join(dir, "work")
	if err := os.Mkdir(working, 0700); err != nil {
		t.Fatal(err)
	}
	sentinel := filepath.Join(working, "another-owner")
	if err := os.WriteFile(sentinel, []byte("retained"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, root := range []string{dir, dir, "", filepath.Join(dir, "absent")} {
		cfg.DataDir = root
		s, err := New(context.Background(), Options{Config: cfg, Authenticate: func(*http.Request) (Grant, bool) { return Grant{Tenant: "test", Manage: true}, true }})
		if err != nil {
			t.Fatal(err)
		}
		defer s.Shutdown(context.Background())
	}
	data, err := os.ReadFile(sentinel)
	if err != nil || string(data) != "retained" {
		t.Fatalf("Brain changed another owner's files: %q %v", data, err)
	}
	for _, name := range []string{"owner.lock", "absent"} {
		if _, err := os.Stat(filepath.Join(dir, name)); !os.IsNotExist(err) {
			t.Fatalf("Brain created storage %s: %v", name, err)
		}
	}
}
