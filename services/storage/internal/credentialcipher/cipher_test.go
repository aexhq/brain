package credentialcipher

import (
	"bytes"
	"context"
	"encoding/base64"
	"github.com/aexhq/brain/services/storage"
	"testing"
)

func TestCiphertextCannotMoveBetweenTenantsSessionsOrBindings(t *testing.T) {
	t.Setenv("CIPHER_KEY", base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{1}, 32)))
	keys := &storage.KeyConfig{ActiveID: "test", Environments: map[string]string{"test": "CIPHER_KEY"}}
	scope := Scope{Tenant: "one", Session: "session", Binding: "$model"}
	record, err := Seal(context.Background(), keys, scope, []byte("secret"))
	if err != nil {
		t.Fatal(err)
	}
	for _, other := range []Scope{{Tenant: "two", Session: scope.Session, Binding: scope.Binding}, {Tenant: scope.Tenant, Session: "other", Binding: scope.Binding}, {Tenant: scope.Tenant, Session: scope.Session, Binding: "data"}} {
		if _, err := Open(context.Background(), keys, other, record); err == nil {
			t.Fatalf("scope substitution accepted: %+v", other)
		}
	}
}
