package credentialcipher

import (
	"bytes"
	"context"
	"encoding/base64"
	"testing"
)

func TestCiphertextCannotCrossCredentialDomains(t *testing.T) {
	t.Setenv("CIPHER_KEY", base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{1}, 32)))
	keys := &KeyConfig{ActiveID: "test", Environments: map[string]string{"test": "CIPHER_KEY"}}
	scope := Scope{Domain: "model", Tenant: "one", Session: "session", Binding: "binding"}
	record, err := Seal(context.Background(), keys, scope, []byte("secret"))
	if err != nil {
		t.Fatal(err)
	}
	// A mistaken deployment using the same key still cannot reuse a model
	// envelope as a tool credential, even with identical identity fields.
	scope.Domain = "tool"
	if _, err := Open(context.Background(), keys, scope, record); err == nil {
		t.Fatal("cross-domain credential substitution accepted")
	}
}
