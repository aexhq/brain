package tools

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/aexhq/brain/internal/core"
)

func registry(t *testing.T, url string, limit int64) *Registry {
	t.Helper()
	r, err := New([]Binding{{Tool: core.Tool{Name: "echo", Description: "Echo", InputSchema: json.RawMessage(`{"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false}`)}, URL: url, Tenants: []string{"a"}, MaxOutputBytes: limit}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(r.Close)
	return r
}

func TestValidationScopeAndNoRetry(t *testing.T) {
	var count atomic.Int32
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { count.Add(1); w.WriteHeader(503) }))
	defer s.Close()
	r := registry(t, s.URL, 1024)
	call := core.Call{ID: "call", Name: "echo", Arguments: json.RawMessage(`{"text":"hello"}`)}
	if got := r.Call(context.Background(), core.ToolScope{Tenant: "b"}, call, nil); !got.IsError || count.Load() != 0 {
		t.Fatal("cross-tenant call executed")
	}
	bad := call
	bad.Arguments = json.RawMessage(`{"text":4}`)
	if got := r.Call(context.Background(), core.ToolScope{Tenant: "a"}, bad, nil); !got.IsError || count.Load() != 0 {
		t.Fatal("invalid input executed")
	}
	got := r.Call(context.Background(), core.ToolScope{Tenant: "a"}, call, nil)
	if !got.IsError || !got.UnknownOutcome || count.Load() != 1 {
		t.Fatalf("unexpected retries/result: %d %+v", count.Load(), got)
	}
}

func TestSSEAndOutputBounds(t *testing.T) {
	s := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "event: progress\ndata: {\"text\":\"waiting\"}\n\nevent: result\ndata: {\"content\":[{\"type\":\"text\",\"text\":\"done\"}]}\n\n")
	}))
	defer s.Close()
	r := registry(t, s.URL, 1024)
	n := 0
	call := core.Call{Name: "echo", Arguments: json.RawMessage(`{"text":"hello"}`)}
	got := r.Call(context.Background(), core.ToolScope{Tenant: "a"}, call, func(json.RawMessage) { n++ })
	if got.IsError || n != 1 || got.Content[0].Text != "done" {
		t.Fatalf("result: %+v progress=%d", got, n)
	}
	r = registry(t, s.URL, 32)
	got = r.Call(context.Background(), core.ToolScope{Tenant: "a"}, call, nil)
	if !got.IsError {
		t.Fatal("output limit ignored")
	}
}

func TestRedirectDoesNotForwardCredentials(t *testing.T) {
	var count atomic.Int32
	dst := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { count.Add(1) }))
	defer dst.Close()
	src := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { http.Redirect(w, r, dst.URL, 307) }))
	defer src.Close()
	r := registry(t, src.URL, 1024)
	got := r.Call(context.Background(), core.ToolScope{Tenant: "a"}, core.Call{Name: "echo", Arguments: json.RawMessage(`{"text":"hello"}`)}, nil)
	if !got.IsError || count.Load() != 0 {
		t.Fatal("redirect executed")
	}
}
