package adapters_test

import (
	"context"
	"errors"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/aexhq/brain/services/storage/internal/adapters"
	"github.com/aexhq/brain/services/storage/internal/adapters/filesystem"
	"github.com/aexhq/brain/services/storage/internal/adapters/memory"
	"github.com/aexhq/brain/services/storage/internal/adapters/sqlite"
)

func TestAdapterContract(t *testing.T) {
	factories := map[string]func(*testing.T) adapters.Adapter{
		"memory": func(t *testing.T) adapters.Adapter { return memory.New() },
		"filesystem": func(t *testing.T) adapters.Adapter {
			s, e := filesystem.New(t.TempDir())
			if e != nil {
				t.Fatal(e)
			}
			return s
		},
		"sqlite": func(t *testing.T) adapters.Adapter {
			s, e := sqlite.New(filepath.Join(t.TempDir(), "objects.sqlite"))
			if e != nil {
				t.Fatal(e)
			}
			return s
		},
	}
	for name, factory := range factories {
		t.Run(name, func(t *testing.T) {
			s := factory(t)
			contract(t, s)
		})
	}
}

func contract(t *testing.T, s adapters.Adapter) {
	t.Cleanup(func() {
		for _, k := range []string{"a", "b", "c"} {
			if e := s.Delete(context.Background(), "tenant/"+k); e != nil {
				t.Error(e)
			}
		}
		if e := s.Close(); e != nil {
			t.Error(e)
		}
	})
	ctx := context.Background()
	if _, e := s.Get(ctx, "tenant/missing"); !errors.Is(e, adapters.ErrNotFound) {
		t.Fatal(e)
	}
	first, e := s.Put(ctx, "tenant/a", []byte("one"), adapters.Condition{CreateOnly: true})
	if e != nil {
		t.Fatal(e)
	}
	if _, e = s.Put(ctx, "tenant/a", nil, adapters.Condition{CreateOnly: true}); !errors.Is(e, adapters.ErrConflict) {
		t.Fatal(e)
	}
	if _, e = s.Put(ctx, "tenant/a", nil, adapters.Condition{Version: "stale"}); !errors.Is(e, adapters.ErrConflict) {
		t.Fatal(e)
	}
	updated, e := s.Put(ctx, "tenant/a", nil, adapters.Condition{Version: first.Version})
	if e != nil || updated.Version == first.Version {
		t.Fatalf("conditional update: %+v %v", updated, e)
	}
	if _, e = s.Put(ctx, "tenant/b", []byte("two"), adapters.Condition{}); e != nil {
		t.Fatal(e)
	}
	if _, e = s.Put(ctx, "tenant/c", []byte("three"), adapters.Condition{}); e != nil {
		t.Fatal(e)
	}
	p, e := s.List(ctx, "tenant/", "", 2)
	if e != nil || len(p.Objects) != 2 || p.Objects[0].Key != "tenant/a" || p.Next != "tenant/b" {
		t.Fatalf("first page: %+v %v", p, e)
	}
	p, e = s.List(ctx, "tenant/", p.Next, 2)
	if e != nil || len(p.Objects) != 1 || p.Objects[0].Key != "tenant/c" || p.Next != "" {
		t.Fatalf("last page: %+v %v", p, e)
	}
	var winners atomic.Int32
	var wg sync.WaitGroup
	for range 12 {
		wg.Go(func() {
			if _, e := s.Put(ctx, "tenant/a", []byte("winner"), adapters.Condition{Version: updated.Version}); e == nil {
				winners.Add(1)
			} else if !errors.Is(e, adapters.ErrConflict) {
				t.Error(e)
			}
		})
	}
	wg.Wait()
	if winners.Load() != 1 {
		t.Fatalf("%d compare-and-swap winners", winners.Load())
	}
	if e = s.Delete(ctx, "tenant/b"); e != nil {
		t.Fatal(e)
	}
	if e = s.Delete(ctx, "tenant/b"); e != nil {
		t.Fatal(e)
	}
	if _, e = s.Get(ctx, "tenant/b"); !errors.Is(e, adapters.ErrNotFound) {
		t.Fatal(e)
	}
	if _, e = s.Put(ctx, "../escape", nil, adapters.Condition{}); !errors.Is(e, adapters.ErrInvalid) {
		t.Fatal(e)
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, e = s.Get(cancelled, "tenant/a"); !errors.Is(e, context.Canceled) {
		t.Fatal(e)
	}
}
