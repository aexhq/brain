package store

import (
	"context"
	core "github.com/aexhq/brain/services/brain/api"
	"os"
	"path/filepath"
	"testing"
)

func TestRestoreHandlesCannotMoveOrDeleteOutsideStorage(t *testing.T) {
	s, err := Open(t.TempDir(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	outside := t.TempDir()
	sentinel := filepath.Join(outside, "retained")
	if err = os.WriteFile(sentinel, []byte("owned elsewhere"), 0600); err != nil {
		t.Fatal(err)
	}
	session := core.Session{Tenant: "tenant", ID: "session"}
	for _, name := range []string{outside, "../.restore-other", "sessions", ".restore-child/../../outside", "", "."} {
		if err = s.DiscardRestore(name); err == nil {
			t.Fatalf("discard accepted %q", name)
		}
		if err = s.InstallRestore(context.Background(), session, name); err == nil {
			t.Fatalf("install accepted %q", name)
		}
	}
	data, err := os.ReadFile(sentinel)
	if err != nil || string(data) != "owned elsewhere" {
		t.Fatalf("external data changed: %q %v", data, err)
	}
	staging, err := os.MkdirTemp(s.root, ".restore-")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.DiscardRestore(filepath.Base(staging)); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(staging); !os.IsNotExist(err) {
		t.Fatalf("staging was not discarded: %v", err)
	}
}
