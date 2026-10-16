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
	"os"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/core"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
)

// HTTPTransport implements JSON responses and the progress/result SSE protocol.
type HTTPTransport struct{ client *http.Client }

func NewHTTPTransport() *HTTPTransport { return &HTTPTransport{client: newHTTPClient()} }

func newHTTPClient() *http.Client {
	return &http.Client{Transport: &http.Transport{Proxy: http.ProxyFromEnvironment, MaxIdleConns: 256, MaxIdleConnsPerHost: 32, IdleConnTimeout: 90 * time.Second, ResponseHeaderTimeout: 30 * time.Second}, CheckRedirect: func(*http.Request, []*http.Request) error { return errors.New("tool redirects are disabled") }}
}

func (t *HTTPTransport) Close() { t.client.CloseIdleConnections() }
func (t *HTTPTransport) Call(ctx context.Context, binding Binding, request Request, progress func(json.RawMessage)) core.Result {
	body, err := json.Marshal(request)
	if err != nil {
		return core.ToolError("invalid tool request", false)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, binding.URL, bytes.NewReader(body))
	if err != nil {
		return core.ToolError("invalid tool request", false)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json, text/event-stream")
	otel.GetTextMapPropagator().Inject(ctx, propagation.HeaderCarrier(req.Header))
	if binding.TokenEnv != "" {
		token := os.Getenv(binding.TokenEnv)
		if token == "" {
			return core.ToolError("tool credential is unavailable", false)
		}
		req.Header.Set("Authorization", "Bearer "+token)
	}
	res, err := t.client.Do(req)
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
	reader := &io.LimitedReader{R: res.Body, N: binding.MaxOutputBytes + 1}
	var result core.Result
	if ct == "text/event-stream" {
		result, err = readSSE(reader, progress)
	} else if ct == "application/json" {
		data, e := io.ReadAll(reader)
		err = e
		if err == nil {
			err = decodeResult(data, &result, nil)
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
	return result
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
