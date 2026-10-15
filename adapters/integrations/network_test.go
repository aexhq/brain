package integrations

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestSearchProviderProtocols(t *testing.T) {
	for _, provider := range []string{"brave", "tavily", "exa"} {
		t.Run(provider, func(t *testing.T) {
			var requests atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests.Add(1)
				switch provider {
				case "brave":
					if r.Method != "GET" || r.Header.Get("X-Subscription-Token") != "scoped" || r.URL.Query().Get("q") != "test query" {
						t.Error("invalid Brave wire request")
					}
					_ = json.NewEncoder(w).Encode(map[string]any{"web": map[string]any{"results": []any{map[string]string{"title": "Brave", "url": "https://example.com", "description": "native description"}}}})
				case "tavily":
					var body map[string]any
					_ = json.NewDecoder(r.Body).Decode(&body)
					if r.Method != "POST" || body["api_key"] != "scoped" || body["query"] != "test query" {
						t.Error("invalid Tavily wire request")
					}
					_ = json.NewEncoder(w).Encode(map[string]any{"results": []any{map[string]string{"title": "Tavily", "url": "https://example.com", "content": "native content"}}})
				case "exa":
					if r.Method != "POST" || r.Header.Get("X-Api-Key") != "scoped" {
						t.Error("invalid Exa wire request")
					}
					_ = json.NewEncoder(w).Encode(map[string]any{"results": []any{map[string]string{"title": "Exa", "url": "https://example.com", "text": "native text", "publishedDate": "2026-01-01"}}})
				}
			}))
			defer server.Close()
			search := Search{Provider: provider, Endpoint: server.URL, Client: server.Client(), Resolve: func(context.Context, string, string) (string, error) { return "scoped", nil }}
			receipt, err := search.Invoke(context.Background(), "tenant", wire.ConnectionTemplate{Capabilities: []string{"search"}, Secrets: map[string]string{"api_key": "reference"}},
				wire.Execution{Arguments: kernel.JSON(map[string]any{"query": "test query", "count": 1}), MaxOutputBytes: 8192})
			if err != nil || receipt.Outcome != wire.OutcomeCompleted || !strings.Contains(string(receipt.Result), "native") {
				t.Fatalf("search: %+v %v", receipt, err)
			}
			if strings.Contains(string(receipt.Result), "scoped") || requests.Load() != 1 {
				t.Fatal("credential leaked or search repeated")
			}
		})
	}
}

func TestMCPNegotiatesAndReadsOneCorrelatedSSEOutcome(t *testing.T) {
	var tools atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			ID     string          `json:"id"`
			Method string          `json:"method"`
			Params json.RawMessage `json:"params"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
			return
		}
		switch body.Method {
		case "initialize":
			w.Header().Set("Mcp-Session-Id", "fixture-session")
			_ = json.NewEncoder(w).Encode(map[string]any{"jsonrpc": "2.0", "id": body.ID, "result": map[string]any{"protocolVersion": "2025-06-18", "capabilities": map[string]any{"tools": map[string]any{}}}})
		case "notifications/initialized":
			if r.Header.Get("MCP-Protocol-Version") != "2025-06-18" {
				t.Error("negotiated version was not used")
			}
			w.WriteHeader(202)
		case "tools/call":
			tools.Add(1)
			if r.Header.Get("Mcp-Session-Id") != "fixture-session" {
				t.Error("MCP session identity was lost")
			}
			w.Header().Set("Content-Type", "text/event-stream")
			fmt.Fprint(w, "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n")
			fmt.Fprintf(w, "event: message\ndata: %s\n\n", kernel.JSON(map[string]any{"jsonrpc": "2.0", "id": body.ID, "result": map[string]any{"content": []any{map[string]string{"type": "text", "text": "tool executed"}}}}))
			w.(http.Flusher).Flush()
			<-r.Context().Done()
		default:
			t.Errorf("unexpected MCP method %s", body.Method)
		}
	}))
	defer server.Close()
	mcp := MCP{Development: true, Client: server.Client()}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	receipt, err := mcp.Invoke(ctx, "tenant", wire.ConnectionTemplate{Capabilities: []string{"tools"}, Configuration: kernel.JSON(map[string]string{"url": server.URL, "tool": "echo"})},
		wire.Execution{Operation: wire.Operation{ID: "operation-1", Action: "echo"}, Arguments: kernel.JSON(map[string]string{"text": "hello"}), MaxOutputBytes: 8192})
	if err != nil || receipt.Outcome != wire.OutcomeCompleted || !strings.Contains(string(receipt.Result), "tool executed") {
		t.Fatalf("MCP: %+v %v", receipt, err)
	}
	if tools.Load() != 1 {
		t.Fatal("tool invocation was repeated")
	}
}
