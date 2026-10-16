package store

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
)

// Run the same record contract through the database and workspace boundaries.
func TestMetadataContract(t *testing.T) {
	for _, workspace := range []bool{false, true} {
		name := "sqlite"
		if workspace {
			name = "workspace"
		}
		t.Run(name, func(t *testing.T) {
			root := t.TempDir()
			db, err := OpenSQLite(filepath.Join(root, "metadata.sqlite"))
			if err != nil {
				t.Fatal(err)
			}
			var m Metadata = db
			if workspace {
				m, err = New(root, db)
				if err != nil {
					t.Fatal(err)
				}
			}
			t.Cleanup(func() { m.Close() })
			ctx := context.Background()
			s := core.Session{ID: core.ID(), Tenant: "one", Status: "idle", CreatedAt: time.Now().UTC()}
			if _, err := m.Get(ctx, s.ID); core.ErrorCode(err) != core.NotFound {
				t.Fatalf("missing: %v", err)
			}
			if err := m.Save(ctx, s); core.ErrorCode(err) != core.NotFound {
				t.Fatalf("missing save: %v", err)
			}
			if err := m.Create(ctx, s); err != nil {
				t.Fatal(err)
			}
			if err := m.Create(ctx, s); err == nil {
				t.Fatal("duplicate create succeeded")
			}
			other := s
			other.Tenant, other.Status = "two", "closed"
			if err := m.Save(ctx, other); core.ErrorCode(err) != core.NotFound {
				t.Fatalf("tenant save: %v", err)
			}
			if err := m.Import(ctx, other); core.ErrorCode(err) != core.Forbidden {
				t.Fatalf("tenant import: %v", err)
			}
			got, err := m.Get(ctx, s.ID)
			if err != nil || got.Tenant != "one" || got.Status != "idle" {
				t.Fatalf("cross-tenant write: %+v %v", got, err)
			}
			rows, err := m.List(ctx, "two", "", 10)
			if err != nil || len(rows) != 0 {
				t.Fatalf("tenant list: %+v %v", rows, err)
			}
			for i := 0; i < 3; i++ {
				s.ID = core.ID()
				if err := m.Import(ctx, s); err != nil {
					t.Fatal(err)
				}
			}
			cursor, count := "", 0
			for {
				rows, err := m.Scan(ctx, cursor, 1)
				if err != nil {
					t.Fatal(err)
				}
				if len(rows) == 0 {
					break
				}
				if len(rows) != 1 || rows[0].ID <= cursor {
					t.Fatal("unordered scan")
				}
				cursor = rows[0].ID
				count++
			}
			if count != 4 {
				t.Fatalf("scan count %d", count)
			}
			cancelled, cancel := context.WithCancel(ctx)
			cancel()
			if err := m.Save(cancelled, s); !errors.Is(err, context.Canceled) {
				t.Fatalf("cancel: %v", err)
			}
		})
	}
}

type failingMetadata struct {
	Metadata
	failure error
}

func (m failingMetadata) Create(context.Context, core.Session) error { return m.failure }

func TestWorkspacePreparesFilesAndPropagatesDatabaseFailure(t *testing.T) {
	failure := errors.New("database unavailable")
	s, err := New(t.TempDir(), failingMetadata{failure: failure})
	if err != nil {
		t.Fatal(err)
	}
	id := core.ID()
	if err := s.Create(context.Background(), core.Session{ID: id}); !errors.Is(err, failure) {
		t.Fatal(err)
	}
	dir, err := s.Dir(id)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(dir, "native")); err != nil {
		t.Fatal(err)
	}
}
