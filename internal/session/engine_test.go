package session

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"sync/atomic"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/store"
	"github.com/aexhq/brain/internal/tools"
)

type testFactory struct {
	started chan string
	opens   atomic.Int32
}

func (f *testFactory) SupportsSteering() bool { return false }
func (f *testFactory) Open(ctx context.Context, o harness.Options) (harness.Runner, error) {
	f.opens.Add(1)
	return &testRunner{opts: o, started: f.started}, nil
}

type testRunner struct {
	opts    harness.Options
	started chan string
}

func (r *testRunner) Prompt(ctx context.Context, text string) error {
	r.started <- text
	if text == "block" {
		<-ctx.Done()
		return ctx.Err()
	}
	data, _ := json.Marshal(core.Text{Text: text})
	return r.opts.Event("assistant.message", data)
}
func (r *testRunner) Steer(context.Context, string) error { return errors.New("unsupported") }
func (r *testRunner) Interrupt(context.Context) error     { return nil }
func (r *testRunner) Checkpoint(context.Context) ([]core.NativeFile, error) {
	return []core.NativeFile{}, nil
}
func (r *testRunner) Close() error { return nil }

func setup(t *testing.T) (*Engine, *testFactory, core.Grant) {
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
	e, err := New(Options{Store: db, Tools: r, Factories: map[string]harness.Factory{"pi": f}, Provider: func(string) (harness.Provider, error) { return harness.Provider{}, nil }, MaxActive: 1, MaxLoaded: 16, MaxQueue: 1, MaxInputBytes: 4096, MaxLogBytes: 16 << 20, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
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
	return e, f, core.Grant{Tenant: "a", Manage: true}
}
func create(t *testing.T, e *Engine, g core.Grant) core.Session {
	t.Helper()
	s, err := e.Create(context.Background(), g, core.CreateSession{Harness: "pi", Config: core.Config{Model: "test/model"}})
	if err != nil {
		t.Fatal(err)
	}
	return s
}
func waitStatus(t *testing.T, e *Engine, g core.Grant, id, status string) {
	t.Helper()
	deadline := time.NewTimer(5 * time.Second)
	defer deadline.Stop()
	tick := time.NewTicker(time.Millisecond)
	defer tick.Stop()
	for {
		got, err := e.Get(context.Background(), g, id)
		if err != nil {
			t.Fatal(err)
		}
		if got.Status == status {
			return
		}
		select {
		case <-deadline.C:
			t.Fatalf("wanted %s, got %s", status, got.Status)
		case <-tick.C:
		}
	}
}

func TestQueueInterruptDedupAndRejectedRetry(t *testing.T) {
	e, f, g := setup(t)
	s := create(t, e, g)
	ctx := context.Background()
	first, err := e.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "block", Key: "first"})
	if err != nil {
		t.Fatal(err)
	}
	select {
	case <-f.started:
	case <-time.After(5 * time.Second):
		t.Fatal("execution did not start")
	}
	retry, err := e.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "block", Key: "first"})
	if err != nil || retry.ID != first.ID {
		t.Fatalf("retry: %+v %v", retry, err)
	}
	if _, err := e.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "different", Key: "first"}); core.ErrorCode(err) != core.Conflict {
		t.Fatal(err)
	}
	if _, err := e.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "queued", Key: "second"}); err != nil {
		t.Fatal(err)
	}
	third := core.Input{Type: "user.message", Text: "third", Key: "third"}
	if _, err := e.Submit(ctx, g, s.ID, third); core.ErrorCode(err) != core.Overloaded {
		t.Fatalf("queue was not bounded: %v", err)
	}
	if err := e.Interrupt(ctx, g, s.ID); err != nil {
		t.Fatal(err)
	}
	waitStatus(t, e, g, s.ID, "interrupted")
	if _, err := e.Resume(ctx, g, s.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := e.Submit(ctx, g, s.ID, third); err != nil {
		t.Fatalf("rejected key was consumed: %v", err)
	}
	waitStatus(t, e, g, s.ID, "idle")
	if got := <-f.started; got != "third" {
		t.Fatalf("interrupted queue was replayed: %s", got)
	}
}

func TestScopeAndPassiveResume(t *testing.T) {
	e, f, g := setup(t)
	s := create(t, e, g)
	ctx := context.Background()
	if _, err := e.Get(ctx, core.Grant{Tenant: "other"}, s.ID); core.ErrorCode(err) != core.NotFound {
		t.Fatal(err)
	}
	other := create(t, e, g)
	scoped := core.Grant{Tenant: g.Tenant, Session: s.ID}
	if _, err := e.Get(ctx, scoped, other.ID); core.ErrorCode(err) != core.NotFound {
		t.Fatal(err)
	}
	list, err := e.List(ctx, scoped, "", 10)
	if err != nil || len(list) != 1 || list[0].ID != s.ID {
		t.Fatalf("scoped list: %+v %v", list, err)
	}
	if _, err := e.Resume(ctx, g, s.ID); err != nil {
		t.Fatal(err)
	}
	if f.opens.Load() != 0 {
		t.Fatal("inspection or resume activated harness")
	}
}

func TestShutdownAndAdmissionDoNotDeadlock(t *testing.T) {
	e, _, g := setup(t)
	s := create(t, e, g)
	ctx := context.Background()
	done := make(chan struct{})
	go func() {
		defer close(done)
		for range 30 {
			_, _ = e.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "hello"})
		}
	}()
	drain, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := e.Shutdown(drain); err != nil {
		t.Fatal(err)
	}
	select {
	case <-done:
	case <-drain.Done():
		t.Fatal("admission deadlocked against shutdown")
	}
}
