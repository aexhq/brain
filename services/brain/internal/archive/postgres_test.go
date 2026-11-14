//go:build postgres

package persistence

import (
	"context"
	"os"
	"testing"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
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
	defer p.db.ExecContext(context.Background(), "DELETE FROM brain_sessions WHERE tenant=$1 AND id=$2", s.Tenant, core.SessionKey(s.Tenant, s.ID))
	if err := p.Put(ctx, s, ref); err != nil {
		t.Fatalf("independent tenant create: %v", err)
	}
}
