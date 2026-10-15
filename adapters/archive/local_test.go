package archive

import (
	"context"
	"crypto/rand"
	"os"
	"path/filepath"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestLocalImmutablePublicationAndRestart(t *testing.T) {
	directory := filepath.Join("..", "..", ".test-output", "archive-"+rand.Text())
	t.Cleanup(func() { _ = os.RemoveAll(directory) })
	store, err := OpenLocal(directory)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	if err = store.Put(ctx, "session/manifest", []byte("original")); err != nil {
		t.Fatal(err)
	}
	if err = store.Put(ctx, "session/manifest", []byte("original")); err != nil {
		t.Fatal(err)
	}
	if err = store.Put(ctx, "session/manifest", []byte("changed!")); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("immutable conflict: %v", err)
	}
	if _, err = store.Get(ctx, "session/manifest", 7); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("read bound: %v", err)
	}
	for _, key := range []string{"../escape", "a/../../b", `a\..\b`, "C:private", "."} {
		if err = store.Put(ctx, key, []byte("x")); err == nil {
			t.Errorf("unsafe key admitted: %s", key)
		}
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	store, err = OpenLocal(directory)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	body, err := store.Get(ctx, "session/manifest", 8)
	if err != nil || string(body) != "original" {
		t.Fatalf("retained archive %q: %v", body, err)
	}
	if _, err = store.db.ExecContext(ctx, "UPDATE objects SET body=? WHERE key=?", []byte("corrupt!"), "session/manifest"); err != nil {
		t.Fatal(err)
	}
	if _, err = store.Get(ctx, "session/manifest", 8); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("corrupt archive: %v", err)
	}
	if err = store.Delete(ctx, "session/manifest"); err != nil {
		t.Fatal(err)
	}
	if _, err = store.Get(ctx, "session/manifest", 8); kernel.Response(err).Code != wire.ErrorCodeUnavailable {
		t.Fatalf("deleted archive: %v", err)
	}
}
