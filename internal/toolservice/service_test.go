package toolservice

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/tools"
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/aws/aws-sdk-go-v2/service/s3"
)

func TestSharedCapacityBoundsManagedAndHTTPCalls(t *testing.T) {
	capacity := make(chan struct{}, 1)
	first, err := New(context.Background(), Settings{Token: strings.Repeat("a", 32), Capacity: capacity})
	if err != nil {
		t.Fatal(err)
	}
	defer first.Close()
	second, err := New(context.Background(), Settings{Token: strings.Repeat("b", 32), Capacity: capacity})
	if err != nil {
		t.Fatal(err)
	}
	defer second.Close()
	started, release := make(chan struct{}), make(chan struct{})
	var releaseOnce sync.Once
	unblock := func() { releaseOnce.Do(func() { close(release) }) }
	defer unblock()
	first.fetch.Transport = transportFunc(func(request *http.Request) (*http.Response, error) {
		close(started)
		<-release
		return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"text/plain"}}, Body: io.NopCloser(strings.NewReader("done")), Request: request}, nil
	})
	var extra atomic.Int32
	second.fetch.Transport = transportFunc(func(*http.Request) (*http.Response, error) {
		extra.Add(1)
		return nil, io.ErrUnexpectedEOF
	})
	request := tools.Request{ToolScope: core.ToolScope{Tenant: "tenant", Session: "session", Call: "call"}, Arguments: json.RawMessage(`{"url":"https://example.com"}`)}
	finished := make(chan core.Result, 1)
	go func() { finished <- first.Call(context.Background(), "/web_fetch", request) }()
	<-started
	result := second.Call(context.Background(), "/web_fetch", request)
	if !result.IsError || result.UnknownOutcome {
		t.Fatal("managed call bypassed shared capacity", result)
	}
	server := httptest.NewServer(second.Handler())
	defer server.Close()
	body, _ := json.Marshal(request)
	httpRequest, _ := http.NewRequest("POST", server.URL+"/web_fetch", strings.NewReader(string(body)))
	httpRequest.Header.Set("Authorization", "Bearer "+strings.Repeat("b", 32))
	response, err := http.DefaultClient.Do(httpRequest)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if err := json.NewDecoder(response.Body).Decode(&result); err != nil {
		t.Fatal(err)
	}
	if !result.IsError || extra.Load() != 0 {
		t.Fatal("HTTP call bypassed shared capacity", result)
	}
	unblock()
	if result := <-finished; result.IsError {
		t.Fatal(result)
	}
}

func TestStorageJourneyAndTenantIsolation(t *testing.T) {
	var mu sync.Mutex
	objects := map[string]string{}
	cloud := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		if !strings.HasPrefix(r.URL.Path, "/bucket/resources/") {
			t.Errorf("unexpected storage path %s", r.URL.Path)
		}
		switch r.Method {
		case "PUT":
			b, _ := io.ReadAll(r.Body)
			objects[r.URL.Path] = string(b)
		case "GET":
			if b, ok := objects[r.URL.Path]; ok {
				_, _ = io.WriteString(w, b)
			} else {
				w.WriteHeader(404)
			}
		case "DELETE":
			delete(objects, r.URL.Path)
		}
	}))
	defer cloud.Close()
	service, err := New(context.Background(), Settings{Token: strings.Repeat("s", 32), Prefix: "resources", AllowWrite: true})
	if err != nil {
		t.Fatal(err)
	}
	service.cfg.Bucket = "bucket"
	service.objects = s3.New(s3.Options{Region: "us-east-1", BaseEndpoint: &cloud.URL, UsePathStyle: true, Credentials: credentials.NewStaticCredentialsProvider("test", "test", ""), RetryMaxAttempts: 1, RequestChecksumCalculation: aws.RequestChecksumCalculationWhenRequired})
	server := httptest.NewServer(service.Handler())
	defer server.Close()
	t.Setenv("RESOURCE_TOOL_TOKEN", service.cfg.Token)
	registry, err := tools.New(service.Catalog(server.URL, "a"))
	if err != nil {
		t.Fatal(err)
	}
	defer registry.Close()
	call := func(name, args string) core.Result {
		return registry.Call(context.Background(), core.ToolScope{Tenant: "a", Session: "session", Call: "call"}, core.Call{Name: name, Arguments: json.RawMessage(args)}, nil)
	}
	if got := call("storage_write", `{"key":"note.txt","text":"hello"}`); got.IsError {
		t.Fatal(got)
	}
	if got := call("storage_read", `{"key":"note.txt"}`); got.IsError || !strings.Contains(got.Content[0].Text, "hello") {
		t.Fatal(got)
	}
	if got := service.storage(context.Background(), "/storage_read", tools.Request{ToolScope: core.ToolScope{Tenant: "b"}, Arguments: json.RawMessage(`{"key":"note.txt"}`)}); !got.IsError {
		t.Fatal("cross tenant read")
	}
	if got := call("storage_read", `{"key":"../escape"}`); !got.IsError {
		t.Fatal("path escape")
	}
	service.cfg.AllowWrite = false
	if got := call("storage_delete", `{"key":"note.txt"}`); !got.IsError {
		t.Fatal("read-only mutation")
	}
	service.cfg.AllowWrite = true
	if got := call("storage_delete", `{"key":"note.txt"}`); got.IsError {
		t.Fatal(got)
	}
	if got := call("storage_read", `{"key":"note.txt"}`); !got.IsError {
		t.Fatal("deleted object exists")
	}
	res, err := http.Post(server.URL+"/storage_read", "application/json", strings.NewReader(`{}`))
	if err != nil {
		t.Fatal(err)
	}
	defer res.Body.Close()
	if res.StatusCode != 401 {
		t.Fatal("missing authentication accepted")
	}
}

type transportFunc func(*http.Request) (*http.Response, error)

func (f transportFunc) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

func TestSearchProvidersAndFetchLimits(t *testing.T) {
	for _, provider := range []string{"brave", "tavily"} {
		t.Run(provider, func(t *testing.T) {
			s, err := New(context.Background(), Settings{Token: strings.Repeat("s", 32), SearchProvider: provider, SearchKey: "secret"})
			if err != nil {
				t.Fatal(err)
			}
			s.client.Transport = transportFunc(func(r *http.Request) (*http.Response, error) {
				data := `{"results":[{"title":"Source","url":"https://example.com","content":"answer"}]}`
				if provider == "brave" {
					if r.Header.Get("X-Subscription-Token") != "secret" || r.URL.Query().Get("q") != "a & b" {
						t.Error("bad Brave request")
					}
					data = `{"web":` + data + `}`
				} else {
					if r.Header.Get("Authorization") != "Bearer secret" {
						t.Error("missing Tavily credential")
					}
					var args map[string]any
					_ = json.NewDecoder(r.Body).Decode(&args)
					if args["query"] != "a & b" {
						t.Error("bad Tavily query")
					}
				}
				return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(data)), Header: http.Header{}}, nil
			})
			got := s.webSearch(context.Background(), "a & b")
			if got.IsError || !strings.Contains(got.Content[0].Text, "https://example.com") {
				t.Fatal(got)
			}
		})
	}
	s, _ := New(context.Background(), Settings{Token: strings.Repeat("s", 32)})
	s.fetch.Transport = transportFunc(func(r *http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(`<h1>Title</h1><script>secret</script><style>css</style><p>Hello &amp; world</p>`)), Header: http.Header{"Content-Type": []string{"text/html"}}, Request: r}, nil
	})
	got := s.webFetch(context.Background(), "https://example.com")
	if got.IsError || strings.Contains(got.Content[0].Text, "secret") || !strings.Contains(got.Content[0].Text, "Hello") {
		t.Fatal(got)
	}
	s.fetch = publicClient()
	if got := s.webFetch(context.Background(), "http://127.0.0.1"); !got.IsError {
		t.Fatal("loopback fetched")
	}
}

func TestPublicNetworkBoundary(t *testing.T) {
	for _, raw := range []string{"127.0.0.1", "10.2.3.4", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.100.100.200", "0.0.0.0", "224.0.0.1", "::1", "::ffff:127.0.0.1", "fc00::1", "fe80::1", "2001:db8::1", "2002:7f00:1::"} {
		if publicIP(netip.MustParseAddr(raw)) {
			t.Errorf("accepted %s", raw)
		}
	}
	for _, raw := range []string{"1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"} {
		if !publicIP(netip.MustParseAddr(raw)) {
			t.Errorf("blocked public %s", raw)
		}
	}
	for _, raw := range []string{"file:///etc/passwd", "https://user:secret@example.com", "http://example.com:8080"} {
		if _, err := fetchURL(raw); err == nil {
			t.Errorf("accepted %s", raw)
		}
	}
}
