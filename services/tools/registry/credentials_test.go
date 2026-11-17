package tools

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
)

func TestProviderCredentialAdmissionRejectsInvalidBindings(t *testing.T) {
	r, err := New(nil)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	s := core.Session{ID: "session", Tenant: "one", Config: core.Config{Bindings: []core.ToolBinding{{Alias: "data", Provider: "data"}, {Alias: "$model", Provider: "data"}}}}
	valid := core.CredentialInput{Binding: "data", Value: json.RawMessage(`{"token":"secret"}`)}
	for _, inputs := range [][]core.CredentialInput{
		{{Binding: "$model", Value: valid.Value}},
		{{Binding: "unknown", Value: valid.Value}},
		{valid, valid},
		{{Binding: "data", Value: json.RawMessage(`{`)}},
		{{Binding: "data", Value: json.RawMessage(`"` + strings.Repeat("a", 64<<10) + `"`)}},
	} {
		if _, err := r.PrepareCredentials(context.Background(), s, inputs); core.ErrorCode(err) != core.Invalid {
			t.Fatalf("invalid credential accepted: %v", err)
		}
	}
	// An invalid user alias must not cause the model envelope to be forwarded.
	s.CredentialBindings = []string{"$model"}
	s.Credentials = []credential.Record{{Binding: "$model", Value: json.RawMessage(`"model-secret"`)}}
	projected := CredentialSession(s)
	if len(projected.Credentials) != 0 || len(projected.CredentialBindings) != 0 {
		t.Fatal("invalid provider alias selected a model credential")
	}
	if len(s.Credentials) != 1 {
		t.Fatal("projection mutated the persisted session")
	}
}
