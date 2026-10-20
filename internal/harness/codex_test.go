package harness

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
)

func TestNativeBrainCallbackFailureStopsTurnAndPreservesPeer(t *testing.T) {
	for _, name := range []string{"codex", "opencode"} {
		t.Run(name, func(t *testing.T) {
			var requests atomic.Int32
			provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				_, _ = io.Copy(io.Discard, r.Body)
				n := requests.Add(1)
				tool := n == 1 || n == 3
				writeNativeResponse(t, w, tool)
			}))
			defer provider.Close()
			ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
			defer cancel()
			script, err := filepath.Abs("../../runners/" + name + "/src/main.mjs")
			if err != nil {
				t.Fatal(err)
			}
			factory := Codex([]string{"node", script})
			if name == "opencode" {
				factory = OpenCode([]string{"node", script})
			}
			pool, err := NewHostPool(PoolLimits{})
			if err != nil {
				t.Fatal(err)
			}
			defer pool.Close()
			factory.Pool = pool
			opts := func() Options {
				return Options{Lifetime: ctx, SessionID: core.ID(), Dir: t.TempDir(), Config: core.Config{Model: "openai/gpt-4.1-mini", ReasoningEffort: "off"}, Provider: Provider{APIKey: "native-fixture-key", API: "openai-responses", BaseURL: provider.URL + "/v1"}, Tools: []core.Tool{{Name: "echo", Description: "Echo", InputSchema: json.RawMessage(`{"type":"object"}`)}}, Event: func(string, json.RawMessage) error { return nil }, Call: func(context.Context, core.Call) (core.Result, error) {
					return core.Result{}, errors.New("local Brain failure")
				}}
			}
			a, err := factory.Open(ctx, opts())
			if err != nil {
				t.Fatal(err)
			}
			defer a.Close()
			peer, err := factory.Open(ctx, opts())
			if err != nil {
				t.Fatal(err)
			}
			defer peer.Close()
			if a.(*attachment).host != peer.(*attachment).host {
				t.Fatal("native peers did not share a host")
			}
			if err := a.Prompt(ctx, core.Text{Text: "invoke echo"}); err == nil || core.ErrorCode(err) != core.Internal {
				t.Fatalf("Brain callback failure was swallowed: %v", err)
			}
			if requests.Load() != 1 {
				t.Fatalf("Brain failure continued inference: %d requests", requests.Load())
			}
			if err := peer.Prompt(ctx, core.Text{Text: "healthy peer"}); err != nil {
				t.Fatalf("peer was stopped by another turn's callback: %v", err)
			}
			if requests.Load() != 2 {
				t.Fatal("healthy peer did not complete its own request")
			}
			external := opts()
			external.Call = func(context.Context, core.Call) (core.Result, error) {
				return core.ToolError("external service denied the action", false), nil
			}
			toolError, err := factory.Open(ctx, external)
			if err != nil {
				t.Fatal(err)
			}
			defer toolError.Close()
			if err := toolError.Prompt(ctx, core.Text{Text: "invoke a denied external tool"}); err != nil {
				t.Fatalf("external tool error stopped the native loop: %v", err)
			}
			if requests.Load() != 4 {
				t.Fatal("external tool error did not return to the model once")
			}
			unrecorded := opts()
			unrecorded.Event = func(string, json.RawMessage) error { return errors.New("model start could not be recorded") }
			noDispatch, err := factory.Open(ctx, unrecorded)
			if err != nil {
				t.Fatal(err)
			}
			defer noDispatch.Close()
			if err := noDispatch.Prompt(ctx, core.Text{Text: "do not dispatch without acknowledgement"}); err == nil || core.ErrorCode(err) != core.Internal {
				t.Fatalf("rejected observation did not stop the turn: %v", err)
			}
			if requests.Load() != 4 {
				t.Fatal("model dispatched before its start was acknowledged")
			}
			if err := peer.Prompt(ctx, core.Text{Text: "peer remains usable after observation failure"}); err != nil {
				t.Fatal(err)
			}
			if requests.Load() != 5 {
				t.Fatal("observation failure stopped a compatible peer")
			}
		})
	}
}

func writeNativeResponse(t *testing.T, w http.ResponseWriter, tool bool) {
	t.Helper()
	w.Header().Set("Content-Type", "text/event-stream")
	sequence := 0
	send := func(kind string, data map[string]any) {
		data["type"], data["sequence_number"] = kind, sequence
		sequence++
		b, err := json.Marshal(data)
		if err != nil {
			t.Error(err)
			return
		}
		_, _ = fmt.Fprintf(w, "event: %s\ndata: %s\n\n", kind, b)
	}
	response := map[string]any{"id": "resp_fixture", "object": "response", "created_at": 1, "status": "in_progress", "model": "gpt-4.1-mini", "output": []any{}, "usage": nil}
	send("response.created", map[string]any{"response": response})
	var item map[string]any
	if tool {
		item = map[string]any{"id": "fc_fixture", "type": "function_call", "call_id": "call_fixture", "name": "echo", "arguments": "", "status": "in_progress"}
		send("response.output_item.added", map[string]any{"output_index": 0, "item": item})
		send("response.function_call_arguments.delta", map[string]any{"output_index": 0, "item_id": item["id"], "delta": "{}"})
		send("response.function_call_arguments.done", map[string]any{"output_index": 0, "item_id": item["id"], "arguments": "{}"})
		item["arguments"] = "{}"
	} else {
		item = map[string]any{"id": "msg_fixture", "type": "message", "role": "assistant", "status": "in_progress", "content": []any{}}
		send("response.output_item.added", map[string]any{"output_index": 0, "item": item})
		part := map[string]any{"type": "output_text", "text": "", "annotations": []any{}}
		send("response.content_part.added", map[string]any{"output_index": 0, "item_id": item["id"], "content_index": 0, "part": part})
		send("response.output_text.delta", map[string]any{"output_index": 0, "item_id": item["id"], "content_index": 0, "delta": "healthy peer"})
		part["text"] = "healthy peer"
		item["content"] = []any{part}
		send("response.output_text.done", map[string]any{"output_index": 0, "item_id": item["id"], "content_index": 0, "text": "healthy peer"})
		send("response.content_part.done", map[string]any{"output_index": 0, "item_id": item["id"], "content_index": 0, "part": part})
	}
	item["status"] = "completed"
	send("response.output_item.done", map[string]any{"output_index": 0, "item": item})
	response["status"], response["output"] = "completed", []any{item}
	send("response.completed", map[string]any{"response": response})
}

func TestNativeOwnedHostFailureWithConcurrentSessions(t *testing.T) {
	for _, name := range []string{"codex", "opencode"} {
		t.Run(name, func(t *testing.T) { testNativeOwnedHostFailure(t, name) })
	}
}

func testNativeOwnedHostFailure(t *testing.T, name string) {
	started := make(chan struct{}, 4)
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		started <- struct{}{}
		<-r.Context().Done()
	}))
	defer provider.Close()
	script, err := filepath.Abs("../../runners/" + name + "/src/main.mjs")
	if err != nil {
		t.Fatal(err)
	}
	pool, err := NewHostPool(PoolLimits{})
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	f := Codex([]string{"node", script})
	if name == "opencode" {
		f = OpenCode([]string{"node", script})
	}
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
	go func() { done <- a.Prompt(ctx, core.Text{Text: "hold"}) }()
	go func() { done <- b.Prompt(ctx, core.Text{Text: "hold"}) }()
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
