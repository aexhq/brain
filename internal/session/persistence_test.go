package session

import (
	"context"
	"errors"
	"io"
	"log/slog"
	"sync/atomic"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/persistence"
	"github.com/aexhq/brain/internal/store"
	"github.com/aexhq/brain/internal/tools"
)

func persistentEngine(t *testing.T, objects persistence.Objects) (*Engine, *testFactory) {
	t.Helper()
	db, err := store.Open(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	r, err := tools.New(nil)
	if err != nil {
		t.Fatal(err)
	}
	f := &testFactory{started: make(chan string, 32)}
	e, err := New(Options{Store: db, Tools: r, Factories: map[string]harness.Factory{"pi": f}, Provider: func(string) (harness.Provider, error) { return harness.Provider{}, nil }, MaxActive: 1, MaxLoaded: 2, MaxQueue: 2, MaxInputBytes: 4096, MaxLogBytes: 16 << 20, Archive: &persistence.Archive{Objects: objects, MaxSessionBytes: 16 << 20}, SyncQueue: 2, SyncInterval: 20 * time.Millisecond, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := e.Shutdown(ctx); err != nil {
			t.Error(err)
		}
		r.Close()
		db.Close()
	})
	until(t, func() bool { return e.syncer.admit() })
	return e, f
}
func until(t *testing.T, fn func() bool) {
	t.Helper()
	deadline := time.NewTimer(5 * time.Second)
	defer deadline.Stop()
	tick := time.NewTicker(time.Millisecond)
	defer tick.Stop()
	for !fn() {
		select {
		case <-deadline.C:
			t.Fatal("condition did not become true")
		case <-tick.C:
		}
	}
}

func TestExternalRestoreIsPassiveAndScoped(t *testing.T) {
	objects := persistence.FileObjects{Root: t.TempDir()}
	e, _ := persistentEngine(t, objects)
	g := core.Grant{Tenant: "a", Manage: true}
	ctx := context.Background()
	s := create(t, e, g)
	if _, err := e.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "retained"}); err != nil {
		t.Fatal(err)
	}
	waitStatus(t, e, g, s.ID, "idle")
	flushed, err := e.Flush(ctx, g, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if flushed.Checkpoint == "" || flushed.HistoryBytes == 0 || flushed.ResumeOffset == 0 {
		t.Fatalf("missing boundaries: %+v", flushed)
	}
	fresh, factory := persistentEngine(t, objects)
	if _, err := fresh.Get(ctx, core.Grant{Tenant: "b", Manage: true}, s.ID); core.ErrorCode(err) != core.NotFound {
		t.Fatalf("cross-tenant recovery: %v", err)
	}
	restored, err := fresh.Get(ctx, g, s.ID)
	if err != nil || restored.Status != "interrupted" {
		t.Fatalf("restore: %+v %v", restored, err)
	}
	if factory.opens.Load() != 0 {
		t.Fatal("restore activated harness")
	}
	if _, err := fresh.Resume(ctx, g, s.ID); err != nil {
		t.Fatal(err)
	}
	if factory.opens.Load() != 0 {
		t.Fatal("resume activated harness")
	}
	log, release, err := fresh.Log(ctx, g, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	events, err := log.Read("", 100, -1)
	if err != nil || len(events) < 3 {
		t.Fatalf("missing retained history: %v", err)
	}
	if _, err := fresh.Restore(ctx, g, s.ID, flushed.Checkpoint); core.ErrorCode(err) != core.Conflict {
		t.Fatalf("restored while a reader held the old log: %v", err)
	}
	release()
	until(t, func() bool {
		_, err := fresh.Restore(ctx, g, s.ID, flushed.Checkpoint)
		if err != nil && core.ErrorCode(err) != core.Conflict {
			t.Fatal(err)
		}
		return err == nil
	})
	if factory.opens.Load() != 0 {
		t.Fatal("explicit restore activated harness")
	}
}

type outageObjects struct {
	persistence.Objects
	down atomic.Bool
}

func (o *outageObjects) Put(ctx context.Context, key string, b []byte, immutable bool) error {
	if o.down.Load() {
		return errors.New("storage unavailable")
	}
	return o.Objects.Put(ctx, key, b, immutable)
}
func TestSynchronizationBackpressureAndRecovery(t *testing.T) {
	o := &outageObjects{Objects: persistence.FileObjects{Root: t.TempDir()}}
	o.down.Store(true)
	e, _ := persistentEngine(t, o)
	g := core.Grant{Tenant: "a", Manage: true}
	ctx := context.Background()
	one := create(t, e, g)
	_ = create(t, e, g)
	if _, err := e.Create(ctx, g, core.CreateSession{Harness: "pi", Config: core.Config{Model: "test/model"}}); core.ErrorCode(err) != core.Overloaded {
		t.Fatalf("admitted during full backlog: %v", err)
	}
	if _, err := e.Flush(ctx, g, one.ID); err == nil {
		t.Fatal("outage flush succeeded")
	}
	o.down.Store(false)
	if _, err := e.Flush(ctx, g, one.ID); err != nil {
		t.Fatal(err)
	}
	until(t, e.syncer.admit)
	if _, err := e.Create(ctx, g, core.CreateSession{Harness: "pi", Config: core.Config{Model: "test/model"}}); err != nil {
		t.Fatal(err)
	}
}

func TestIdleCacheEvictionRetainsPinnedLog(t *testing.T) {
	e, _, g := setup(t)
	e.opts.MaxLoaded = 2
	ctx := context.Background()
	one := create(t, e, g)
	log, release, err := e.Log(ctx, g, one.ID)
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	for range 8 {
		s := create(t, e, g)
		if _, err := e.Get(ctx, g, s.ID); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := log.Read("", 10, -1); err != nil {
		t.Fatalf("evicted pinned reader: %v", err)
	}
	e.mu.Lock()
	count := len(e.entries)
	e.mu.Unlock()
	if count != 2 {
		t.Fatalf("unbounded loaded entries: %d", count)
	}
}
