package api

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/config"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/session"
	"github.com/aexhq/brain/internal/store"
	"github.com/aexhq/brain/internal/tools"
)

func TestRealPiHTTPAndIsolation(t *testing.T) {
	var mu sync.Mutex
	var requests []map[string]any
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var input map[string]any
		if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
			t.Error(err)
			return
		}
		mu.Lock()
		requests = append(requests, input)
		mu.Unlock()
		w.Header().Set("Content-Type", "text/event-stream")
		chunk := map[string]any{"id": "test", "object": "chat.completion.chunk", "created": 1, "model": "gpt-4.1-mini", "choices": []any{map[string]any{"index": 0, "delta": map[string]string{"role": "assistant", "content": "hello from Pi"}, "finish_reason": nil}}}
		b, _ := json.Marshal(chunk)
		fmt.Fprintf(w, "data: %s\n\n", b)
		chunk["choices"] = []any{map[string]any{"index": 0, "delta": map[string]any{}, "finish_reason": "stop"}}
		b, _ = json.Marshal(chunk)
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", b)
	}))
	defer provider.Close()
	db, err := store.Open(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	registry, err := tools.New(nil)
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	_, file, _, _ := runtime.Caller(0)
	runner := filepath.Join(filepath.Dir(file), "..", "..", "runners", "pi", "dist", "main.js")
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	engine, err := session.New(session.Options{Store: db, Tools: registry, Factories: map[string]harness.Factory{"pi": &harness.ProcessFactory{Command: []string{"node", runner}, Logger: logger}}, Provider: func(string) (harness.Provider, error) {
		return harness.Provider{APIKey: "test-key", BaseURL: provider.URL + "/v1", API: "openai-completions"}, nil
	}, MaxActive: 2, MaxLoaded: 16, MaxQueue: 2, MaxInputBytes: 1024, MaxLogBytes: 16 << 20, Logger: logger})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := engine.Shutdown(ctx); err != nil {
			t.Error(err)
		}
	}()
	t.Setenv("BRAIN_TEST_TOKEN_A", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
	t.Setenv("BRAIN_TEST_TOKEN_B", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
	auth := NewAuth([]config.Token{{Env: "BRAIN_TEST_TOKEN_A", Grant: core.Grant{Tenant: "a", Manage: true}}, {Env: "BRAIN_TEST_TOKEN_B", Grant: core.Grant{Tenant: "b", Manage: true}}})
	srv := &Server{Engine: engine, Tools: registry, Auth: auth, Logger: logger, Limits: config.Limits{InputBytes: 1024, Subscribers: 4, SubscriberEvents: 32, SubscriberBytes: 65536}}
	httpServer := httptest.NewServer(srv.Handler())
	defer httpServer.Close()
	request := func(method, path, token string, body any) (int, map[string]any) {
		t.Helper()
		var data []byte
		if body != nil {
			data, _ = json.Marshal(body)
		}
		req, _ := http.NewRequest(method, httpServer.URL+path, bytes.NewReader(data))
		if token != "" {
			req.Header.Set("Authorization", "Bearer "+token)
		}
		res, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatal(err)
		}
		defer res.Body.Close()
		var got map[string]any
		if err := json.NewDecoder(res.Body).Decode(&got); err != nil {
			t.Fatal(err)
		}
		return res.StatusCode, got
	}
	if status, _ := request("GET", "/sessions", "", nil); status != 401 {
		t.Fatal(status)
	}
	a := "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	b := "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	status, created := request("POST", "/sessions", a, core.CreateSession{Harness: "pi", Config: core.Config{Model: "openai/gpt-4.1-mini", Instructions: "stable", ReasoningEffort: "off", Tools: []string{}}})
	if status != 201 {
		t.Fatalf("create: %d %+v", status, created)
	}
	id := created["id"].(string)
	if status, _ := request("GET", "/sessions/"+id, b, nil); status != 404 {
		t.Fatal("cross tenant read", status)
	}
	if status, _ := request("POST", "/sessions/"+id+"/events", a, core.Input{Type: "user.message", Text: "hello", Key: "request"}); status != 202 {
		t.Fatal(status)
	}
	deadline := time.Now().Add(15 * time.Second)
	for {
		status, state := request("GET", "/sessions/"+id, a, nil)
		if status != 200 {
			t.Fatalf("get %d %+v", status, state)
		}
		if state["status"] == "idle" {
			break
		}
		if state["status"] == "failed" || time.Now().After(deadline) {
			_, history := request("GET", "/sessions/"+id+"/events", a, nil)
			t.Fatalf("execution failed: %+v %+v", state, history)
		}
		time.Sleep(10 * time.Millisecond)
	}
	status, history := request("GET", "/sessions/"+id+"/events", a, nil)
	if status != 200 {
		t.Fatal(status)
	}
	var assistant bool
	for _, item := range history["events"].([]any) {
		ev := item.(map[string]any)
		if ev["type"] == "assistant.message" {
			assistant = true
		}
		if ev["type"] == "assistant.delta" {
			t.Fatal("provisional tokens were retained")
		}
	}
	if !assistant {
		t.Fatal("no assistant response")
	}
	mu.Lock()
	count := len(requests)
	first := requests[0]
	mu.Unlock()
	if count != 1 {
		t.Fatal("unexpected inference", count)
	}
	if list, ok := first["tools"].([]any); ok && len(list) > 0 {
		t.Fatal("native tools exposed", list)
	}
	meta, err := engine.Get(context.Background(), core.Grant{Tenant: "a"}, id)
	if err != nil {
		t.Fatal(err)
	}
	delegated, revoke := auth.Delegate(meta)
	if status, _ := request("GET", "/sessions/"+id, delegated, nil); status != 200 {
		t.Fatal("inspection denied")
	}
	if status, _ := request("POST", "/sessions/"+id+"/events", delegated, core.Input{Type: "user.message", Text: "forbidden"}); status != 403 {
		t.Fatal("inspection token could mutate")
	}
	revoke()
	if status, _ := request("GET", "/sessions/"+id, delegated, nil); status != 401 {
		t.Fatal("revocation failed")
	}
	if status, _ := request("GET", "/sessions/"+id+"/events?after=lost", a, nil); status != 409 {
		t.Fatal("lost cursor accepted")
	}
}
