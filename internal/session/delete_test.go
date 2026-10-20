package session

import (
	"context"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/persistence"
	"os"
	"testing"
	"time"
)

func TestDeleteCancelsActiveWorkErasesFilesAndCannotBeRestored(t *testing.T) {
	e, f, g := setup(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	s := create(t, e, g)
	if _, err := e.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "block"}); err != nil {
		t.Fatal(err)
	}
	<-f.started
	for _, grant := range []core.Grant{{Tenant: "b", Manage: true}, {Tenant: g.Tenant, ReadOnly: true}} {
		if err := e.Delete(ctx, grant, s.ID); err == nil {
			t.Fatal("unauthorized deletion")
		}
	}
	log, release, err := e.Log(ctx, g, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	sub, _ := log.Subscribe(8, 1<<20)
	defer sub.Close()
	if err := e.Delete(ctx, g, s.ID); err != nil {
		t.Fatal(err)
	}
	select {
	case <-sub.Done:
	default:
		t.Fatal("event stream stayed open")
	}
	if _, err := e.Get(ctx, g, s.ID); core.ErrorCode(err) != core.NotFound {
		t.Fatal(err)
	}
	dir, _ := e.opts.Store.Dir(s.ID)
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatalf("files remain: %v", err)
	}
	if err := e.Delete(ctx, g, s.ID); err != nil {
		t.Fatal(err)
	}
	if err := e.opts.Store.Import(ctx, s); err == nil {
		t.Fatal("restored deleted session")
	}
	sessions, err := e.List(ctx, g, "", 10)
	if err != nil || len(sessions) != 0 {
		t.Fatalf("deleted session listed: %v %v", sessions, err)
	}
}

func TestDeleteRemovesArchiveAndFreshOwnersCannotRestore(t *testing.T) {
	objects := persistence.FileObjects{Root: t.TempDir()}
	e, _ := persistentEngine(t, objects)
	g := core.Grant{Tenant: "a", Manage: true}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	s := create(t, e, g)
	if _, err := e.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "retained"}); err != nil {
		t.Fatal(err)
	}
	waitStatus(t, e, g, s.ID, "idle")
	checkpoint, err := e.Flush(ctx, g, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Delete(ctx, g, s.ID); err != nil {
		t.Fatal(err)
	}
	fresh, factory := persistentEngine(t, objects)
	if _, err := fresh.Get(ctx, g, s.ID); core.ErrorCode(err) != core.NotFound {
		t.Fatal(err)
	}
	if _, err := fresh.Restore(ctx, g, s.ID, checkpoint.Checkpoint); core.ErrorCode(err) != core.NotFound {
		t.Fatal(err)
	}
	if factory.opens.Load() != 0 {
		t.Fatal("restoration invoked inference")
	}
}
