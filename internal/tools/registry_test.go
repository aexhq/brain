package tools

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/core"
)

type recordingTransport struct {
	calls, closed int
	request       Request
	deadline      time.Time
	result        core.Result
	wait          bool
}

func (t *recordingTransport) Close() { t.closed++ }
func (t *recordingTransport) Call(ctx context.Context, _ Binding, request Request, progress func(json.RawMessage)) core.Result {
	t.calls++
	t.request = request
	t.deadline, _ = ctx.Deadline()
	if t.wait {
		<-ctx.Done()
		return core.ToolError("cancelled", true)
	}
	if progress != nil {
		progress(json.RawMessage(`{"progress":1}`))
	}
	return t.result
}

func TestTransportSubstitutionPreservesRegistryPolicy(t *testing.T) {
	transport := &recordingTransport{result: core.Result{Content: []core.Content{{Type: "text", Text: "ok"}}}}
	b := Binding{Tool: core.Tool{Name: "echo", Description: "Echo", InputSchema: json.RawMessage(`{"type":"object"}`)}, URL: "https://tools.example.test", Transport: "custom", Tenants: []string{"one"}, TimeoutSeconds: 1, MaxOutputBytes: 256}
	r, err := NewWithTransports([]Binding{b}, map[string]Transport{"custom": transport})
	if err != nil {
		t.Fatal(err)
	}
	call := core.Call{Name: "echo", Arguments: json.RawMessage(`{}`)}
	scope := core.ToolScope{Tenant: "one", Session: "session", Call: "call", InspectionToken: "secret", BrainURL: "https://brain.test"}
	if got := r.Call(context.Background(), core.ToolScope{Tenant: "two"}, call, nil); !got.IsError || transport.calls != 0 {
		t.Fatal("authorization bypass")
	}
	bad := call
	bad.Arguments = json.RawMessage(`[]`)
	if got := r.Call(context.Background(), scope, bad, nil); !got.IsError || transport.calls != 0 {
		t.Fatal("schema bypass")
	}
	progress := 0
	if got := r.Call(context.Background(), scope, call, func(json.RawMessage) { progress++ }); got.IsError {
		t.Fatal(got)
	}
	if transport.calls != 1 || progress != 1 || transport.deadline.IsZero() || time.Until(transport.deadline) > time.Second {
		t.Fatal("deadline or progress lost")
	}
	if transport.request.Tenant != "one" || transport.request.Session != "session" || transport.request.Call != "call" || transport.request.InspectionToken != "" || transport.request.BrainURL != "" {
		t.Fatalf("scope: %+v", transport.request)
	}
	transport.result.Content[0].Text = strings.Repeat("x", 300)
	if got := r.Call(context.Background(), scope, call, nil); !got.IsError {
		t.Fatal("output cap bypass")
	}
	transport.result.Content = []core.Content{{Type: "audio"}}
	if got := r.Call(context.Background(), scope, call, nil); !got.IsError {
		t.Fatal("content validation bypass")
	}
	transport.wait = true
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if got := r.Call(ctx, scope, call, nil); !got.IsError || !got.UnknownOutcome {
		t.Fatal("cancellation lost")
	}
	if transport.calls != 4 {
		t.Fatalf("unexpected retry: %d", transport.calls)
	}
	r.Close()
	if transport.closed != 1 {
		t.Fatal("transport not closed")
	}
	b.Transport = "missing"
	if _, err := NewWithTransports([]Binding{b}, map[string]Transport{"custom": transport}); err == nil {
		t.Fatal("unknown transport accepted")
	}
}

func TestTransportFailureKeepsOutcomeOutsideSuccessSchema(t *testing.T) {
	transport := &recordingTransport{result: core.ToolError("connection refused before dispatch", false)}
	b := Binding{Tool: core.Tool{Name: "echo", Description: "Echo", InputSchema: json.RawMessage(`{"type":"object"}`), OutputSchema: json.RawMessage(`{"type":"object","properties":{"is_error":{"const":false}}}`)}, URL: "https://tools.example.test", Transport: "custom", Tenants: []string{"one"}}
	r, err := NewWithTransports([]Binding{b}, map[string]Transport{"custom": transport})
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	got := r.Call(context.Background(), core.ToolScope{Tenant: "one"}, core.Call{Name: "echo", Arguments: json.RawMessage(`{}`)}, nil)
	if !got.IsError || got.UnknownOutcome || got.Content[0].Text != transport.result.Content[0].Text {
		t.Fatalf("transport failure was replaced by a schema error: %+v", got)
	}
}
