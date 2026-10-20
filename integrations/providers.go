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
