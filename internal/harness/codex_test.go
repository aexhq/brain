package harness

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
)

func TestCodexOwnedHostFailureWithConcurrentNativeThreads(t *testing.T) {
	started := make(chan struct{}, 4)
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		started <- struct{}{}
		<-r.Context().Done()
	}))
	defer provider.Close()
	script, err := filepath.Abs("../../runners/codex/src/main.mjs")
	if err != nil {
		t.Fatal(err)
	}
	pool, err := NewHostPool(PoolLimits{})
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	f := Codex([]string{"node", script})
	f.Pool = pool
	f.MaxShared = 2
	f.MaxActiveTurns = 2
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	opts := func() Options {
		return Options{Lifetime: ctx, SessionID: core.ID(), Dir: t.TempDir(), Config: core.Config{Model: "openai/gpt-4.1-mini", ReasoningEffort: "off"}, Provider: Provider{APIKey: "native-fixture-key", API: "openai-responses", BaseURL: provider.URL + "/v1"}, Tools: []core.Tool{}, Event: func(string, json.RawMessage) error { return nil }}
	}
	a, err := f.Open(ctx, opts())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	b, err := f.Open(ctx, opts())
	if err != nil {
		t.Fatal(err)
	}
	defer b.Close()
	if a.(*attachment).host != b.(*attachment).host {
		t.Fatal("compatible native threads did not share their host")
	}
	done := make(chan error, 2)
	go func() { done <- a.Prompt(ctx, "hold") }()
	go func() { done <- b.Prompt(ctx, "hold") }()
	for range 2 {
		select {
		case <-started:
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
	}
	if err := a.(*attachment).host.cmd.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		select {
		case err := <-done:
			if err == nil {
				t.Fatal("native host loss returned success")
			}
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
	}
	recovered, err := f.Open(ctx, opts())
	if err != nil {
		t.Fatal(err)
	}
	defer recovered.Close()
	select {
	case <-started:
		t.Fatal("reopening native host continued inference")
	default:
	}
}
