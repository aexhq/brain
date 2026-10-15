// Package tools validates and transports calls. It never retries a tool action.
package tools

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"mime"
	"net/http"
	"net/url"
	"os"
	"regexp"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/santhosh-tekuri/jsonschema/v6"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
)

type Binding struct {
	core.Tool
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
}
type Registry struct {
	tools  map[string]compiled
	client *http.Client
}
type Scope struct {
	Tenant          string `json:"tenant_id"`
	Session         string `json:"session_id"`
	Call            string `json:"call_id"`
	InspectionToken string `json:"inspection_token,omitempty"`
	BrainURL        string `json:"brain_url,omitempty"`
}
type Request struct {
	Scope
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

func New(bindings []Binding) (*Registry, error) {
	r := &Registry{tools: make(map[string]compiled), client: &http.Client{Transport: &http.Transport{Proxy: http.ProxyFromEnvironment, MaxIdleConns: 256, MaxIdleConnsPerHost: 32, IdleConnTimeout: 90 * time.Second, ResponseHeaderTimeout: 30 * time.Second}, CheckRedirect: func(*http.Request, []*http.Request) error { return errors.New("tool redirects are disabled") }}}
	namePattern := regexp.MustCompile(`^[a-zA-Z][a-zA-Z0-9_]{0,63}$`)
	for _, b := range bindings {
		if !namePattern.MatchString(b.Name) || b.Description == "" || len(b.InputSchema) == 0 || len(b.Tenants) == 0 {
			return nil, fmt.Errorf("invalid tool descriptor %q", b.Name)
		}
		if _, ok := r.tools[b.Name]; ok {
			return nil, fmt.Errorf("duplicate tool %q", b.Name)
		}
		u, err := url.Parse(b.URL)
		if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil || u.Fragment != "" {
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
		r.tools[b.Name] = compiled{b, input, output}
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
func (r *Registry) Close()                           { r.client.CloseIdleConnections() }

func (r *Registry) Call(ctx context.Context, scope Scope, call core.Call, progress func(json.RawMessage)) core.Result {
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
	if t.binding.MCPTool != "" {
		return r.callMCP(ctx, scope, t, call, progress)
	}
	if !t.binding.InspectSession {
		scope.InspectionToken = ""
		scope.BrainURL = ""
	}
	body, err := json.Marshal(Request{scope, call.Arguments})
	if err != nil {
		return core.ToolError("invalid tool request", false)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, t.binding.URL, bytes.NewReader(body))
	if err != nil {
		return core.ToolError("invalid tool request", false)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json, text/event-stream")
	otel.GetTextMapPropagator().Inject(ctx, propagation.HeaderCarrier(req.Header))
	if t.binding.TokenEnv != "" {
		token := os.Getenv(t.binding.TokenEnv)
		if token == "" {
			return core.ToolError("tool credential is unavailable", false)
		}
		req.Header.Set("Authorization", "Bearer "+token)
	}
	res, err := r.client.Do(req)
	if err != nil {
		return core.ToolError("tool transport failed; remote outcome may be unknown", true)
	}
	defer res.Body.Close()
	if res.StatusCode < 200 || res.StatusCode >= 300 {
		return core.ToolError(fmt.Sprintf("tool returned HTTP %d; remote outcome may be unknown", res.StatusCode), true)
	}
	ct, _, err := mime.ParseMediaType(res.Header.Get("Content-Type"))
	if err != nil {
		return core.ToolError("tool returned an invalid content type", true)
	}
	reader := &io.LimitedReader{R: res.Body, N: t.binding.MaxOutputBytes + 1}
	var result core.Result
	if ct == "text/event-stream" {
		result, err = readSSE(reader, progress)
	} else if ct == "application/json" {
		data, e := io.ReadAll(reader)
		err = e
		if err == nil {
			err = decodeResult(data, &result, t.output)
		}
	} else {
		return core.ToolError("unsupported tool response content type", true)
	}
	if reader.N == 0 {
		return core.ToolError("tool output limit exceeded", true)
	}
	if err != nil {
		return core.ToolError("invalid or incomplete tool response: "+err.Error(), true)
	}
	if ct == "text/event-stream" && t.output != nil {
		data, _ := json.Marshal(result)
		if err := decodeResult(data, &result, t.output); err != nil {
			return core.ToolError("tool output validation failed", true)
		}
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
	if len(result.Content) == 0 {
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
		if err := json.Unmarshal(data, &value); err != nil {
			return err
		}
		return schema.Validate(value)
	}
	return nil
}

func readSSE(r io.Reader, progress func(json.RawMessage)) (core.Result, error) {
	s := bufio.NewScanner(r)
	s.Buffer(make([]byte, 4096), 2<<20)
	var event string
	var data []string
	for s.Scan() {
		line := s.Text()
		if line == "" {
			if len(data) > 0 {
				payload := []byte(strings.Join(data, "\n"))
				switch event {
				case "result":
					var result core.Result
					err := decodeResult(payload, &result, nil)
					return result, err
				case "progress":
					if !json.Valid(payload) {
						return core.Result{}, errors.New("invalid progress JSON")
					}
					if progress != nil {
						progress(payload)
					}
				default:
					return core.Result{}, errors.New("unsupported tool stream event")
				}
			}
			event = ""
			data = nil
			continue
		}
		if strings.HasPrefix(line, ":") {
			continue
		}
		field, value, found := strings.Cut(line, ":")
		if !found {
			value = ""
		}
		value = strings.TrimPrefix(value, " ")
		if field == "event" {
			event = value
		}
		if field == "data" {
			data = append(data, value)
		}
	}
	if err := s.Err(); err != nil {
		return core.Result{}, err
	}
	return core.Result{}, errors.New("stream ended without terminal result")
}
