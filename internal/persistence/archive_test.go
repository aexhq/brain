package persistence

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/store"
)

type recordedObjects struct {
	Objects
	bytes int
	fail  string
}

func (s *recordedObjects) Put(ctx context.Context, key string, b []byte, immutable bool) error {
	if strings.Contains(key, s.fail) && s.fail != "" {
		return errors.New("upload unavailable")
	}
	s.bytes += len(b)
	return s.Objects.Put(ctx, key, b, immutable)
}

func fixture(t *testing.T) (*Archive, *recordedObjects, Snapshot, *store.Log) {
	t.Helper()
	dir := t.TempDir()
	if err := os.MkdirAll(filepath.Join(dir, "native", "session"), 0700); err != nil {
		t.Fatal(err)
	}
	l, err := store.OpenLog(filepath.Join(dir, "events.jsonl"), 16<<20)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = l.Close() })
	now := time.Now().UTC()
	s := core.Session{ID: core.ID(), Tenant: "tenant-a", Harness: "pi", Config: core.Config{Model: "test/model"}, Status: "idle", CreatedAt: now, UpdatedAt: now, Checkpoint: &core.Checkpoint{ID: core.ID(), Time: now, Config: core.Config{Model: "test/model"}}}
	r := &recordedObjects{Objects: FileObjects{Root: t.TempDir()}}
	return &Archive{Objects: r, MaxSessionBytes: 16 << 20}, r, Snapshot{Session: s, Directory: dir}, l
}

func appendEvent(t *testing.T, l *store.Log, text string) {
	t.Helper()
	ev, err := core.NewEvent("assistant.message", "", core.Text{Text: text})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := l.Append(ev); err != nil {
		t.Fatal(err)
	}
}

func TestAppendBlocksRestoreAndPublicationFailure(t *testing.T) {
	a, objects, s, l := fixture(t)
	archiveContract(t, a, objects, s, l)
}

func archiveContract(t *testing.T, a *Archive, objects *recordedObjects, s Snapshot, l *store.Log) {
	t.Helper()
	ctx := context.Background()
	native := bytes.Repeat([]byte("n"), 3*int(BlockBytes)+19)
	if err := os.WriteFile(filepath.Join(s.Directory, "native/session/data.jsonl"), native, 0600); err != nil {
		t.Fatal(err)
	}
	s.Session.Checkpoint.Files = []core.NativeFile{{Path: "session/data.jsonl", Size: int64(len(native)), AppendOnly: true}}
	appendEvent(t, l, strings.Repeat("a", 3*int(BlockBytes)))
	s.HistoryBytes = l.Size()
	s.Session.Checkpoint.Offset = s.HistoryBytes
	first, err := a.Publish(ctx, s)
	if err != nil {
		t.Fatal(err)
	}
	firstBytes := objects.bytes
	appendEvent(t, l, "later history, without a newer native boundary")
	s.HistoryBytes = l.Size()
	second, err := a.Publish(ctx, s)
	if err != nil {
		t.Fatal(err)
	}
	if overhead := objects.bytes - firstBytes; overhead > 2*int(BlockBytes)+10000 {
		t.Fatalf("rewrote growing history: %d bytes", overhead)
	}
	if second.Files[0].Generation != first.Files[0].Generation {
		t.Fatal("append replaced immutable blocks")
	}
	objects.fail = "/checkpoints/"
	appendEvent(t, l, "unpublished tail")
	s.HistoryBytes = l.Size()
	if _, err := a.Publish(ctx, s); err == nil {
		t.Fatal("publication succeeded during outage")
	}
	objects.fail = ""
	m, restored, err := a.Restore(ctx, "tenant-a", s.Session.ID, "", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if m.ID != second.ID || m.HistoryBytes != second.HistoryBytes || m.Session.Checkpoint.Offset != first.HistoryBytes {
		t.Fatal("published/native boundary moved after failed upload")
	}
	got, err := os.ReadFile(filepath.Join(restored, "native/session/data.jsonl"))
	if err != nil || !bytes.Equal(got, native) {
		t.Fatalf("native restore: %v", err)
	}
	if _, _, err := a.Restore(ctx, "tenant-b", s.Session.ID, first.ID, t.TempDir()); core.ErrorCode(err) != core.NotFound {
		t.Fatalf("cross-tenant restore: %v", err)
	}
	if _, _, err := a.Restore(ctx, "tenant-a", s.Session.ID, first.ID, t.TempDir()); err != nil {
		t.Fatalf("older immutable checkpoint unavailable: %v", err)
	}
}

func TestRestoreRejectsMissingBlockAndUnsafeManifest(t *testing.T) {
	a, objects, s, l := fixture(t)
	appendEvent(t, l, strings.Repeat("a", int(BlockBytes)))
	s.HistoryBytes = l.Size()
	m, err := a.Publish(context.Background(), s)
	if err != nil {
		t.Fatal(err)
	}
	root := objects.Objects.(FileObjects).Root
	block := filepath.Join(root, filepath.FromSlash(blockKey(s.Session.ID, m.Files[0].Generation, 0)))
	if err := os.Remove(block); err != nil {
		t.Fatal(err)
	}
	parent := t.TempDir()
	if _, _, err := a.Restore(context.Background(), "tenant-a", s.Session.ID, m.ID, parent); err == nil {
		t.Fatal("restored missing block")
	}
	entries, err := os.ReadDir(parent)
	if err != nil || len(entries) != 0 {
		t.Fatal("failed restore leaked staging files")
	}
	m.Files[0].Path = "../escaped"
	b, _ := json.Marshal(m)
	if err := objects.Objects.Put(context.Background(), manifestKey(s.Session.ID, m.ID), b, false); err != nil {
		t.Fatal(err)
	}
	if _, _, err := a.Restore(context.Background(), "tenant-a", s.Session.ID, m.ID, parent); err == nil {
		t.Fatal("restored unsafe path")
	}
}

func TestFileObjectsBoundsAndImmutability(t *testing.T) {
	s := FileObjects{Root: t.TempDir()}
	ctx := context.Background()
	for _, key := range []string{"../outside", "/absolute", "a/../b", `a\b`} {
		if err := s.Put(ctx, key, nil, false); err == nil {
			t.Fatalf("accepted %q", key)
		}
	}
	if err := s.Put(ctx, "key", []byte("first"), true); err != nil {
		t.Fatal(err)
	}
	if err := s.Put(ctx, "key", []byte("second"), true); err != nil {
		t.Fatal(err)
	}
	b, err := s.Get(ctx, "key")
	if err != nil || string(b) != "first" {
		t.Fatal("immutable object changed")
	}
	if err := s.Put(ctx, "key", []byte("latest"), false); err != nil {
		t.Fatal(err)
	}
	b, err = s.Get(ctx, "key")
	if err != nil || string(b) != "latest" {
		t.Fatal("mutable pointer did not change")
	}
	if err := s.Put(ctx, "large", make([]byte, MaxObjectBytes+1), true); err == nil {
		t.Fatal("accepted oversized object")
	}
}
