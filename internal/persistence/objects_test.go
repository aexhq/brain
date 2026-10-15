package persistence

import (
	"context"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"sync"
	"testing"
)

// Exercise the real AWS SDK, including signed requests and R2 configuration.
// This is a transport contract test; cloud-account qualification is separate.
func TestS3CompatibleTransport(t *testing.T) {
	for _, provider := range []string{"s3", "r2"} {
		t.Run(provider, func(t *testing.T) {
			var mu sync.Mutex
			objects := map[string][]byte{}
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if !strings.HasPrefix(r.Header.Get("Authorization"), "AWS4-HMAC-SHA256 ") {
					t.Error("unsigned request")
				}
				if !strings.HasPrefix(r.URL.Path, "/bucket/brain/") {
					t.Errorf("unexpected key %s", r.URL.Path)
				}
				mu.Lock()
				defer mu.Unlock()
				switch r.Method {
				case "PUT":
					if _, ok := objects[r.URL.Path]; ok && r.Header.Get("If-None-Match") == "*" {
						w.WriteHeader(412)
						_, _ = io.WriteString(w, "<Error><Code>PreconditionFailed</Code></Error>")
						return
					}
					b, err := io.ReadAll(r.Body)
					if err != nil {
						t.Error(err)
					}
					objects[r.URL.Path] = b
					w.Header().Set("ETag", `"test"`)
				case "GET":
					b, ok := objects[r.URL.Path]
					if !ok {
						w.WriteHeader(404)
						_, _ = io.WriteString(w, "<Error><Code>NoSuchKey</Code></Error>")
						return
					}
					_, _ = w.Write(b)
				default:
					t.Errorf("unexpected request %s", r.Method)
					w.WriteHeader(405)
				}
			}))
			defer server.Close()
			t.Setenv("TEST_STORAGE_ACCESS", "local-access")
			t.Setenv("TEST_STORAGE_SECRET", "local-secret")
			store, err := NewObjects(context.Background(), Settings{Provider: provider, Bucket: "bucket", Region: "us-east-1", Endpoint: server.URL, Prefix: "brain", AccessKeyEnv: "TEST_STORAGE_ACCESS", SecretKeyEnv: "TEST_STORAGE_SECRET"})
			if err != nil {
				t.Fatal(err)
			}
			ctx := context.Background()
			if err := store.Put(ctx, "chunk", []byte("content"), true); err != nil {
				t.Fatal(err)
			}
			if err := store.Put(ctx, "chunk", []byte("replacement"), true); err != nil {
				t.Fatal(err)
			}
			b, err := store.Get(ctx, "chunk")
			if err != nil || string(b) != "content" {
				t.Fatalf("read %q: %v", b, err)
			}
			if _, err := store.Get(ctx, "missing"); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("missing object: %v", err)
			}
		})
	}
}
