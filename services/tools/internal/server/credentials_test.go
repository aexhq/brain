package server

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/aexhq/brain/internal/credentialcipher"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
	"github.com/aexhq/brain/services/tools/api"
	tools "github.com/aexhq/brain/services/tools/registry"
)

func TestCredentialRPCScopesModelRecordsOutAndFailsOnLostToolKeys(t *testing.T) {
	t.Setenv("TOOLS_TEST_KEY", base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{3}, 32)))
	keys := &credentialcipher.KeyConfig{ActiveID: "tools", Environments: map[string]string{"tools": "TOOLS_TEST_KEY"}}
	registry, err := tools.New(nil)
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	registry.Keys = keys
	srv := &Server{Registry: registry, Token: "test-service-token"}
	requests := 0
	endpoint := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(body, []byte("$model")) || bytes.Contains(body, []byte("model-ciphertext-marker")) {
			t.Error("model credential crossed Tools boundary")
		}
		r.Body = io.NopCloser(bytes.NewReader(body))
		requests++
		srv.rpc(w, r)
	}))
	defer endpoint.Close()
	client := &api.Client{URL: endpoint.URL, Token: srv.Token}
	ctx := context.Background()
	s := core.Session{ID: "session", Tenant: "one", Config: core.Config{Bindings: []core.ToolBinding{{Alias: "data", Provider: "data", Operations: []string{"read"}}}}, CredentialBindings: []string{"$model"}, Credentials: []credential.Record{{Binding: "$model", Ciphertext: []byte("model-ciphertext-marker")}}}
	records, err := client.SealCredentials(ctx, s, []core.CredentialInput{{Binding: "data", Value: json.RawMessage(`{"token":"private-tool-value"}`)}})
	if err != nil || len(records) != 1 {
		t.Fatalf("admission: %v", err)
	}
	s.Credentials = append(s.Credentials, records...)
	s.CredentialBindings = append(s.CredentialBindings, "data")
	if err := client.ValidateCredentials(ctx, s); err != nil {
		t.Fatal(err)
	}
	rotated, err := client.RotateCredentials(ctx, s)
	if err != nil || len(rotated) != 1 || rotated[0].Binding != "data" {
		t.Fatalf("rotation: %v", err)
	}
	missing := s
	missing.Credentials = s.Credentials[:1]
	if err := client.ValidateCredentials(ctx, missing); err == nil {
		t.Fatal("missing tool record accepted")
	}
	registry.Keys = nil
	if err := client.ValidateCredentials(ctx, s); err == nil {
		t.Fatal("lost Tools key accepted")
	}
	if requests != 5 {
		t.Fatalf("expected credential operations through Tools RPC, got %d", requests)
	}
}
