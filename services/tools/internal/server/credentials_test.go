package server

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
	"github.com/aexhq/brain/services/tools/api"
	tools "github.com/aexhq/brain/services/tools/registry"
)

func TestCredentialRPCScopesModelRecordsAndRejectsMissingValues(t *testing.T) {
	registry, err := tools.New(nil)
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	srv := &Server{Registry: registry, Token: "test-service-token"}
	requests := 0
	endpoint := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(body, []byte("$model")) || bytes.Contains(body, []byte("model-secret-marker")) {
			t.Error("model credential crossed Tools boundary")
		}
		r.Body = io.NopCloser(bytes.NewReader(body))
		requests++
		srv.rpc(w, r)
	}))
	defer endpoint.Close()
	client := &api.Client{URL: endpoint.URL, Token: srv.Token}
	ctx := context.Background()
	s := core.Session{ID: "session", Tenant: "one", Config: core.Config{Bindings: []core.ToolBinding{{Alias: "data", Provider: "data", Operations: []string{"read"}}}}, CredentialBindings: []string{"$model"}, Credentials: []credential.Record{{Binding: "$model", Value: json.RawMessage(`"model-secret-marker"`)}}}
	records, err := client.PrepareCredentials(ctx, s, []core.CredentialInput{{Binding: "data", Value: json.RawMessage(`{"token":"private-tool-value"}`)}})
	if err != nil || len(records) != 1 {
		t.Fatalf("admission: %v", err)
	}
	s.Credentials = append(s.Credentials, records...)
	s.CredentialBindings = append(s.CredentialBindings, "data")
	if err := client.ValidateCredentials(ctx, s); err != nil {
		t.Fatal(err)
	}
	missing := s
	missing.Credentials = s.Credentials[:1]
	if err := client.ValidateCredentials(ctx, missing); err == nil {
		t.Fatal("missing tool record accepted")
	}
	if requests != 3 {
		t.Fatalf("expected credential operations through Tools RPC, got %d", requests)
	}
}
