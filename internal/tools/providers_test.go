package tools

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain/internal/core"
	"testing"
)

func TestIndependentProviderBindingsAndFrozenDescriptors(t *testing.T) {
	r, err := New(nil)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	description := "Read the bound resource"
	calls := 0
	r.Providers = map[string]Provider{"data": {
		Parameters: json.RawMessage(`{"type":"string","enum":["first","second"]}`),
		Authorize: func(_ context.Context, b BindingContext) error {
			if b.Tenant != "one" && b.Tenant != "two" {
				return errors.New("denied")
			}
			return nil
		},
		Bind: func(_ context.Context, b BindingContext) ([]Binding, error) {
			return []Binding{{Tool: core.Tool{Name: "read", Description: description, InputSchema: json.RawMessage(`{"type":"object"}`), OutputSchema: json.RawMessage(`{"type":"string"}`)}, Handler: func(_ context.Context, r Request, _ func(json.RawMessage)) core.Result {
				calls++
				if r.Tenant != b.Tenant || r.Session != b.Session {
					t.Error("scope crossed bindings")
				}
				return core.Result{StructuredContent: b.Parameters}
			}}}, nil
		},
	}}
	for _, tenant := range []string{"one", "two"} {
		s := core.Session{ID: core.ID(), Tenant: tenant, Config: core.Config{Bindings: []core.ToolBinding{{Alias: "a", Provider: "data", Operations: []string{"read"}, Parameters: json.RawMessage(`"first"`)}, {Alias: "b", Provider: "data", Operations: []string{"read"}, Parameters: json.RawMessage(`"second"`)}}}}
		if err := r.Bind(context.Background(), &s); err != nil {
			t.Fatal(err)
		}
		if len(s.Descriptors) != 2 || s.Descriptors[0].Name != "a_read" || s.Descriptors[1].Name != "b_read" {
			t.Fatal(s.Descriptors)
		}
		for i, name := range []string{"a_read", "b_read"} {
			got, err := r.Invoke(context.Background(), s, core.ToolScope{Tenant: tenant, Session: s.ID}, core.Call{Name: name, Arguments: json.RawMessage(`{}`)}, nil)
			if err != nil || got.IsError || string(got.StructuredContent) != string(s.Config.Bindings[i].Parameters) {
				t.Fatalf("binding result: %+v %v", got, err)
			}
		}
		description = "changed descriptor"
		if _, err := r.Describe(context.Background(), s); core.ErrorCode(err) != core.Conflict {
			t.Fatalf("catalog change accepted: %v", err)
		}
		description = "Read the bound resource"
		s.Config.Bindings[1].Alias = "a"
		if err := r.Bind(context.Background(), &s); core.ErrorCode(err) != core.Invalid {
			t.Fatalf("alias collision: %v", err)
		}
	}
	if calls != 4 {
		t.Fatalf("actions ran during binding or were retried: %d", calls)
	}
}

func TestOutputSchemaValidatesStructuredSuccess(t *testing.T) {
	result := core.Result{Content: []core.Content{{Type: "text", Text: "display"}}, StructuredContent: json.RawMessage(`{"count":3}`)}
	r, err := New([]Binding{{Tool: core.Tool{Name: "count", Description: "Count", InputSchema: json.RawMessage(`{"type":"object"}`), OutputSchema: json.RawMessage(`{"type":"object","required":["count"],"properties":{"count":{"type":"integer"}}}`)}, Tenants: []string{"one"}, Handler: func(context.Context, Request, func(json.RawMessage)) core.Result { return result }}})
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	call := func() core.Result {
		return r.Call(context.Background(), core.ToolScope{Tenant: "one"}, core.Call{Name: "count", Arguments: json.RawMessage(`{}`)}, nil)
	}
	if got := call(); got.IsError {
		t.Fatal(got)
	}
	result.StructuredContent = json.RawMessage(`{"count":"bad"}`)
	if got := call(); !got.IsError || !got.UnknownOutcome {
		t.Fatal(got)
	}
	result = core.ToolError("denied", false)
	if got := call(); !got.IsError || got.UnknownOutcome {
		t.Fatal(got)
	}
}

func TestProviderOutagePreservesCatalogAndDoesNotDispatch(t *testing.T) {
	ctx := context.Background()
	r, err := New(nil)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	down := false
	r.Providers = map[string]Provider{"remote": {Parameters: json.RawMessage(`{"type":"object"}`), Authorize: func(context.Context, BindingContext) error { return nil }, Bind: func(context.Context, BindingContext) ([]Binding, error) {
		if down {
			return nil, &Unavailable{Message: "remote unavailable"}
		}
		return []Binding{{Tool: core.Tool{Name: "read", Description: "Read", InputSchema: json.RawMessage(`{"type":"object"}`)}, Handler: func(context.Context, Request, func(json.RawMessage)) core.Result {
			t.Fatal("action dispatched")
			return core.Result{}
		}}}, nil
	}}}
	s := core.Session{ID: core.ID(), Tenant: "a", Config: core.Config{Bindings: []core.ToolBinding{{Alias: "data", Provider: "remote", Operations: []string{"read"}}}}}
	if err := r.Bind(ctx, &s); err != nil {
		t.Fatal(err)
	}
	down = true
	if tools, err := r.Describe(ctx, s); err != nil || len(tools) != 1 || tools[0].Name != "data_read" {
		t.Fatalf("lost catalog: %v %v", tools, err)
	}
	result, err := r.Invoke(ctx, s, core.ToolScope{Tenant: "a", Session: s.ID}, core.Call{Name: "data_read", Arguments: json.RawMessage(`{}`)}, nil)
	if err != nil || !result.IsError || result.UnknownOutcome {
		t.Fatalf("outage: %+v %v", result, err)
	}
}
