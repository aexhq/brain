//go:build postgres

package persistence

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
)

func TestPostgresContract(t *testing.T) {
	dsn := os.Getenv("BRAIN_TEST_POSTGRES")
	if dsn == "" {
		t.Fatal("BRAIN_TEST_POSTGRES is required for the PostgreSQL profile")
	}
	p, err := NewPostgres(dsn)
	if err != nil {
		t.Fatal(err)
	}
	defer p.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	tenant := core.ID()
	now := time.Now().UTC()
	s := core.Session{ID: core.ID(), Tenant: tenant, Harness: "pi", Status: "idle", CreatedAt: now, UpdatedAt: now, Config: core.Config{Model: "test/model"}}
	ref := core.ID()
	if err := p.Put(ctx, s, ref); err != nil {
		t.Fatal(err)
	}
	defer p.db.ExecContext(context.Background(), "DELETE FROM brain_sessions WHERE tenant=$1", tenant)
	got, checkpoint, err := p.Get(ctx, tenant, s.ID)
	if err != nil || got.ID != s.ID || got.Tenant != tenant || checkpoint != ref {
		t.Fatalf("read: %+v %s %v", got, checkpoint, err)
	}
	if _, _, err := p.Get(ctx, "another-tenant", s.ID); core.ErrorCode(err) != core.NotFound {
		t.Fatalf("cross-tenant read: %v", err)
	}
	s.Status = "closed"
	if err := p.Put(ctx, s, ref); err != nil {
		t.Fatal(err)
	}
	list, err := p.List(ctx, tenant, "", 100)
	if err != nil || len(list) != 1 || list[0].Status != "closed" {
		t.Fatalf("list: %+v %v", list, err)
	}
	s.Tenant = "another-tenant"
	if err := p.Put(ctx, s, ref); core.ErrorCode(err) != core.Forbidden {
		t.Fatalf("cross-tenant overwrite: %v", err)
	}
}
