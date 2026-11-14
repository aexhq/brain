// Package integrations supplies optional managed provider constructors. Resolver
// functions and endpoint allowlists belong to trusted application code.
package integrations

import (
	"context"
	"encoding/json"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/tools/registry"
)

type Endpoint struct {
	URL   string
	Token string
	Stdio *Stdio
}
type Stdio = tools.StdioConfig

type ProviderOptions struct {
	Parameters json.RawMessage
	Authorize  func(context.Context, tools.BindingContext) error
	// Resolve must allowlist the endpoint/resource and choose deployment defaults
	// only when Credential is absent. Never fall back after a rejected override.
	Resolve func(context.Context, tools.BindingContext) (Endpoint, error)
}

// HTTP binds reviewed descriptors to an application-authorized HTTP/SSE service.
func HTTP(o ProviderOptions, descriptors []core.Tool) tools.Provider {
	return tools.Provider{Parameters: o.Parameters, Authorize: o.Authorize, Bind: func(ctx context.Context, b tools.BindingContext) ([]tools.Binding, error) {
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
		result := make([]tools.Binding, 0, len(descriptors))
		for _, tool := range descriptors {
			result = append(result, tools.Binding{Tool: tool, URL: endpoint.URL, Credential: endpoint.Token, Tenants: []string{b.Tenant}})
		}
		return result, nil
	}}
}

// MCP explicitly discovers installed remote names. Frozen session descriptors
// make later catalog drift fail before an action. Stdio trees are call-owned.
func MCP(o ProviderOptions, names []string) tools.Provider {
	return tools.Provider{Parameters: o.Parameters, Authorize: o.Authorize, Bind: func(ctx context.Context, b tools.BindingContext) ([]tools.Binding, error) {
		if o.Resolve == nil {
			return nil, errors.New("endpoint resolver is required")
		}
		endpoint, err := o.Resolve(ctx, b)
		if err != nil {
			return nil, err
		}
		return tools.DiscoverMCP(ctx, tools.Binding{URL: endpoint.URL, Credential: endpoint.Token, Stdio: endpoint.Stdio, Tenants: []string{b.Tenant}}, names)
	}}
}
