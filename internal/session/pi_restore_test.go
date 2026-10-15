package session

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/persistence"
	"github.com/aexhq/brain/internal/store"
	"github.com/aexhq/brain/internal/tools"
)

func TestPiRestorePreservesNextModelRequest(t *testing.T) {
	var mu sync.Mutex
	var requests []json.RawMessage
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		b, err := io.ReadAll(r.Body)
		if err != nil {
			t.Error(err)
		}
		mu.Lock()
		requests = append(requests, b)
		mu.Unlock()
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = fmt.Fprint(w, "data: {\"id\":\"test\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"saved answer\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"test\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer provider.Close()
	_, file, _, _ := runtime.Caller(0)
	runner := filepath.Join(filepath.Dir(file), "..", "..", "runners", "pi", "dist", "main.js")
	objects := persistence.FileObjects{Root: t.TempDir()}
	newEngine := func() *Engine {
		db, err := store.Open(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		registry, err := tools.New(nil)
		if err != nil {
			t.Fatal(err)
		}
		logger := slog.New(slog.NewTextHandler(io.Discard, nil))
		e, err := New(Options{Store: db, Tools: registry, Factories: map[string]harness.Factory{"pi": &harness.ProcessFactory{Command: []string{"node", runner}, Logger: logger}}, Provider: func(string) (harness.Provider, error) {
			return harness.Provider{APIKey: "test-key", BaseURL: provider.URL + "/v1", API: "openai-completions"}, nil
		}, MaxActive: 1, MaxLoaded: 2, MaxQueue: 2, MaxInputBytes: 4096, MaxLogBytes: 16 << 20, Archive: &persistence.Archive{Objects: objects, MaxSessionBytes: 16 << 20}, SyncQueue: 4, SyncInterval: time.Hour, Logger: logger})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if err := e.Shutdown(ctx); err != nil {
				t.Error(err)
			}
			registry.Close()
			db.Close()
		})
		until(t, e.syncer.admit)
		return e
	}
	ctx := context.Background()
	g := core.Grant{Tenant: "a", Manage: true}
	e := newEngine()
	s, err := e.Create(ctx, g, core.CreateSession{Harness: "pi", Config: core.Config{Model: "openai/gpt-4.1-mini", Instructions: "stable instructions", ReasoningEffort: "off", Tools: []string{}}})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := e.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "remember the first message"}); err != nil {
		t.Fatal(err)
	}
	waitStatus(t, e, g, s.ID, "idle")
	uploaded, err := e.Flush(ctx, g, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Shutdown(ctx); err != nil {
		t.Fatal(err)
	}
	fresh := newEngine()
	if _, err := fresh.Restore(ctx, g, s.ID, uploaded.Checkpoint); err != nil {
		t.Fatal(err)
	}
	if _, err := fresh.Resume(ctx, g, s.ID); err != nil {
		t.Fatal(err)
	}
	mu.Lock()
	count := len(requests)
	mu.Unlock()
	if count != 1 {
		t.Fatal("restoration invoked the model")
	}
	if _, err := fresh.Submit(ctx, g, s.ID, core.Input{Type: "user.message", Text: "continue after restore"}); err != nil {
		t.Fatal(err)
	}
	waitStatus(t, fresh, g, s.ID, "idle")
	mu.Lock()
	defer mu.Unlock()
	if len(requests) != 2 {
		t.Fatalf("unexpected model calls: %d", len(requests))
	}
	var first, next struct {
		Messages []json.RawMessage `json:"messages"`
	}
	if err := json.Unmarshal(requests[0], &first); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(requests[1], &next); err != nil {
		t.Fatal(err)
	}
	if len(next.Messages) <= len(first.Messages) {
		t.Fatal("native context was reset")
	}
	for i, message := range first.Messages {
		if string(message) != string(next.Messages[i]) {
			t.Fatalf("prefix changed after relocation at message %d", i)
		}
	}
	if !strings.Contains(string(requests[1]), "saved answer") || !strings.Contains(string(requests[1]), "continue after restore") {
		t.Fatal("missing saved answer or new input")
	}
}
