package tiering

import (
	"context"
	"errors"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/aexhq/brain/services/storage/internal/adapters"
	"github.com/aexhq/brain/services/storage/internal/adapters/memory"
	"github.com/aexhq/brain/services/storage/internal/adapters/sqlite"
)

type failing struct {
	adapters.Adapter
	err error
}

func (f failing) Get(context.Context, string) (adapters.Object, error) {
	return adapters.Object{}, f.err
}
func cache(t *testing.T, disk, remote adapters.Adapter, budget int64) *Store {
	t.Helper()
	s, e := New(context.Background(), Options{Disk: disk, Remote: remote, Index: filepath.Join(t.TempDir(), "index.sqlite"), MemoryBytes: 4, DiskBytes: budget, MemoryTTL: time.Hour, DiskTTL: time.Hour, Interval: time.Hour})
	if e != nil {
		t.Fatal(e)
	}
	t.Cleanup(func() {
		if e := s.Close(); e != nil {
			t.Error(e)
		}
	})
	return s
}
func TestReadTiersAndDirtyEviction(t *testing.T) {
	ctx := context.Background()
	disk := memory.New()
	remote := memory.New()
	s := cache(t, disk, remote, 20)
	if _, e := s.Put(ctx, "t/a", []byte("123456789012345678"), adapters.Condition{}); e != nil {
		t.Fatal(e)
	}
	if e := s.Maintain(ctx); e != nil {
		t.Fatal(e)
	}
	if _, e := disk.Get(ctx, "t/a"); e != nil {
		t.Fatal("dirty object was evicted", e)
	}
	if e := s.Commit(ctx, "t/a"); e != nil {
		t.Fatal(e)
	}
	if e := s.Maintain(ctx); e != nil {
		t.Fatal(e)
	}
	if _, e := disk.Get(ctx, "t/a"); !errors.Is(e, adapters.ErrNotFound) {
		t.Fatal("clean object did not demote", e)
	}
	o, e := s.Get(ctx, "t/a")
	if e != nil || string(o.Data) != "123456789012345678" {
		t.Fatal(o, e)
	}
	if _, e := disk.Get(ctx, "t/a"); e != nil {
		t.Fatal("external read was not promoted", e)
	}
}
func TestCorruptionDoesNotFallThrough(t *testing.T) {
	ctx := context.Background()
	remote := memory.New()
	_, _ = remote.Put(ctx, "t/a", []byte("remote"), adapters.Condition{})
	failure := errors.New("disk corruption")
	s := cache(t, failing{Adapter: memory.New(), err: failure}, remote, 20)
	if _, e := s.Get(ctx, "t/a"); !errors.Is(e, failure) {
		t.Fatal(e)
	}
}
func TestConcurrentWritesRespectDiskBudget(t *testing.T) {
	s := cache(t, memory.New(), memory.New(), 10)
	var accepted atomic.Int32
	var wg sync.WaitGroup
	for _, key := range []string{"t/a", "t/b", "t/c", "t/d"} {
		wg.Go(func() {
			if _, e := s.Put(context.Background(), key, []byte("123456"), adapters.Condition{}); e == nil {
				accepted.Add(1)
			}
		})
	}
	wg.Wait()
	if accepted.Load() != 1 {
		t.Fatalf("accepted %d writes beyond capacity", accepted.Load())
	}
}
func TestRestartRetainsUnsynchronizedObjects(t *testing.T) {
	root := t.TempDir()
	file := filepath.Join(root, "disk.sqlite")
	index := filepath.Join(root, "index.sqlite")
	remote := memory.New()
	ctx := context.Background()
	open := func() *Store {
		d, e := sqlite.New(file)
		if e != nil {
			t.Fatal(e)
		}
		s, e := New(ctx, Options{Disk: d, Remote: remote, Index: index, MemoryBytes: 4, DiskBytes: 100, MemoryTTL: time.Hour, DiskTTL: time.Hour, Interval: time.Hour})
		if e != nil {
			t.Fatal(e)
		}
		return s
	}
	s := open()
	if _, e := s.Put(ctx, "t/a", []byte("survives"), adapters.Condition{}); e != nil {
		t.Fatal(e)
	}
	if e := s.Close(); e != nil {
		t.Fatal(e)
	}
	s = open()
	defer s.Close()
	n, e := s.Pending(ctx)
	if e != nil || n != 1 {
		t.Fatal(n, e)
	}
	if e = s.Commit(ctx, "t/a"); e != nil {
		t.Fatal(e)
	}
	o, e := remote.Get(ctx, "t/a")
	if e != nil || string(o.Data) != "survives" {
		t.Fatal(o, e)
	}
}
