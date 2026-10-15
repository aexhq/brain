package integrations

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"

	"github.com/aexhq/brain/adapters/models"
	policy "github.com/aexhq/brain/adapters/network"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

type Fetch struct{ Client *http.Client }

func (f *Fetch) Validate(template wire.ConnectionTemplate) error {
	if !slices.Contains(template.Capabilities, "fetch") {
		return kernel.Fail(wire.ErrorCodeForbidden, "web_fetch requires the fetch capability")
	}
	return nil
}
func (f *Fetch) Invoke(ctx context.Context, tenant string, template wire.ConnectionTemplate, execution wire.Execution) (wire.Receipt, error) {
	if err := f.Validate(template); err != nil {
		return wire.Receipt{}, err
	}
	var args struct {
		URL string `json:"url"`
	}
	if err := json.Unmarshal(execution.Arguments, &args); err != nil {
		return wire.Receipt{}, err
	}
	u, err := policy.ValidateURL(args.URL)
	if err != nil {
		return wire.Receipt{}, err
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, u.String(), nil)
	if err != nil {
		return wire.Receipt{}, err
	}
	request.Header.Set("User-Agent", "Brain-web-fetch/2")
	client := f.Client
	if client == nil {
		client = policy.Client(30 * time.Second)
	}
	response, err := client.Do(request)
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeFailed, Error: kernel.Ptr("web destination denied or unavailable")}, nil
	}
	body, err := policy.Body(response, int64(execution.MaxOutputBytes)/2)
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeCompleted, Error: kernel.Ptr("web response completed but its body is unavailable within the admitted limits")}, nil
	}
	return wire.Receipt{Outcome: wire.OutcomeCompleted, Result: kernel.JSON(map[string]any{"url": response.Request.URL.String(), "status": response.StatusCode, "media_type": response.Header.Get("Content-Type"), "text": string(body)})}, nil
}

type Search struct {
	Provider string
	Resolve  models.Resolver
	Client   *http.Client
	Endpoint string
}

func (s *Search) Validate(template wire.ConnectionTemplate) error {
	if !slices.Contains(template.Capabilities, "search") || template.Secrets["api_key"] == "" {
		return kernel.Fail(wire.ErrorCodeForbidden, "search requires its capability and scoped provider key")
	}
	if !slices.Contains([]string{"brave", "tavily", "exa"}, s.Provider) {
		return kernel.Fail(wire.ErrorCodeUnsupported, "search provider is not implemented")
	}
	return nil
}

func (s *Search) Invoke(ctx context.Context, tenant string, template wire.ConnectionTemplate, execution wire.Execution) (wire.Receipt, error) {
	if err := s.Validate(template); err != nil {
		return wire.Receipt{}, err
	}
	var args struct {
		Query   string                     `json:"query"`
		Count   int                        `json:"count"`
		Options map[string]json.RawMessage `json:"options"`
	}
	if err := json.Unmarshal(execution.Arguments, &args); err != nil {
		return wire.Receipt{}, err
	}
	if len(args.Query) < 1 || len(args.Query) > 4096 {
		return wire.Receipt{}, kernel.Fail(wire.ErrorCodeInvalid, "search query must be between one and 4096 bytes")
	}
	if args.Count == 0 {
		args.Count = 5
	}
	if args.Count < 1 || args.Count > 20 {
		return wire.Receipt{}, kernel.Fail(wire.ErrorCodeInvalid, "search count must be between one and twenty")
	}
	key, err := s.Resolve(ctx, tenant, template.Secrets["api_key"])
	if err != nil {
		return wire.Receipt{}, err
	}
	target := s.Endpoint
	method := http.MethodPost
	payload := map[string]any{}
	headers := map[string]string{"Content-Type": "application/json"}
	switch s.Provider {
	case "brave":
		if target == "" {
			target = "https://api.search.brave.com/res/v1/web/search"
		}
		u, err := url.Parse(target)
		if err != nil {
			return wire.Receipt{}, err
		}
		query := u.Query()
		query.Set("q", args.Query)
		query.Set("count", fmt.Sprint(args.Count))
		for name, value := range args.Options {
			if !slices.Contains([]string{"country", "search_lang", "safesearch", "freshness"}, name) {
				return wire.Receipt{}, kernel.Fail(wire.ErrorCodeUnsupported, "unsupported Brave option")
			}
			var text string
			if json.Unmarshal(value, &text) != nil {
				return wire.Receipt{}, kernel.Fail(wire.ErrorCodeInvalid, "Brave option must be a string")
			}
			query.Set(name, text)
		}
		u.RawQuery = query.Encode()
		target = u.String()
		method = http.MethodGet
		headers["X-Subscription-Token"] = key
	case "tavily":
		if target == "" {
			target = "https://api.tavily.com/search"
		}
		payload = map[string]any{"query": args.Query, "max_results": args.Count, "api_key": key}
		for name, value := range args.Options {
			if !slices.Contains([]string{"search_depth", "topic", "include_domains", "exclude_domains"}, name) {
				return wire.Receipt{}, kernel.Fail(wire.ErrorCodeUnsupported, "unsupported Tavily option")
			}
			payload[name] = value
		}
	case "exa":
		if target == "" {
			target = "https://api.exa.ai/search"
		}
		payload = map[string]any{"query": args.Query, "numResults": args.Count, "type": "auto", "contents": map[string]bool{"text": true}}
		headers["X-Api-Key"] = key
		for name, value := range args.Options {
			if !slices.Contains([]string{"type", "category", "includeDomains", "excludeDomains", "startPublishedDate"}, name) {
				return wire.Receipt{}, kernel.Fail(wire.ErrorCodeUnsupported, "unsupported Exa option")
			}
			payload[name] = value
		}
	}
	var body []byte
	if method == http.MethodPost {
		body = kernel.JSON(payload)
	}
	request, err := http.NewRequestWithContext(ctx, method, target, bytes.NewReader(body))
	if err != nil {
		return wire.Receipt{}, err
	}
	request.GetBody = nil
	for name, value := range headers {
		request.Header.Set(name, value)
	}
	client := s.Client
	if client == nil {
		client = policy.Client(30 * time.Second)
		client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	}
	response, err := client.Do(request)
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("search request has an uncertain remote outcome")}, nil
	}
	raw, err := policy.Body(response, 2<<20)
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeCompleted, Error: kernel.Ptr("search result exceeded transport limits")}, nil
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return wire.Receipt{Outcome: wire.OutcomeFailed, Error: kernel.Ptr("search provider returned HTTP " + response.Status)}, nil
	}
	var envelope struct {
		Results []map[string]json.RawMessage `json:"results"`
		Web     struct {
			Results []map[string]json.RawMessage `json:"results"`
		} `json:"web"`
	}
	if err = json.Unmarshal(raw, &envelope); err != nil {
		return wire.Receipt{Outcome: wire.OutcomeCompleted, Error: kernel.Ptr("search provider returned invalid JSON")}, nil
	}
	items := envelope.Results
	if s.Provider == "brave" {
		items = envelope.Web.Results
	}
	results := []map[string]any{}
	for _, item := range items[:min(len(items), args.Count)] {
		text := func(names ...string) string {
			for _, name := range names {
				var value string
				if json.Unmarshal(item[name], &value) == nil {
					return value
				}
			}
			return ""
		}
		result := map[string]any{"title": text("title"), "url": text("url"), "snippet": text("description", "content", "text", "summary")}
		if value := text("publishedDate", "published_date"); value != "" {
			result["published"] = value
		}
		results = append(results, result)
	}
	result := kernel.JSON(map[string]any{"provider": s.Provider, "results": results})
	if len(result) > int(execution.MaxOutputBytes) {
		return wire.Receipt{Outcome: wire.OutcomeCompleted, Error: kernel.Ptr("search completed; normalized results exceed the admitted output limit")}, nil
	}
	return wire.Receipt{Outcome: wire.OutcomeCompleted, Result: result}, nil
}

type MCP struct {
	Client      *http.Client
	Resolve     models.Resolver
	Development bool
}

func (m *MCP) Validate(template wire.ConnectionTemplate) error {
	var c endpoint
	if json.Unmarshal(template.Configuration, &c) != nil {
		return kernel.Fail(wire.ErrorCodeInvalid, "MCP endpoint is required")
	}
	if !m.Development {
		if _, err := policy.ValidateURL(c.URL); err != nil {
			return err
		}
	}
	if !slices.Contains(template.Capabilities, "tools") {
		return kernel.Fail(wire.ErrorCodeForbidden, "MCP tools capability is required")
	}
	return nil
}
func (m *MCP) Invoke(ctx context.Context, tenant string, template wire.ConnectionTemplate, execution wire.Execution) (wire.Receipt, error) {
	if err := m.Validate(template); err != nil {
		return wire.Receipt{}, err
	}
	var configuration struct {
		URL  string `json:"url"`
		Tool string `json:"tool"`
	}
	_ = json.Unmarshal(template.Configuration, &configuration)
	tool := configuration.Tool
	if tool == "" {
		tool = execution.Operation.Action
	}
	client := m.Client
	if client == nil {
		client = policy.Client(30 * time.Second)
		client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	}
	sessionID := ""
	protocolVersion := "2025-11-25"
	call := func(id string, method string, params any) (json.RawMessage, error) {
		payload := map[string]any{"jsonrpc": "2.0", "method": method, "params": params}
		if id != "" {
			payload["id"] = id
		}
		request, err := http.NewRequestWithContext(ctx, http.MethodPost, configuration.URL, bytes.NewReader(kernel.JSON(payload)))
		if err != nil {
			return nil, err
		}
		request.GetBody = nil
		request.Header.Set("Content-Type", "application/json")
		request.Header.Set("Accept", "application/json, text/event-stream")
		request.Header.Set("MCP-Protocol-Version", protocolVersion)
		if sessionID != "" {
			request.Header.Set("Mcp-Session-Id", sessionID)
		}
		if ref := template.Secrets["token"]; ref != "" {
			key, err := m.Resolve(ctx, tenant, ref)
			if err != nil {
				return nil, err
			}
			request.Header.Set("Authorization", "Bearer "+key)
		}
		response, err := client.Do(request)
		if err != nil {
			return nil, err
		}
		if sessionID == "" {
			sessionID = response.Header.Get("Mcp-Session-Id")
		}
		defer response.Body.Close()
		if response.StatusCode < 200 || response.StatusCode >= 300 {
			return nil, fmt.Errorf("MCP HTTP %d", response.StatusCode)
		}
		if id == "" {
			return nil, nil
		}
		decode := func(body []byte) (json.RawMessage, bool, error) {
			var reply struct {
				ID     string          `json:"id"`
				Result json.RawMessage `json:"result"`
				Error  json.RawMessage `json:"error"`
			}
			if err := json.Unmarshal(body, &reply); err != nil {
				return nil, false, fmt.Errorf("invalid MCP response")
			}
			if reply.ID != id {
				return nil, false, nil
			}
			if len(reply.Error) > 0 && string(reply.Error) != "null" {
				return nil, true, fmt.Errorf("MCP returned a JSON-RPC error")
			}
			if len(reply.Result) == 0 {
				return nil, true, fmt.Errorf("missing MCP result")
			}
			return reply.Result, true, nil
		}
		limit := int64(execution.MaxOutputBytes) + 16384
		if strings.HasPrefix(response.Header.Get("Content-Type"), "text/event-stream") {
			reader := bufio.NewReader(io.LimitReader(response.Body, limit+1))
			parts := []string{}
			size := int64(0)
			for {
				line, err := reader.ReadString('\n')
				size += int64(len(line))
				if size > limit {
					return nil, fmt.Errorf("MCP event stream exceeded its byte limit")
				}
				text := strings.TrimSuffix(strings.TrimSuffix(line, "\n"), "\r")
				if text == "" && len(parts) > 0 {
					value, matched, e := decode([]byte(strings.Join(parts, "\n")))
					if e != nil || matched {
						return value, e
					}
					parts = parts[:0]
				} else if strings.HasPrefix(text, "data:") {
					parts = append(parts, strings.TrimSpace(text[5:]))
				}
				if err != nil {
					return nil, fmt.Errorf("MCP stream ended before its correlated result")
				}
			}
		}
		body, err := policy.Body(response, limit)
		if err != nil {
			return nil, err
		}
		value, matched, err := decode(body)
		if err != nil {
			return nil, err
		}
		if !matched {
			return nil, fmt.Errorf("MCP response identity mismatch")
		}
		return value, nil
	}
	initialized, err := call(execution.Operation.ID+":initialize", "initialize", map[string]any{"protocolVersion": "2025-11-25", "capabilities": map[string]any{}, "clientInfo": map[string]string{"name": "brain", "version": "2"}})
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("MCP initialization did not produce a receipt")}, nil
	}
	var protocol struct {
		ProtocolVersion string `json:"protocolVersion"`
	}
	_ = json.Unmarshal(initialized, &protocol)
	if protocol.ProtocolVersion != "2025-11-25" && protocol.ProtocolVersion != "2025-06-18" {
		return wire.Receipt{Outcome: wire.OutcomeFailed, Error: kernel.Ptr("MCP server selected an unsupported protocol version")}, nil
	}
	protocolVersion = protocol.ProtocolVersion
	if _, err = call("", "notifications/initialized", map[string]any{}); err != nil {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("MCP initialization acknowledgement is uncertain")}, nil
	}
	result, err := call(execution.Operation.ID, "tools/call", map[string]any{"name": tool, "arguments": json.RawMessage(execution.Arguments)})
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("MCP tool may have executed; it is not automatically repeated")}, nil
	}
	return wire.Receipt{Outcome: wire.OutcomeCompleted, Result: result}, nil
}
