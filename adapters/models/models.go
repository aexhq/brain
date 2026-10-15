// Package models is the concrete SDK boundary. Every invocation disables SDK
// retries and redirects, and response decoding is bounded before allocation.
package models

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	anthropic "github.com/anthropics/anthropic-sdk-go"
	anthropicoption "github.com/anthropics/anthropic-sdk-go/option"
	openai "github.com/openai/openai-go/v3"
	openaioption "github.com/openai/openai-go/v3/option"
)

type Provider struct {
	Kind       string `json:"kind"`
	BaseURL    string `json:"base_url"`
	Credential string `json:"credential"`
}

type Resolver func(context.Context, string, string) (string, error)
type Registry struct {
	Providers map[string]Provider
	Resolve   Resolver
	Client    *http.Client
}

func New(providers map[string]Provider, resolve Resolver) *Registry {
	return &Registry{Providers: providers, Resolve: resolve, Client: &http.Client{Transport: transport(), Timeout: 300 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}

func (r *Registry) Validate(selection wire.ModelSelection) error {
	p, ok := r.Providers[selection.Provider]
	if !ok || (p.Kind != "openai" && p.Kind != "anthropic" && p.Kind != "openai-responses") {
		return kernel.Fail(wire.ErrorCodeUnsupported, "model provider is not configured")
	}
	if selection.Name == "" || len(selection.Name) > 256 || len(selection.Native) > 65536 || len(selection.Format) > 65536 {
		return kernel.Fail(wire.ErrorCodeInvalid, "model selection exceeds its bounded fields")
	}
	if p.BaseURL != "" {
		base, err := url.Parse(p.BaseURL)
		if err != nil || base.User != nil || base.Hostname() == "" || base.RawQuery != "" || base.Fragment != "" ||
			(base.Scheme != "http" && base.Scheme != "https") {
			return kernel.Fail(wire.ErrorCodeInvalid, "configured model base URL is invalid")
		}
	}
	if _, err := nativeOptions(selection); err != nil {
		return err
	}
	if _, err := kernel.ModelOutputTokens(selection); err != nil {
		return err
	}
	var native map[string]json.RawMessage
	_ = json.Unmarshal(selection.Native, &native)
	for _, key := range []string{"max_tokens", "max_completion_tokens", "max_output_tokens"} {
		if _, ok := native[key]; ok && ((p.Kind == "anthropic" && key != "max_tokens") ||
			(p.Kind == "openai-responses" && key != "max_output_tokens") || (p.Kind == "openai" && key == "max_output_tokens")) {
			return kernel.Fail(wire.ErrorCodeInvalid, "native output-token limit belongs to another model API")
		}
	}
	if len(selection.Format) > 0 && p.Kind == "anthropic" {
		return kernel.Fail(wire.ErrorCodeUnsupported, "this Anthropic adapter does not support a model output format")
	}
	return nil
}

func nativeOptions(selection wire.ModelSelection) (map[string]any, error) {
	payload := map[string]any{}
	if len(selection.Native) != 0 && json.Unmarshal(selection.Native, &payload) != nil {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "native model options must be an object")
	}
	if payload == nil {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "native model options must be an object")
	}
	for key, value := range payload {
		switch strings.ToLower(key) {
		case "api_key", "base_url", "model", "messages", "input", "tools", "stream", "http_client",
			"max_retries", "timeout", "headers", "extra_headers", "extra_body", "proxies":
			return nil, kernel.Fail(wire.ErrorCodeInvalid, "native options cannot replace model authority, messages, tools or transport")
		case "background":
			if value != false {
				return nil, kernel.Fail(wire.ErrorCodeUnsupported, "this model path does not poll background provider jobs")
			}
		case "n", "best_of":
			if value != float64(1) {
				return nil, kernel.Fail(wire.ErrorCodeUnsupported, "a model operation requests one candidate, not hidden additional generations")
			}
		}
	}
	return payload, nil
}

func (r *Registry) Credential(selection wire.ModelSelection) string {
	if selection.Credential != nil {
		return *selection.Credential
	}
	return r.Providers[selection.Provider].Credential
}

func (r *Registry) Capability(name string) string {
	provider := r.Providers[name]
	return kernel.Capability("model", name, provider.Kind, provider.BaseURL)
}

func (r *Registry) Call(ctx context.Context, tenant string, selection wire.ModelSelection, request wire.ModelRequest, limit int64) (wire.Receipt, error) {
	if err := r.Validate(selection); err != nil {
		return wire.Receipt{}, err
	}
	provider := r.Providers[selection.Provider]
	credential, err := r.Resolve(ctx, tenant, r.Credential(selection))
	if err != nil {
		return wire.Receipt{}, err
	}
	payload, err := nativeOptions(selection)
	if err != nil {
		return wire.Receipt{}, err
	}
	clientTransport := r.Client.Transport
	if clientTransport == nil {
		clientTransport = transport()
	}
	httpClient := *r.Client
	httpClient.Transport = boundedTransport{base: clientTransport, limit: limit}
	payload["model"] = selection.Name
	outputTokens, err := kernel.ModelOutputTokens(selection)
	if err != nil {
		return wire.Receipt{}, err
	}
	if provider.Kind == "openai" {
		if _, legacy := payload["max_tokens"]; !legacy {
			payload["max_completion_tokens"] = outputTokens
		}
	} else if provider.Kind == "openai-responses" {
		payload["max_output_tokens"] = outputTokens
	} else {
		payload["max_tokens"] = outputTokens
	}
	var response *http.Response
	switch provider.Kind {
	case "openai", "openai-responses":
		options := []openaioption.RequestOption{openaioption.WithAPIKey(credential), openaioption.WithMaxRetries(0), openaioption.WithHTTPClient(&httpClient)}
		if provider.BaseURL != "" {
			options = append(options, openaioption.WithBaseURL(provider.BaseURL))
		}
		client := openai.NewClient(options...)
		messages := []any{}
		tools := []any{}
		for _, m := range request.Messages {
			item := map[string]any{"role": m.Role, "content": json.RawMessage(m.Content)}
			if len(m.ToolCalls) > 0 {
				calls := []any{}
				for _, call := range m.ToolCalls {
					calls = append(calls, map[string]any{"id": call.ID, "type": "function", "function": map[string]any{"name": call.Name, "arguments": string(call.Arguments)}})
				}
				item["tool_calls"] = calls
			}
			if m.ToolCallID != nil {
				item["tool_call_id"] = *m.ToolCallID
			}
			messages = append(messages, item)
		}
		endpoint := "chat/completions"
		payload["messages"] = messages
		for _, a := range request.Actions {
			tools = append(tools, map[string]any{"type": "function", "function": map[string]any{"name": a.Name, "description": a.Description, "parameters": json.RawMessage(a.InputSchema)}})
		}
		if len(tools) > 0 {
			payload["tools"] = tools
		}
		if len(selection.Format) > 0 {
			payload["response_format"] = json.RawMessage(selection.Format)
		}
		if provider.Kind == "openai-responses" {
			endpoint = "responses"
			delete(payload, "messages")
			delete(payload, "response_format")
			input := []any{}
			tools = []any{}
			for _, m := range request.Messages {
				if m.Role == "tool" {
					input = append(input, map[string]any{"type": "function_call_output", "call_id": kernel.Value(m.ToolCallID), "output": rawText(m.Content)})
					continue
				}
				if len(m.ToolCalls) == 0 || string(m.Content) != "null" {
					input = append(input, map[string]any{"role": m.Role, "content": json.RawMessage(m.Content)})
				}
				for _, call := range m.ToolCalls {
					input = append(input, map[string]any{"type": "function_call", "call_id": call.ID, "name": call.Name, "arguments": string(call.Arguments)})
				}
			}
			for _, a := range request.Actions {
				tools = append(tools, map[string]any{"type": "function", "name": a.Name, "description": a.Description, "parameters": json.RawMessage(a.InputSchema)})
			}
			payload["input"] = input
			if len(tools) > 0 {
				payload["tools"] = tools
			}
			if len(selection.Format) > 0 {
				payload["text"] = map[string]any{"format": json.RawMessage(selection.Format)}
			}
		}
		err = client.Execute(ctx, http.MethodPost, endpoint, payload, &response)
	case "anthropic":
		options := []anthropicoption.RequestOption{anthropicoption.WithAPIKey(credential), anthropicoption.WithMaxRetries(0), anthropicoption.WithHTTPClient(&httpClient)}
		if provider.BaseURL != "" {
			options = append(options, anthropicoption.WithBaseURL(provider.BaseURL))
		}
		client := anthropic.NewClient(options...)
		messages := []any{}
		tools := []any{}
		for _, m := range request.Messages {
			if m.Role == "system" {
				payload["system"] = rawText(m.Content)
				continue
			}
			content := []any{}
			if m.Role == "tool" {
				content = append(content, map[string]any{"type": "tool_result", "tool_use_id": kernel.Value(m.ToolCallID), "content": rawText(m.Content)})
			} else {
				if text := rawText(m.Content); text != "" && text != "null" {
					content = append(content, map[string]any{"type": "text", "text": text})
				}
				for _, call := range m.ToolCalls {
					content = append(content, map[string]any{"type": "tool_use", "id": call.ID, "name": call.Name, "input": json.RawMessage(call.Arguments)})
				}
			}
			role := m.Role
			if role == "tool" {
				role = "user"
			}
			messages = append(messages, map[string]any{"role": role, "content": content})
		}
		for _, a := range request.Actions {
			tools = append(tools, map[string]any{"name": a.Name, "description": a.Description, "input_schema": json.RawMessage(a.InputSchema)})
		}
		payload["messages"] = messages
		if len(tools) > 0 {
			payload["tools"] = tools
		}
		if _, ok := payload["max_tokens"]; !ok {
			payload["max_tokens"] = 4096
		}
		err = client.Execute(ctx, http.MethodPost, "v1/messages", payload, &response)
	}
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("model request has an uncertain remote outcome")}, nil
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, limit+1))
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("model response was interrupted")}, nil
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return wire.Receipt{Outcome: wire.OutcomeFailed, Error: kernel.Ptr("model provider returned HTTP " + response.Status)}, nil
	}
	if int64(len(body)) > limit {
		return wire.Receipt{Outcome: wire.OutcomeCompleted, Error: kernel.Ptr("model completed; response exceeds admitted output limit")}, nil
	}
	result, err := normalize(provider.Kind, body)
	if err != nil {
		return wire.Receipt{Outcome: wire.OutcomeCompleted, Error: kernel.Ptr("model completed; response cannot be decoded")}, nil
	}
	return wire.Receipt{Outcome: wire.OutcomeCompleted, Result: kernel.JSON(result), Usage: &result.Usage}, nil
}

func rawText(value json.RawMessage) string {
	var text string
	if json.Unmarshal(value, &text) == nil {
		return text
	}
	return string(value)
}

func normalize(kind string, body []byte) (wire.ModelResponse, error) {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(body, &raw); err != nil {
		return wire.ModelResponse{}, err
	}
	result := wire.ModelResponse{Message: wire.ModelMessage{Role: "assistant", Content: kernel.JSON(""), ToolCalls: []wire.ModelToolCall{}}, Usage: wire.Usage{}, Native: body}
	var usage map[string]json.RawMessage
	_ = json.Unmarshal(raw["usage"], &usage)
	counter := func(names ...string) *uint64 {
		for _, name := range names {
			if data, ok := usage[name]; ok && string(data) != "null" {
				var n uint64
				if json.Unmarshal(data, &n) == nil {
					return &n
				}
			}
		}
		return nil
	}
	result.Usage.InputTokens = counter("input_tokens", "prompt_tokens")
	result.Usage.OutputTokens = counter("output_tokens", "completion_tokens")
	result.Usage.CachedTokens = counter("cache_read_input_tokens")
	result.Usage.Provider = raw["usage"]
	switch kind {
	case "openai":
		var choices []struct {
			Message struct {
				Role    string          `json:"role"`
				Content json.RawMessage `json:"content"`
				Calls   []struct {
					ID       string `json:"id"`
					Function struct {
						Name      string `json:"name"`
						Arguments string `json:"arguments"`
					} `json:"function"`
				} `json:"tool_calls"`
			} `json:"message"`
		}
		if err := json.Unmarshal(raw["choices"], &choices); err != nil || len(choices) == 0 {
			return result, kernel.Fail(wire.ErrorCodeInvalid, "missing assistant choice")
		}
		result.Message.Content = choices[0].Message.Content
		for _, call := range choices[0].Message.Calls {
			if !json.Valid([]byte(call.Function.Arguments)) {
				return result, kernel.Fail(wire.ErrorCodeInvalid, "invalid tool arguments")
			}
			result.Message.ToolCalls = append(result.Message.ToolCalls, wire.ModelToolCall{ID: call.ID, Name: call.Function.Name, Arguments: []byte(call.Function.Arguments)})
		}
	case "openai-responses":
		var output []struct {
			Type      string `json:"type"`
			CallID    string `json:"call_id"`
			Name      string `json:"name"`
			Arguments string `json:"arguments"`
			Content   []struct {
				Text string `json:"text"`
			} `json:"content"`
		}
		if err := json.Unmarshal(raw["output"], &output); err != nil {
			return result, err
		}
		text := strings.Builder{}
		for _, item := range output {
			if item.Type == "function_call" {
				if !json.Valid([]byte(item.Arguments)) {
					return result, kernel.Fail(wire.ErrorCodeInvalid, "invalid tool arguments")
				}
				result.Message.ToolCalls = append(result.Message.ToolCalls, wire.ModelToolCall{ID: item.CallID, Name: item.Name, Arguments: []byte(item.Arguments)})
			}
			for _, part := range item.Content {
				text.WriteString(part.Text)
			}
		}
		result.Message.Content = kernel.JSON(text.String())
	case "anthropic":
		var content []struct {
			Type  string          `json:"type"`
			Text  string          `json:"text"`
			ID    string          `json:"id"`
			Name  string          `json:"name"`
			Input json.RawMessage `json:"input"`
		}
		if err := json.Unmarshal(raw["content"], &content); err != nil {
			return result, err
		}
		text := strings.Builder{}
		for _, part := range content {
			if part.Type == "text" {
				text.WriteString(part.Text)
			}
			if part.Type == "tool_use" {
				result.Message.ToolCalls = append(result.Message.ToolCalls, wire.ModelToolCall{ID: part.ID, Name: part.Name, Arguments: part.Input})
			}
		}
		result.Message.Content = kernel.JSON(text.String())
	}
	return result, nil
}
