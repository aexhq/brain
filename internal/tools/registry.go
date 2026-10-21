// Package tools validates and transports calls. It never retries a tool action.
package tools

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"os"
	"regexp"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

type Handler func(context.Context, Request, func(json.RawMessage)) core.Result

type Binding struct {
	Handler    Handler         `json:"-"`
	Credential string          `json:"-"`
	Context    json.RawMessage `json:"-"`
	Stdio      *StdioConfig    `json:"-"`
	core.Tool
	Transport      string   `json:"transport,omitempty"`
	URL            string   `json:"url"`
	MCPTool        string   `json:"mcp_tool,omitempty"`
	TokenEnv       string   `json:"token_env,omitempty"`
	Tenants        []string `json:"tenants"`
	TimeoutSeconds int      `json:"timeout_seconds,omitempty"`
	MaxOutputBytes int64    `json:"max_output_bytes,omitempty"`
	InspectSession bool     `json:"inspect_session,omitempty"`
}

type compiled struct {
	binding       Binding
	input, output *jsonschema.Schema
	transport     Transport
}
type Registry struct {
	tools      map[string]compiled
	transports map[string]Transport
	Providers  map[string]Provider
	Keys       credentials.Keys
	Runtime    RuntimeProvider
}
type Request struct {
	core.ToolScope
	Arguments json.RawMessage `json:"arguments"`
}

type noLoader struct{}

func (noLoader) Load(s string) (any, error) {
	return nil, fmt.Errorf("external schema references are disabled: %s", s)
}

func compile(raw json.RawMessage) (*jsonschema.Schema, error) {
	if len(raw) == 0 {
		return nil, nil
	}
	var data any
	if err := json.Unmarshal(raw, &data); err != nil {
		return nil, err
	}
	c := jsonschema.NewCompiler()
	c.UseLoader(noLoader{})
	c.AssertFormat()
	if err := c.AddResource("https://brain.invalid/schema", data); err != nil {
		return nil, err
	}
	return c.Compile("https://brain.invalid/schema")
}

// Transport executes a single authorized, validated call. It must honor ctx,
// bound protocol reads, preserve outcome ambiguity, and never retry an action.
// The registry owns transports after NewWithTransports succeeds.
type Transport interface {
	Call(context.Context, Binding, Request, func(json.RawMessage)) core.Result
	Close()
}

func New(bindings []Binding) (*Registry, error) {
	transports := map[string]Transport{"http": NewHTTPTransport(), "mcp": NewMCPTransport()}
	r, err := NewWithTransports(bindings, transports)
	if err != nil {
		for _, t := range transports {
			t.Close()
		}
	}
	return r, err
}

// NewWithTransports binds transports once at startup. Callers must not mutate
// the supplied adapters while the registry is in use.
func NewWithTransports(bindings []Binding, transports map[string]Transport) (*Registry, error) {
	r := &Registry{tools: make(map[string]compiled), transports: make(map[string]Transport)}
	for name, transport := range transports {
		r.transports[name] = transport
	}
	namePattern := regexp.MustCompile(`^[a-zA-Z][a-zA-Z0-9_]{0,63}$`)
	for _, b := range bindings {
		if !namePattern.MatchString(b.Name) || b.Description == "" || len(b.InputSchema) == 0 || len(b.Tenants) == 0 {
			return nil, fmt.Errorf("invalid tool descriptor %q", b.Name)
		}
		if _, ok := r.tools[b.Name]; ok {
			return nil, fmt.Errorf("duplicate tool %q", b.Name)
		}
		if b.Transport == "" {
			b.Transport = "http"
			if b.MCPTool != "" {
				b.Transport = "mcp"
			}
		}
		transport := r.transports[b.Transport]
		if b.Handler != nil {
			transport = handlerTransport{b.Handler}
		}
		if transport == nil {
			return nil, fmt.Errorf("unknown tool transport %q", b.Transport)
		}
		if (b.Transport == "mcp") != (b.MCPTool != "") {
			return nil, fmt.Errorf("mcp_tool is required only for MCP transport")
		}
		u, err := url.Parse(b.URL)
		if b.Stdio != nil && (b.Transport != "mcp" || b.URL != "" || len(b.Stdio.Command) == 0) {
			return nil, errors.New("stdio requires an installed MCP command and no URL")
		}
		if b.Handler == nil && b.Stdio == nil && (err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil || u.Fragment != "") {
			return nil, fmt.Errorf("invalid tool URL for %s", b.Name)
		}
		if b.TokenEnv != "" && os.Getenv(b.TokenEnv) == "" {
			return nil, fmt.Errorf("missing tool credential %s", b.TokenEnv)
		}
		if b.MCPTool != "" && b.InspectSession {
			return nil, fmt.Errorf("MCP tools cannot receive session inspection credentials")
		}
		if b.TimeoutSeconds == 0 {
			b.TimeoutSeconds = 300
		}
		if b.MaxOutputBytes == 0 {
			b.MaxOutputBytes = 1 << 20
		}
		if b.TimeoutSeconds < 1 || b.TimeoutSeconds > 86400 || b.MaxOutputBytes < 1 || b.MaxOutputBytes > 2<<20 {
			return nil, fmt.Errorf("invalid limits for tool %s", b.Name)
		}
		input, err := compile(b.InputSchema)
		if err != nil {
			return nil, fmt.Errorf("tool %s input: %w", b.Name, err)
		}
		output, err := compile(b.OutputSchema)
		if err != nil {
			return nil, fmt.Errorf("tool %s output: %w", b.Name, err)
		}
		r.tools[b.Name] = compiled{b, input, output, transport}
	}
	return r, nil
}

func allowed(b Binding, tenant string) bool {
	for _, t := range b.Tenants {
		if t == tenant {
			return true
		}
	}
	return false
}
func (r *Registry) Catalog(tenant string) []core.Tool {
	result := []core.Tool{}
	for _, t := range r.tools {
		if allowed(t.binding, tenant) {
			result = append(result, t.binding.Tool)
		}
	}
	return result
}
func (r *Registry) Select(tenant string, names []string) ([]core.Tool, error) {
	result := make([]core.Tool, 0, len(names))
	seen := make(map[string]bool)
	for _, name := range names {
		t, ok := r.tools[name]
		if !ok || !allowed(t.binding, tenant) {
			return nil, core.E(core.Forbidden, "tool is not available: "+name)
		}
		if seen[name] {
			return nil, core.E(core.Invalid, "duplicate tool: "+name)
		}
		seen[name] = true
		result = append(result, t.binding.Tool)
	}
	return result, nil
}
func (r *Registry) NeedsInspection(name string) bool { return r.tools[name].binding.InspectSession }
func (r *Registry) Close() {
	for _, t := range r.transports {
		t.Close()
	}
}

func (r *Registry) Call(ctx context.Context, scope core.ToolScope, call core.Call, progress func(json.RawMessage)) core.Result {
	t, ok := r.tools[call.Name]
	if !ok || !allowed(t.binding, scope.Tenant) {
		return core.ToolError("tool is not authorized", false)
	}
	var args any
	if err := json.Unmarshal(call.Arguments, &args); err != nil {
		return core.ToolError("invalid tool arguments", false)
	}
	if err := t.input.Validate(args); err != nil {
		return core.ToolError("tool input validation failed: "+err.Error(), false)
	}
	ctx, cancel := context.WithTimeout(ctx, time.Duration(t.binding.TimeoutSeconds)*time.Second)
	defer cancel()
	if !t.binding.InspectSession {
		scope.InspectionToken = ""
		scope.BrainURL = ""
	}
	result := t.transport.Call(ctx, t.binding, Request{scope, call.Arguments}, progress)
	if len(result.Content) == 0 && len(result.StructuredContent) > 0 {
		result.Content = []core.Content{{Type: "text", Text: string(result.StructuredContent)}}
	}
	data, err := json.Marshal(result)
	if err != nil || int64(len(data)) > t.binding.MaxOutputBytes {
		return core.ToolError("tool output limit exceeded", true)
	}
	// A failed call cannot satisfy a success schema. Preserve its original
	// outcome classification while still enforcing content and size bounds.
	schema := t.output
	if result.IsError {
		schema = nil
	}
	if err := decodeResult(data, &result, schema); err != nil {
		return core.ToolError("tool output validation failed", true)
	}
	return result
}

func decodeResult(data []byte, result *core.Result, schema *jsonschema.Schema) error {
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if err := d.Decode(result); err != nil {
		return err
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing result data")
	}
	if len(result.Content) == 0 && len(result.StructuredContent) == 0 {
		return errors.New("tool result requires content")
	}
	for _, c := range result.Content {
		if c.Type != "text" && c.Type != "image" {
			return errors.New("unsupported content type")
		}
		if c.Type == "image" && (c.Data == "" || c.MimeType == "") {
			return errors.New("incomplete image content")
		}
	}
	if schema != nil {
		var value any
		if err := json.Unmarshal(result.StructuredContent, &value); err != nil {
			return err
		}
		return schema.Validate(value)
	}
	return nil
}
