package tools

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"

	"github.com/aexhq/brain/internal/core"
	"github.com/modelcontextprotocol/go-sdk/mcp"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
)

// MCPTransport implements isolated Streamable HTTP sessions.
type MCPTransport struct{ client *http.Client }

func NewMCPTransport() *MCPTransport { return &MCPTransport{client: newHTTPClient()} }

func (t *MCPTransport) Close() { t.client.CloseIdleConnections() }

// Each invocation has its own MCP session: server-side session state and
// progress cannot cross tenant/call boundaries. The HTTP connection pool is shared.
func (t *MCPTransport) connect(ctx context.Context, b Binding, progress func(json.RawMessage)) (*mcp.ClientSession, func(), error) {
	client := mcp.NewClient(&mcp.Implementation{Name: "brain", Version: "dev"}, &mcp.ClientOptions{
		Capabilities:   &mcp.ClientCapabilities{},
		MultiRoundTrip: &mcp.MultiRoundTripOptions{Disabled: true},
		ProgressNotificationHandler: func(_ context.Context, req *mcp.ProgressNotificationClientRequest) {
			if progress != nil {
				data, err := json.Marshal(req.Params)
				if err == nil {
					progress(data)
				}
			}
		},
	})
	if b.Stdio != nil {
		return connectStdio(ctx, client, b)
	}
	transport := &mcp.StreamableClientTransport{Endpoint: b.URL, MaxRetries: -1, DisableStandaloneSSE: true,
		MaxEventSize: int(b.MaxOutputBytes + 65536),
		HTTPClient:   &http.Client{Transport: mcpHTTP{base: t.client.Transport, binding: b}, CheckRedirect: t.client.CheckRedirect},
	}
	session, err := client.Connect(ctx, transport, nil)
	if err != nil {
		return nil, nil, err
	}
	return session, func() { _ = session.Close() }, nil
}

type mcpHTTP struct {
	base    http.RoundTripper
	binding Binding
}

func (t mcpHTTP) RoundTrip(req *http.Request) (*http.Response, error) {
	req = req.Clone(req.Context())
	if t.binding.Credential != "" {
		req.Header.Set("Authorization", "Bearer "+t.binding.Credential)
	} else if t.binding.TokenEnv != "" {
		key := os.Getenv(t.binding.TokenEnv)
		if key == "" {
			return nil, errors.New("MCP credential is unavailable")
		}
		req.Header.Set("Authorization", "Bearer "+key)
	}
	otel.GetTextMapPropagator().Inject(req.Context(), propagation.HeaderCarrier(req.Header))
	res, err := t.base.RoundTrip(req)
	if err == nil {
		// Bound JSON as well as SSE, including framing and protocol metadata.
		res.Body = &boundedMCPBody{ReadCloser: res.Body, remaining: t.binding.MaxOutputBytes + 65536}
	}
	return res, err
}

type boundedMCPBody struct {
	io.ReadCloser
	remaining int64
}

func (b *boundedMCPBody) Read(p []byte) (int, error) {
	if b.remaining == 0 {
		return 0, errors.New("MCP response exceeds limit")
	}
	if int64(len(p)) > b.remaining {
		p = p[:b.remaining]
	}
	n, err := b.ReadCloser.Read(p)
	b.remaining -= int64(n)
	return n, err
}

func (t *MCPTransport) Call(ctx context.Context, binding Binding, request Request, progress func(json.RawMessage)) core.Result {
	scope := request.ToolScope
	session, close, err := t.connect(ctx, binding, progress)
	if err != nil {
		return core.ToolError("MCP connection failed before tool dispatch", false)
	}
	defer close()
	params := &mcp.CallToolParams{Name: binding.MCPTool, Arguments: request.Arguments,
		Meta: mcp.Meta{"brain.dev/scope": map[string]string{"tenant_id": scope.Tenant, "session_id": scope.Session, "call_id": scope.Call}},
	}
	params.SetProgressToken(scope.Call)
	out, err := session.CallTool(ctx, params)
	if err != nil {
		return core.ToolError("MCP call failed; remote outcome may be unknown", true)
	}
	if out.NeedsInput() {
		return core.ToolError("MCP input requests are not supported; implement approval in the tool", true)
	}
	result := core.Result{IsError: out.IsError, Content: []core.Content{}}
	for _, content := range out.Content {
		switch c := content.(type) {
		case *mcp.TextContent:
			result.Content = append(result.Content, core.Content{Type: "text", Text: c.Text})
		case *mcp.ImageContent:
			result.Content = append(result.Content, core.Content{Type: "image", Data: base64.StdEncoding.EncodeToString(c.Data), MimeType: c.MIMEType})
		default:
			return core.ToolError("unsupported MCP content type", true)
		}
	}
	if out.StructuredContent != nil {
		data, err := json.Marshal(out.StructuredContent)
		if err != nil {
			return core.ToolError("invalid MCP structured content", true)
		}
		result.StructuredContent = data
		result.Content = append(result.Content, core.Content{Type: "text", Text: string(data)})
	}
	return result
}

// DiscoverMCP produces reviewable, static bindings. Discovery never changes the
// tools exposed to a running session. Operators select the remote names explicitly.
func DiscoverMCP(ctx context.Context, binding Binding, names []string) ([]Binding, error) {
	if len(names) == 0 || len(names) > 128 {
		return nil, errors.New("select 1 to 128 MCP tools")
	}
	binding.MCPTool = "discovery"
	binding.Transport = "mcp"
	binding.Tool = core.Tool{Name: "discovery", Description: "MCP discovery", InputSchema: json.RawMessage(`{"type":"object"}`)}
	registry, err := New([]Binding{binding})
	if err != nil {
		return nil, err
	}
	defer registry.Close()
	binding = registry.tools["discovery"].binding
	transport := registry.transports["mcp"].(*MCPTransport)
	session, close, err := transport.connect(ctx, binding, nil)
	if err != nil {
		return nil, &Unavailable{Message: "MCP discovery connection is unavailable"}
	}
	defer close()
	selected := make(map[string]bool)
	for _, name := range names {
		if selected[name] {
			return nil, fmt.Errorf("duplicate MCP tool %q", name)
		}
		selected[name] = true
	}
	result := []Binding{}
	count := 0
	for tool, err := range session.Tools(ctx, nil) {
		if err != nil {
			return nil, &Unavailable{Message: "MCP catalog is unavailable"}
		}
		count++
		if count > 4096 {
			return nil, errors.New("MCP catalog exceeds limit")
		}
		if !selected[tool.Name] {
			continue
		}
		schema, err := json.Marshal(tool.InputSchema)
		if err != nil {
			return nil, err
		}
		b := binding
		b.Tool = core.Tool{Name: tool.Name, Description: tool.Description, InputSchema: schema}
		if tool.OutputSchema != nil {
			b.OutputSchema, err = json.Marshal(tool.OutputSchema)
			if err != nil {
				return nil, err
			}
		}
		b.MCPTool = tool.Name
		result = append(result, b)
		delete(selected, tool.Name)
	}
	if len(selected) != 0 {
		return nil, errors.New("selected MCP tool is missing")
	}
	check, err := New(result)
	if err != nil {
		return nil, err
	}
	check.Close()
	return result, nil
}
