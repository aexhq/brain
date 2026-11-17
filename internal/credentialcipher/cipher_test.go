package credentialcipher

import (
	"bytes"
	"context"
	"encoding/base64"
	"testing"
)

func TestModelCiphertextCannotBecomeToolCredential(t *testing.T) {
	t.Setenv("CIPHER_KEY", base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{1}, 32)))
	keys := &KeyConfig{ActiveID: "test", Environments: map[string]string{"test": "CIPHER_KEY"}}
	scope := Scope{Tenant: "one", Session: "session", Binding: "$model"}
	record, err := Seal(context.Background(), keys, scope, []byte("secret"))
	if err != nil {
		t.Fatal(err)
	}
	// A mistaken deployment using the same key still cannot reuse a model
	// envelope as a tool credential by changing its binding.
	scope.Binding = "data"
	record.Binding = "data"
	if _, err := Open(context.Background(), keys, scope, record); err == nil {
		t.Fatal("model-to-tool credential substitution accepted")
	}
}
