// Package integrations supplies optional managed provider constructors. Resolver
// functions and endpoint allowlists belong to trusted application code.
package integrations

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain"
	"github.com/aexhq/brain/internal/tools"
)

type Endpoint struct {
	URL   string
	Token string
	Stdio *Stdio
}
type Stdio = tools.StdioConfig

type ProviderOptions struct {
	Parameters json.RawMessage
	Authorize  func(context.Context, brain.BindingContext) error
	// Resolve must allowlist the endpoint/resource and choose deployment defaults
	// only when Credential is absent. Never fall back after a rejected override.
	Resolve func(context.Context, brain.BindingContext) (Endpoint, error)
}

// HTTP binds reviewed descriptors to an application-authorized HTTP/SSE service.
func HTTP(o ProviderOptions, descriptors []brain.Tool) brain.ToolProvider {
	return brain.ToolProvider{Parameters: o.Parameters, Authorize: o.Authorize, Bind: func(ctx context.Context, b brain.BindingContext) ([]brain.ManagedTool, error) {
		if o.Resolve == nil {
			return nil, errors.New("endpoint resolver is required")
		}
		endpoint, err := o.Resolve(ctx, b)
		if err != nil {
			return nil, err
		}
		if endpoint.Stdio != nil {
			return nil, errors.New("HTTP provider cannot use stdio")
		}
		result := make([]brain.ManagedTool, 0, len(descriptors))
		for _, tool := range descriptors {
			result = append(result, brain.ManagedTool{Tool: tool, URL: endpoint.URL, Credential: endpoint.Token, Tenants: []string{b.Tenant}})
		}
		return result, nil
	}}
}

// MCP explicitly discovers installed remote names. Frozen session descriptors
// make later catalog drift fail before an action. Stdio trees are call-owned.
func MCP(o ProviderOptions, names []string) brain.ToolProvider {
	return brain.ToolProvider{Parameters: o.Parameters, Authorize: o.Authorize, Bind: func(ctx context.Context, b brain.BindingContext) ([]brain.ManagedTool, error) {
		if o.Resolve == nil {
			return nil, errors.New("endpoint resolver is required")
		}
		endpoint, err := o.Resolve(ctx, b)
		if err != nil {
			return nil, err
		}
		return tools.DiscoverMCP(ctx, brain.ManagedTool{URL: endpoint.URL, Credential: endpoint.Token, Stdio: endpoint.Stdio, Tenants: []string{b.Tenant}}, names)
	}}
}

type SessionReader interface {
	ReadSession(context.Context, string, string) (brain.Session, error)
	ReadEvents(context.Context, string, string, string, int) ([]brain.Event, error)
}

// Sessions grants only explicit, application-authorized target reads. The
// caller's tenant always comes from the authenticated binding, never arguments.
func Sessions(reader SessionReader, authorize func(context.Context, brain.BindingContext, string) error) brain.ToolProvider {
	return brain.ToolProvider{Parameters: json.RawMessage(`{"type":"object","additionalProperties":false}`), Authorize: func(context.Context, brain.BindingContext) error {
		if reader == nil || authorize == nil {
			return errors.New("session reader and target authorization are required")
		}
		return nil
	}, Bind: func(ctx context.Context, b brain.BindingContext) ([]brain.ManagedTool, error) {
		return []brain.ManagedTool{{Tool: brain.Tool{Name: "read", Description: "Read an authorized session and a bounded page of its public events", InputSchema: json.RawMessage(`{"type":"object","properties":{"session":{"type":"string","pattern":"^[a-f0-9]{32}$"},"after":{"type":"string"}},"required":["session"],"additionalProperties":false}`)}, Handler: func(ctx context.Context, r brain.ToolRequest, progress func(json.RawMessage)) brain.Result {
			var input struct {
				Session string `json:"session"`
				After   string `json:"after"`
			}
			if err := json.Unmarshal(r.Arguments, &input); err != nil {
				return brain.ToolError("invalid session read", false)
			}
			if err := authorize(ctx, b, input.Session); err != nil {
				return brain.ToolError("target session is not authorized", false)
			}
			session, err := reader.ReadSession(ctx, b.Tenant, input.Session)
			if err != nil {
				return brain.ToolError("target session is unavailable", false)
			}
			events, err := reader.ReadEvents(ctx, b.Tenant, input.Session, input.After, 100)
			if err != nil {
				return brain.ToolError("target history is unavailable", false)
			}
			data, err := json.Marshal(struct {
				Session brain.Session `json:"session"`
				Events  []brain.Event `json:"events"`
			}{session, events})
			if err != nil {
				return brain.ToolError("session read failed", false)
			}
			return brain.Result{Content: []brain.Content{{Type: "text", Text: string(data)}}, StructuredContent: data}
		}}}, nil
	}}
}
