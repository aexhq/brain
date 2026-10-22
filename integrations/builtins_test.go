package integrations

import (
	"context"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/aexhq/brain"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/tools"
)

func TestS3DefaultOverrideMissingAndRejectedCredentials(t *testing.T) {
	var mu sync.Mutex
	requests := []string{}
	cloud := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		auth := r.Header.Get("Authorization")
		mu.Lock()
		requests = append(requests, auth)
		mu.Unlock()
		if !strings.HasSuffix(r.URL.Path, "/objects/bG9jYWw/note.txt") {
			t.Errorf("unexpected tenant-scoped key: %s", r.URL.Path)
		}
		if strings.Contains(auth, "Credential=invalid/") {
			w.WriteHeader(403)
			return
		}
		if strings.Contains(auth, "Credential=customer/") && r.Header.Get("X-Amz-Security-Token") != "temporary-session" {
			t.Error("session token was lost")
		}
		_, _ = w.Write([]byte("private text"))
	}))
	defer cloud.Close()
	cert := filepath.Join(t.TempDir(), "ca.pem")
	if err := os.WriteFile(cert, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: cloud.Certificate().Raw}), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("AWS_CA_BUNDLE", cert)
	t.Setenv("S3_DEFAULT", `{"accessKeyId":"server","secretAccessKey":"server-secret"}`)
	parameters, _ := json.Marshal(map[string]string{"bucket": "bucket", "region": "us-east-1", "endpoint": cloud.URL, "prefix": "objects"})
	definition := Installation{Name: "aws_s3", Parameters: parameters, Operations: []string{"read"}, CredentialEnv: "S3_DEFAULT"}
	provider, err := builtin(definition)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	bind := func(secret string) ([]brain.ManagedTool, error) {
		var raw json.RawMessage
		if secret != "" {
			raw = json.RawMessage(secret)
		}
		return provider.Bind(ctx, brain.BindingContext{Tenant: "local", Session: core.ID(), Parameters: json.RawMessage(`{}`), Credential: raw})
	}
	for _, secret := range []string{"", `{"accessKeyId":"customer","secretAccessKey":"private-key","sessionToken":"temporary-session"}`, `{"accessKeyId":"invalid","secretAccessKey":"rejected-key"}`} {
		bound, err := bind(secret)
		if err != nil {
			t.Fatal(err)
		}
		result := bound[0].Handler(ctx, brain.ToolRequest{ToolScope: core.ToolScope{Tenant: "local", Session: core.ID(), Call: core.ID()}, Arguments: json.RawMessage(`{"key":"note.txt"}`)}, nil)
		if result.IsError != strings.Contains(secret, "invalid") {
			t.Fatalf("unexpected result: %+v", result)
		}
	}
	mu.Lock()
	recorded := append([]string(nil), requests...)
	mu.Unlock()
	if len(recorded) != 3 || !strings.Contains(recorded[0], "Credential=server/") || !strings.Contains(recorded[1], "Credential=customer/") || !strings.Contains(recorded[2], "Credential=invalid/") {
		t.Fatalf("credential selection or retries: %v", requests)
	}
	for _, rejected := range []string{`{}`, `null`, `{"accessKeyId":"partial"}`, `{"token":"unknown"}`} {
		if _, err := bind(rejected); core.ErrorCode(err) != core.Forbidden {
			t.Fatalf("rejected override selected defaults: %q, %v", rejected, err)
		}
	}
	definition.CredentialEnv = ""
	provider, err = builtin(definition)
	if err != nil {
		t.Fatal("installation without credentials must succeed", err)
	}
	if _, err := bind(""); core.ErrorCode(err) != core.Forbidden {
		t.Fatalf("missing credentials accepted: %v", err)
	}
	mu.Lock()
	count := len(requests)
	mu.Unlock()
	if count != 3 {
		t.Fatal("missing or malformed credentials reached the provider")
	}
	// Installing credentials alone allows sessions to supply their own resource settings.
	definition.CredentialEnv = "S3_DEFAULT"
	definition.Parameters, _ = json.Marshal(map[string]string{"endpoint": cloud.URL, "prefix": "objects"})
	provider, err = builtin(definition)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := provider.Bind(ctx, brain.BindingContext{Tenant: "local", Parameters: json.RawMessage(`{"bucket":"bucket","region":"us-east-1"}`)}); err != nil {
		t.Fatal("generic installation rejected resource selection with default credentials", err)
	}
}

func TestSandboxBindingCredentialsArePrivateAndResourcesAreHostAuthorized(t *testing.T) {
	var mu sync.Mutex
	seen := []map[string]any{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer private-tool-host-token-with-32-bytes" {
			t.Error("missing tool host authentication")
		}
		var request map[string]any
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			t.Error(err)
		}
		mu.Lock()
		seen = append(seen, request)
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"content":[{"type":"text","text":"ok"}]}`))
	}))
	defer server.Close()
	t.Setenv("MODAL_DEFAULT", `{"tokenId":"server-id","tokenSecret":"server-secret"}`)
	t.Setenv("TOOL_HOST", "private-tool-host-token-with-32-bytes")
	definition := Installation{Name: "modal", Parameters: json.RawMessage(`{"app":"installed-app"}`), Operations: []string{"inspect"}, CredentialEnv: "MODAL_DEFAULT", URL: server.URL, TokenEnv: "TOOL_HOST", Tools: []brain.Tool{{Name: "inspect", Description: "Inspect a sandbox", InputSchema: json.RawMessage(`{"type":"object"}`)}}}
	provider, err := builtin(definition)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	for _, credential := range []json.RawMessage{nil, json.RawMessage(`{"tokenId":"session-id","tokenSecret":"session-secret"}`)} {
		bound, err := provider.Bind(ctx, brain.BindingContext{Tenant: "tenant", Session: "session", Parameters: json.RawMessage(`{}`), Credential: credential})
		if err != nil {
			t.Fatal(err)
		}
		serialized, _ := json.Marshal(bound)
		if strings.Contains(string(serialized), "secret") {
			t.Fatal("private context leaked into tool definitions")
		}
		registry, err := tools.New(bound)
		if err != nil {
			t.Fatal(err)
		}
		result := registry.Call(ctx, core.ToolScope{Tenant: "tenant", Session: "session", Call: core.ID()}, core.Call{Name: "inspect", Arguments: json.RawMessage(`{}`)}, nil)
		registry.Close()
		if result.IsError {
			t.Fatal(result)
		}
	}
	for index, expected := range []string{"server-secret", "session-secret"} {
		mu.Lock()
		request := seen[index]
		mu.Unlock()
		private := request["context"].(map[string]any)["credential"].(map[string]any)
		if private["tokenSecret"] != expected || request["session_id"] != "session" {
			t.Fatal("credential scope crossed sessions")
		}
		if len(request["arguments"].(map[string]any)) != 0 {
			t.Fatal("credential leaked into model arguments")
		}
	}
	if _, err := provider.Bind(ctx, brain.BindingContext{Parameters: json.RawMessage(`{"app":"another-app"}`)}); core.ErrorCode(err) != core.Forbidden {
		t.Fatal("server credentials authorized an uninstalled resource")
	}
	definition.CredentialEnv = ""
	provider, err = builtin(definition)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := provider.Bind(ctx, brain.BindingContext{Parameters: json.RawMessage(`{}`)}); core.ErrorCode(err) != core.Forbidden {
		t.Fatal("missing sandbox credentials accepted")
	}
}
