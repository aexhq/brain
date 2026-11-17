package brain

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"testing"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/brain/internal/modelcredential"
	tools "github.com/aexhq/brain/services/tools/registry"
)

func TestModelAndToolCredentialOwnersUseIndependentKeys(t *testing.T) {
	o, _ := modelOptions(t)
	t.Setenv("TOOL_KEY_FIRST", base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{1}, 32)))
	t.Setenv("TOOL_KEY_SECOND", base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{2}, 32)))
	toolKeys := &KeyConfig{ActiveID: "first", Environments: map[string]string{"first": "TOOL_KEY_FIRST", "second": "TOOL_KEY_SECOND"}}
	o.ToolKeys = toolKeys
	secret := json.RawMessage(`{"token":"tool-only-secret"}`)
	o.Providers = map[string]ToolProvider{"data": {
		Parameters: json.RawMessage(`{"type":"object"}`),
		Authorize: func(_ context.Context, b BindingContext) error {
			if !bytes.Equal(b.Credential, secret) {
				t.Error("tool provider received another owner's credential")
			}
			return nil
		},
		Bind: func(context.Context, BindingContext) ([]ManagedTool, error) {
			return []ManagedTool{{Tool: core.Tool{Name: "read", Description: "Read", InputSchema: json.RawMessage(`{"type":"object"}`)}, Handler: func(context.Context, ToolRequest, func(json.RawMessage)) Result { return Result{} }}}, nil
		},
	}}
	s := openModelServer(t, o)
	ctx := context.Background()
	v, err := s.engine.Create(ctx, Grant{Tenant: "one", Manage: true}, CreateSession{
		ID: core.ID(), Harness: "test", Config: SessionConfig{Model: "test/model", Bindings: []core.ToolBinding{{Alias: "data", Provider: "data", Operations: []string{"read"}}}},
		ModelCredential: APIKey("model-only-secret"), Credentials: []CredentialInput{{Binding: "data", Value: secret}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := s.engine.ValidateCredentials(ctx, v); err != nil {
		t.Fatal(err)
	}
	if value, err := modelcredential.Open(ctx, toolKeys, v); err == nil || value != nil {
		t.Fatal("Tools keys decrypted model credentials")
	}
	registry, err := tools.New(nil)
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	registry.Keys = o.Keys
	if err := registry.ValidateCredentials(ctx, v); err == nil {
		t.Fatal("model keys decrypted tool credentials")
	}
	toolKeys.ActiveID = "second"
	if err := s.RotateCredentials(ctx, "one", v.ID); err != nil {
		t.Fatal(err)
	}
	v, err = s.engine.Get(ctx, Grant{Tenant: "one", Manage: true}, v.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, record := range v.Credentials {
		if record.Binding == "data" && record.Key != "second" {
			t.Fatal("tool rotation used the model key provider")
		}
		if record.Binding == modelcredential.Binding && record.Key != "test" {
			t.Fatal("model rotation used the Tools key provider")
		}
	}
	// Both revocation tombstones must survive subsequent rotation.
	if err := s.RevokeCredential(ctx, "one", v.ID, "data"); err != nil {
		t.Fatal(err)
	}
	if err := s.RevokeModelCredential(ctx, "one", v.ID); err != nil {
		t.Fatal(err)
	}
	if err := s.RotateCredentials(ctx, "one", v.ID); err != nil {
		t.Fatal(err)
	}
	v, err = s.engine.Get(ctx, Grant{Tenant: "one", Manage: true}, v.ID)
	if err != nil {
		t.Fatal(err)
	}
	if len(v.Credentials) != 2 {
		t.Fatal("rotation lost revocation tombstones")
	}
	for _, record := range v.Credentials {
		if !record.Revoked {
			t.Fatal("rotation undid revocation")
		}
	}
}
