package storage

import (
	"context"
	"crypto/rand"
	"os"
	"path/filepath"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestResourceIdentityPersistsButCannotBeRecreatedOverExistingData(t *testing.T) {
	directory := filepath.Join("..", "..", ".test-output", "storage-identity-"+rand.Text())
	handler, err := OpenLocal(directory, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(directory) })
	first := handler.Identity()
	if err = handler.Close(); err != nil {
		t.Fatal(err)
	}
	handler, err = OpenLocal(directory, nil, nil)
	if err != nil || handler.Identity() != first {
		t.Fatalf("retained identity changed: %v", err)
	}
	if err = handler.Close(); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(directory, "retained-data"), []byte("preserve"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.Remove(filepath.Join(directory, "storage.identity")); err != nil {
		t.Fatal(err)
	}
	if _, err = OpenLocal(directory, nil, nil); kernel.Response(err).Code != wire.ErrorCodeUnsupported {
		t.Fatalf("unidentified existing data was adopted without verification: %v", err)
	}
	body, err := os.ReadFile(filepath.Join(directory, "retained-data"))
	if err != nil || string(body) != "preserve" {
		t.Fatal("existing data was changed")
	}
}

func TestRangeOverflowFailsBeforeStorageAccess(t *testing.T) {
	directory := filepath.Join("..", "..", ".test-output", "storage-range-"+rand.Text())
	handler, err := OpenLocal(directory, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = handler.Close(); _ = os.RemoveAll(directory) })
	_, err = handler.Invoke(context.Background(), "tenant", wire.ConnectionTemplate{Name: "files", Capabilities: []string{"read"}},
		wire.Execution{Operation: wire.Operation{SessionID: "session"}, MaxOutputBytes: 8192,
			Arguments: kernel.JSON(map[string]any{"op": "read", "path": "file", "offset": int64(1<<63 - 1), "bytes": 2})})
	if kernel.Response(err).Code != wire.ErrorCodeInvalid {
		t.Fatalf("overflowing range was accepted: %v", err)
	}
}
