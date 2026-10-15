package tools

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/modelcontextprotocol/go-sdk/mcp"
)

func TestMCPDiscoveryCallIsolationAndCancellation(t *testing.T) {
	t.Setenv("TEST_MCP_TOKEN", "test-secret")
	var calls, progress atomic.Int32
	started, cancelled := make(chan struct{}), make(chan struct{})
	s := mcp.NewServer(&mcp.Implementation{Name: "fixture", Version: "dev"}, nil)
	s.AddTool(&mcp.Tool{Name: "echo", Description: "Echo", InputSchema: json.RawMessage(`{"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false}`)}, func(ctx context.Context, req *mcp.CallToolRequest) (*mcp.CallToolResult, error) {
		calls.Add(1)
		scope, ok := req.Params.Meta["brain.dev/scope"].(map[string]any)
		if !ok || scope["tenant_id"] != "a" || scope["session_id"] != "session" || scope["call_id"] != "call" {
			t.Error("missing trusted scope")
		}
		var args struct{ Text string }
		_ = json.Unmarshal(req.Params.Arguments, &args)
		if args.Text == "wait" {
			close(started)
			<-ctx.Done()
			close(cancelled)
			return nil, ctx.Err()
		}
		if args.Text == "large" {
			return &mcp.CallToolResult{Content: []mcp.Content{&mcp.TextContent{Text: strings.Repeat("x", 100000)}}}, nil
		}
		if args.Text == "audio" {
			return &mcp.CallToolResult{Content: []mcp.Content{&mcp.AudioContent{Data: []byte("audio"), MIMEType: "audio/wav"}}}, nil
		}
		_ = req.Session.NotifyProgress(ctx, &mcp.ProgressNotificationParams{ProgressToken: req.Params.GetProgressToken(), Progress: 1, Message: "ready"})
		return &mcp.CallToolResult{Content: []mcp.Content{&mcp.TextContent{Text: args.Text}}, StructuredContent: map[string]any{"ok": true}, IsError: args.Text == "error"}, nil
	})
	handler := mcp.NewStreamableHTTPHandler(func(*http.Request) *mcp.Server { return s }, &mcp.StreamableHTTPOptions{Stateless: true, PropagateRequestCancellation: true})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer test-secret" {
			t.Error("missing credential")
			w.WriteHeader(401)
			return
		}
		handler.ServeHTTP(w, r)
	}))
	defer server.Close()
	bindings, err := DiscoverMCP(context.Background(), Binding{URL: server.URL, TokenEnv: "TEST_MCP_TOKEN", Tenants: []string{"a"}, MaxOutputBytes: 1024}, []string{"echo"})
	if err != nil {
		t.Fatal(err)
	}
	registry, err := New(bindings)
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	call := core.Call{Name: "echo", Arguments: json.RawMessage(`{"text":"hello"}`)}
	scope := Scope{Tenant: "a", Session: "session", Call: "call"}
	if got := registry.Call(context.Background(), Scope{Tenant: "b"}, call, nil); !got.IsError || calls.Load() != 0 {
		t.Fatal("cross tenant dispatch")
	}
	call.Arguments = json.RawMessage(`{"text":1}`)
	if got := registry.Call(context.Background(), scope, call, nil); !got.IsError || calls.Load() != 0 {
		t.Fatal("invalid input dispatched")
	}
	for _, text := range []string{"hello", "error", "large", "audio"} {
		call.Arguments, _ = json.Marshal(map[string]string{"text": text})
		got := registry.Call(context.Background(), scope, call, func(json.RawMessage) { progress.Add(1) })
		if got.IsError != (text != "hello") {
			t.Fatalf("%s: %+v", text, got)
		}
		if text == "hello" && (len(got.Content) != 2 || got.Content[0].Text != "hello") {
			t.Fatalf("lost content: %+v", got)
		}
		if text == "error" && got.UnknownOutcome {
			t.Fatal("tool error incorrectly ambiguous")
		}
	}
	if calls.Load() != 4 || progress.Load() != 2 {
		t.Fatalf("calls=%d progress=%d", calls.Load(), progress.Load())
	}
	ctx, cancel := context.WithCancel(context.Background())
	call.Arguments = json.RawMessage(`{"text":"wait"}`)
	result := make(chan core.Result, 1)
	go func() { result <- registry.Call(ctx, scope, call, nil) }()
	select {
	case <-started:
	case <-time.After(5 * time.Second):
		t.Fatal("call did not start")
	}
	cancel()
	select {
	case got := <-result:
		if !got.UnknownOutcome {
			t.Fatal("cancelled call not ambiguous")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("call not cancelled")
	}
	select {
	case <-cancelled:
	case <-time.After(5 * time.Second):
		t.Fatal("server not cancelled")
	}
	if calls.Load() != 5 {
		t.Fatal("cancelled action retried")
	}
}
