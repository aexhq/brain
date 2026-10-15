package storage

import (
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestRootedStorageAndNamespaceIsolation(t *testing.T) {
	directory := filepath.Join("..", "..", ".test-output", "agent-storage-"+rand.Text())
	handler, err := OpenLocal(directory, nil, nil)
	if err != nil {
		t.Fatal(err)
	}

	t.Cleanup(func() { _ = handler.Close(); _ = os.RemoveAll(directory) })
	template := wire.ConnectionTemplate{Name: "files", Capabilities: []string{"read", "write", "list", "stat", "delete"}}
	invoke := func(session string, args map[string]any) wire.Receipt {
		t.Helper()
		receipt, err := handler.Invoke(context.Background(), "tenant", template, wire.Execution{Operation: wire.Operation{SessionID: session}, Arguments: kernel.JSON(args), MaxOutputBytes: 8192})
		if err != nil {
			t.Fatal(err)
		}
		return receipt
	}
	if receipt := invoke("one", map[string]any{"op": "write", "path": "folder/value.txt", "text": "scoped"}); receipt.Outcome != wire.OutcomeCompleted {
		t.Fatal(receipt)
	}
	receipt := invoke("one", map[string]any{"op": "read", "path": "folder/value.txt"})
	var body struct {
		Base64 string `json:"base64"`
	}
	if json.Unmarshal(receipt.Result, &body) != nil {
		t.Fatal(receipt)
	}
	decoded, err := base64.StdEncoding.DecodeString(body.Base64)
	if err != nil || string(decoded) != "scoped" {
		t.Fatalf("read %s %v", decoded, err)
	}
	if receipt := invoke("two", map[string]any{"op": "read", "path": "folder/value.txt"}); receipt.Outcome == wire.OutcomeCompleted {
		t.Fatal("another session accessed storage")
	}
	for _, name := range []string{"../control.sqlite", `folder\..\control.sqlite`, "/absolute", "C:private", "."} {
		_, err := handler.Invoke(context.Background(), "tenant", template, wire.Execution{Operation: wire.Operation{SessionID: "one"}, Arguments: kernel.JSON(map[string]string{"op": "read", "path": name}), MaxOutputBytes: 8192})
		if kernel.Response(err).Code != wire.ErrorCodeForbidden {
			t.Fatalf("unsafe path %s: %v", name, err)
		}
	}
	template.Capabilities = []string{"read"}
	if _, err := handler.Invoke(context.Background(), "tenant", template, wire.Execution{Operation: wire.Operation{SessionID: "one"}, Arguments: kernel.JSON(map[string]string{"op": "delete", "path": "folder/value.txt"}), MaxOutputBytes: 8192}); kernel.Response(err).Code != wire.ErrorCodeForbidden {
		t.Fatal("read-only grant permitted deletion")
	}
}
