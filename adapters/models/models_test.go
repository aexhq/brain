package models

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestAuthorityAndTransportOptionsFailBeforeProviderExecution(t *testing.T) {
	registry := New(map[string]Provider{"fixture": {Kind: "openai"}}, nil)
	for _, options := range []string{`{"model":"changed"}`, `{"tools":[]}`, `{"max_retries":1}`, `{"extra_headers":{}}`, `{"background":true}`} {
		if err := registry.Validate(wire.ModelSelection{Provider: "fixture", Name: "model", Native: []byte(options)}); err == nil {
			t.Fatalf("admitted authority/transport option: %s", options)
		}
	}
	if err := registry.Validate(wire.ModelSelection{Provider: "fixture", Name: "model", Native: []byte(`{"temperature":0.2,"max_tokens":16}`)}); err != nil {
		t.Fatal(err)
	}
}

func TestSDKLostResponseAndRateLimitNeverRepeatModelInvocation(t *testing.T) {
	for _, kind := range []string{"openai", "openai-responses", "anthropic"} {
		for _, fault := range []string{"lost", "rate-limited", "redirect"} {
			t.Run(kind+"/"+fault, func(t *testing.T) {
				var calls atomic.Int32
				service := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
					calls.Add(1)
					_, _ = io.Copy(io.Discard, r.Body)
					switch fault {
					case "lost":
						connection, _, err := w.(http.Hijacker).Hijack()
						if err != nil {
							t.Error(err)
							return
						}
						_ = connection.Close()
					case "rate-limited":
						w.Header().Set("Retry-After", "0")
						w.WriteHeader(http.StatusTooManyRequests)
						_, _ = io.WriteString(w, `{"error":{"message":"rate limit","type":"rate_limit"}}`)
					case "redirect":
						w.Header().Set("Location", "/must-not-repeat")
						w.WriteHeader(http.StatusTemporaryRedirect)
					}
				}))
				defer service.Close()
				registry := New(map[string]Provider{"fixture": {Kind: kind, BaseURL: service.URL + "/"}},
					func(context.Context, string, string) (string, error) { return "fixture-model-authority", nil })
				request := wire.ModelRequest{Messages: []wire.ModelMessage{{Role: "user", Content: kernel.JSON("hello")}}}
				if _, err := registry.Call(context.Background(), "tenant", wire.ModelSelection{Provider: "fixture", Name: "fixture"}, request, 4096); err != nil {
					t.Fatal(err)
				}
				if calls.Load() != 1 {
					t.Fatalf("provider observed %d calls", calls.Load())
				}
			})
		}
	}
}

func TestTransportBoundsSDKErrorDecodingAndDisablesBodyReplay(t *testing.T) {
	client := boundedTransport{base: roundTripFunc(func(r *http.Request) (*http.Response, error) {
		if r.GetBody != nil || !r.Close {
			t.Fatal("SDK request retained transport replay permission")
		}
		return &http.Response{StatusCode: 500, Body: io.NopCloser(strings.NewReader(strings.Repeat("x", 1<<20)))}, nil
	}), limit: 4096}
	request, _ := http.NewRequest(http.MethodPost, "https://model.invalid", strings.NewReader("request"))
	response, err := client.RoundTrip(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	body, err := io.ReadAll(response.Body)
	if err != nil || len(body) != 4097 {
		t.Fatalf("SDK response bound: %d %v", len(body), err)
	}
}

type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
