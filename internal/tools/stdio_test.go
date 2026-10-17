package tools

import (
	"context"
	"encoding/json"
	"github.com/aexhq/brain/internal/core"
	"github.com/modelcontextprotocol/go-sdk/mcp"
	"os"
	"testing"
	"time"
)

func TestMCPStdioChild(t *testing.T) {
	if os.Getenv("BRAIN_MCP_TEST_CHILD") != "1" {
		return
	}
	server := mcp.NewServer(&mcp.Implementation{Name: "test", Version: "1"}, nil)
	server.AddTool(&mcp.Tool{Name: "echo", Description: "Echo", InputSchema: json.RawMessage(`{"type":"object"}`), OutputSchema: json.RawMessage(`{"type":"object","required":["tenant"]}`)}, func(ctx context.Context, req *mcp.CallToolRequest) (*mcp.CallToolResult, error) {
		if string(req.Params.Arguments) == `{"crash":true}` {
			os.Exit(7)
		}
		scope := req.Params.Meta["brain.dev/scope"].(map[string]any)
		return &mcp.CallToolResult{Content: []mcp.Content{&mcp.TextContent{Text: "ok"}}, StructuredContent: map[string]any{"tenant": scope["tenant_id"]}}, nil
	})
	if err := server.Run(context.Background(), &mcp.IOTransport{Reader: os.Stdin, Writer: os.Stdout}); err != nil {
		os.Exit(1)
	}
	os.Exit(0)
}

func TestMCPStdioDiscoveryScopeAndProcessLoss(t *testing.T) {
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	binding := Binding{Tenants: []string{"a"}, Stdio: &StdioConfig{Command: []string{exe, "-test.run=^TestMCPStdioChild$"}, Env: map[string]string{"BRAIN_MCP_TEST_CHILD": "1"}}}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	definitions, err := DiscoverMCP(ctx, binding, []string{"echo"})
	if err != nil {
		t.Fatal(err)
	}
	if len(definitions) != 1 || len(definitions[0].OutputSchema) == 0 {
		t.Fatal("discovery lost schema")
	}
	registry, err := New(definitions)
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	scope := core.ToolScope{Tenant: "a", Session: core.ID(), Call: core.ID()}
	got := registry.Call(ctx, scope, core.Call{Name: "echo", Arguments: json.RawMessage(`{}`)}, nil)
	if got.IsError || string(got.StructuredContent) != `{"tenant":"a"}` {
		t.Fatalf("%+v", got)
	}
	got = registry.Call(ctx, scope, core.Call{Name: "echo", Arguments: json.RawMessage(`{"crash":true}`)}, nil)
	if !got.IsError || !got.UnknownOutcome {
		t.Fatalf("lost process outcome: %+v", got)
	}
	if _, err := DiscoverMCP(ctx, binding, []string{"missing"}); err == nil {
		t.Fatal("missing tool accepted")
	}
}
