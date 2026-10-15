package store

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
)

func appendText(t *testing.T, l *Log, text string) core.Event {
	t.Helper()
	e, err := core.NewEvent("user.message", "", core.Text{Text: text})
	if err != nil {
		t.Fatal(err)
	}
	e, err = l.Append(e)
	if err != nil {
		t.Fatal(err)
	}
	return e
}

func TestTailRecoveryAndLostCursor(t *testing.T) {
	path := filepath.Join(t.TempDir(), "events.jsonl")
	l, err := OpenLog(path, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	first := appendText(t, l, "first")
	prefix := l.Size()
	lost := appendText(t, l, "lost")
	if err := l.Close(); err != nil {
		t.Fatal(err)
	}
	if err := os.Truncate(path, prefix); err != nil {
		t.Fatal(err)
	}
	f, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := f.WriteString(`{"partial":`); err != nil {
		t.Fatal(err)
	}
	f.Close()
	l, err = OpenLog(path, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	appendText(t, l, "replacement")
	if _, err := l.Read(lost.Cursor, 10, -1); core.ErrorCode(err) != core.CursorUnavailable {
		t.Fatalf("lost cursor: %v", err)
	}
	got, err := l.Read(first.Cursor, 10, -1)
	if err != nil || len(got) != 1 {
		t.Fatalf("history: %v %v", got, err)
	}
}

func TestInteriorCorruptionFails(t *testing.T) {
	path := filepath.Join(t.TempDir(), "events.jsonl")
	if err := os.WriteFile(path, []byte("{broken}\n{}\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if l, err := OpenLog(path, 1<<20); err == nil {
		l.Close()
		t.Fatal("accepted corrupt complete line")
	}
}

func TestReplayLiveBoundaryAndSlowSubscriber(t *testing.T) {
	l, err := OpenLog(filepath.Join(t.TempDir(), "events.jsonl"), 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	first := appendText(t, l, "first")
	sub, boundary := l.Subscribe(1, 1024)
	defer sub.Close()
	second := appendText(t, l, "second")
	replay, err := l.Read("", 10, boundary)
	if err != nil || len(replay) != 1 || replay[0].ID != first.ID {
		t.Fatalf("replay: %v %v", replay, err)
	}
	if live := <-sub.Events; live.ID != second.ID {
		t.Fatal("wrong live event")
	} else {
		sub.Consumed(live)
	}
	appendText(t, l, "third")
	appendText(t, l, "fourth")
	select {
	case <-sub.Done:
		if core.ErrorCode(sub.Err()) != core.Overloaded {
			t.Fatal(sub.Err())
		}
	default:
		t.Fatal("slow subscriber was not disconnected")
	}
}

func TestMetadataTenantAndRestart(t *testing.T) {
	root := t.TempDir()
	s, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	now := time.Now().UTC()
	a := core.Session{ID: core.ID(), Tenant: "a", Harness: "pi", Config: core.Config{Model: "openai/test", Tools: []string{}}, Status: "idle", CreatedAt: now, UpdatedAt: now}
	if err := s.Create(ctx, a); err != nil {
		t.Fatal(err)
	}
	s.Close()
	s, err = Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	got, err := s.Get(ctx, a.ID)
	if err != nil || got.Tenant != "a" || got.Config.Model != a.Config.Model {
		t.Fatalf("get: %+v %v", got, err)
	}
	other, err := s.List(ctx, "b", "", 10)
	if err != nil || len(other) != 0 {
		t.Fatalf("scope: %+v %v", other, err)
	}
	if _, err := s.Dir("../escape"); core.ErrorCode(err) != core.Invalid {
		t.Fatal("invalid id accepted")
	}
}
