//go:build !windows

package filesystem

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/aexhq/brain/services/storage/internal/adapters"
)

func TestRootRejectsSymlinkEscape(t *testing.T) {
	directory, outside := t.TempDir(), t.TempDir()
	s, err := New(directory)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	if err = os.Symlink(outside, filepath.Join(directory, "escape")); err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	if _, err = s.Put(ctx, "escape/object", []byte("must stay inside root"), adapters.Condition{}); err == nil {
		t.Fatal("write escaped root")
	}
	if err = os.WriteFile(filepath.Join(outside, "private.object"), []byte("private"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Get(ctx, "escape/private"); err == nil {
		t.Fatal("read escaped root")
	}
	if err = s.Delete(ctx, "escape/private"); err == nil {
		t.Fatal("delete escaped root")
	}
	if _, err = os.Stat(filepath.Join(outside, "private.object")); err != nil {
		t.Fatal(err)
	}
}
