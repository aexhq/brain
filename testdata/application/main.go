// Built from a separate module by TestSeparateGoApplication.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"github.com/aexhq/brain"
	"github.com/aexhq/brain/integrations"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
)

type factory struct{}

func (factory) SupportsSteering() bool { return false }
func (factory) Open(context.Context, brain.HarnessOptions) (brain.Harness, error) {
	return nil, fmt.Errorf("test must remain passive")
}

func main() {
	ctx := context.Background()
	cfg := brain.DefaultConfig()
	cfg.DataDir = filepath.Join("state", "sessions")
	if err := os.Setenv("CONSUMER_TOKEN", "consumer-secret-with-24-bytes"); err != nil {
		panic(err)
	}
	cfg.Tokens = []brain.Token{{Env: "CONSUMER_TOKEN", Grant: brain.Grant{Tenant: "one", Manage: true}}}
	options := brain.Options{Config: cfg, Models: map[string]brain.ModelProvider{"test": {APIKey: "startup-model-key"}}, Harnesses: map[string]brain.HarnessFactory{"custom": factory{}}}
	options.Tools = []brain.ManagedTool{brain.NewTool(brain.Tool{Name: "hello", Description: "Greet", InputSchema: json.RawMessage(`{"type":"object"}`)}, []string{"one"}, func(context.Context, brain.ToolRequest, func(json.RawMessage)) brain.Result {
		return brain.Result{Content: []brain.Content{{Type: "text", Text: "hello"}}}
	})}
	if err := brain.Validate(options); err != nil {
		panic(err)
	}
	if _, err := os.Stat(cfg.DataDir); !os.IsNotExist(err) {
		panic("validation opened storage")
	}
	server, err := brain.New(ctx, options)
	if err != nil {
		panic(err)
	}
	// Optional integrations compile against the same public types.
	_ = integrations.Sessions(server, func(context.Context, brain.BindingContext, string) error { return nil })
	request := httptest.NewRequest(http.MethodPost, "/sessions", strings.NewReader(`{"harness":"custom","config":{"model":"test/model","tools":[]}}`))
	request.Header.Set("Authorization", "Bearer consumer-secret-with-24-bytes")
	response := httptest.NewRecorder()
	server.Handler().ServeHTTP(response, request)
	if response.Code != 201 {
		panic(response.Body.String())
	}
	var session brain.Session
	if err := json.Unmarshal(response.Body.Bytes(), &session); err != nil {
		panic(err)
	}
	if _, err := server.ReadSession(ctx, "other", session.ID); err == nil {
		panic("foreign session visible")
	}
	if events, err := server.ReadEvents(ctx, "one", session.ID, "", 100); err != nil || len(events) != 0 {
		panic("passive session read failed")
	}
	if err := server.Shutdown(ctx); err != nil {
		panic(err)
	}
	if err := server.Shutdown(ctx); err != nil {
		panic(err)
	}
}
