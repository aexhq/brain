package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/tools"
)

func TestExternalApprovalAndDelegatedInspection(t *testing.T) {
	sessionID := core.ID()
	brain := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/sessions/"+sessionID+"/events" || r.Header.Get("Authorization") != "Bearer scoped-token" {
			t.Errorf("wrong inspection scope %s", r.URL.Path)
			w.WriteHeader(403)
			return
		}
		_, _ = io.WriteString(w, `{"events":[],"next_cursor":""}`)
	}))
	defer brain.Close()
	s := &service{key: "tool-token", operator: "operator-token", tenant: "a", brainURL: brain.URL, pending: map[string]*approval{}, client: brain.Client()}
	server := httptest.NewServer(s.handler())
	defer server.Close()
	t.Setenv("BRAIN_TOOLS_KEY", s.key)
	registry, err := tools.New(bindings(server.URL, "a"))
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	scope := core.ToolScope{Tenant: "a", Session: sessionID, Call: core.ID(), BrainURL: brain.URL, InspectionToken: "scoped-token"}
	inspected := registry.Call(ctx, scope, core.Call{Name: "inspect_session", Arguments: json.RawMessage(`{}`)}, nil)
	if inspected.IsError || len(inspected.Content) != 1 {
		t.Fatalf("inspection failed: %+v", inspected)
	}
	progress := make(chan struct{}, 1)
	answer := make(chan core.Result, 1)
	go func() {
		answer <- registry.Call(ctx, scope, core.Call{Name: "ask_approval", Arguments: json.RawMessage(`{"question":"Continue?"}`)}, func(json.RawMessage) { progress <- struct{}{} })
	}()
	select {
	case <-progress:
	case <-ctx.Done():
		t.Fatal("approval did not start")
	}
	request := func(method, path, token string, body []byte) int {
		t.Helper()
		req, _ := http.NewRequestWithContext(ctx, method, server.URL+path, bytes.NewReader(body))
		req.Header.Set("Authorization", "Bearer "+token)
		res, err := server.Client().Do(req)
		if err != nil {
			t.Fatal(err)
		}
		defer res.Body.Close()
		_, _ = io.Copy(io.Discard, res.Body)
		return res.StatusCode
	}
	if status := request("GET", "/approvals", s.key, nil); status != 401 {
		t.Fatal("tool credential could inspect operator approvals")
	}
	if status := request("GET", "/approvals", s.operator, nil); status != 200 {
		t.Fatal(status)
	}
	if status := request("POST", "/approvals/"+scope.Call, s.operator, []byte(`{"approved":false}`)); status != 204 {
		t.Fatal(status)
	}
	select {
	case result := <-answer:
		if !result.IsError || result.UnknownOutcome {
			t.Fatalf("denial is not a normal known tool outcome: %+v", result)
		}
	case <-ctx.Done():
		t.Fatal("approval did not finish")
	}
	s.mu.Lock()
	pending := len(s.pending)
	s.mu.Unlock()
	if pending != 0 {
		t.Fatal("completed approval retained")
	}
	bad := scope
	bad.Tenant = "b"
	if result := registry.Call(ctx, bad, core.Call{Name: "echo", Arguments: json.RawMessage(`{"text":"denied"}`)}, nil); !result.IsError {
		t.Fatal("cross-tenant call accepted")
	}
}
