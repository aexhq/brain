package persistence

import (
	"context"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"errors"
	"hash/crc32"
	"io"
	"mime"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"sync"
	"testing"

	"google.golang.org/api/option"
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
			objectContract(t, store)
		})
	}
}

func TestFileObjectContract(t *testing.T) { objectContract(t, FileObjects{Root: t.TempDir()}) }

func objectContract(t *testing.T, store Objects) {
	t.Helper()
	ctx := context.Background()
	for _, value := range []string{"content", "replacement"} {
		if err := store.Put(ctx, "chunk", []byte(value), true); err != nil {
			t.Fatal(err)
		}
	}
	if b, err := store.Get(ctx, "chunk"); err != nil || string(b) != "content" {
		t.Fatalf("immutable read %q: %v", b, err)
	}
	if err := store.Put(ctx, "chunk", []byte("latest"), false); err != nil {
		t.Fatal(err)
	}
	if b, err := store.Get(ctx, "chunk"); err != nil || string(b) != "latest" {
		t.Fatalf("mutable read %q: %v", b, err)
	}
	if _, err := store.Get(ctx, "missing"); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("missing object: %v", err)
	}
	if err := store.Put(ctx, "large", make([]byte, MaxObjectBytes+1), false); err == nil {
		t.Fatal("oversize write accepted")
	}
	if err := store.Put(ctx, "../escape", nil, false); err == nil {
		t.Fatal("invalid key accepted")
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if err := store.Put(cancelled, "cancelled", nil, false); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled write: %v", err)
	}
	if _, err := store.Get(cancelled, "chunk"); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled read: %v", err)
	}
}

func TestGCSTransport(t *testing.T) {
	var mu sync.Mutex
	objects := map[string][]byte{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		if r.Method == "POST" {
			if r.URL.Query().Get("uploadType") != "multipart" {
				t.Error("expected bounded multipart upload")
			}
			_, params, _ := mime.ParseMediaType(r.Header.Get("Content-Type"))
			parts := multipart.NewReader(r.Body, params["boundary"])
			meta, err := parts.NextPart()
			if err != nil {
				t.Error(err)
				w.WriteHeader(400)
				return
			}
			var object struct{ Name string }
			if err := json.NewDecoder(meta).Decode(&object); err != nil {
				t.Error(err)
			}
			if !strings.HasPrefix(object.Name, "brain/") {
				t.Errorf("unexpected key %s", object.Name)
			}
			if _, exists := objects[object.Name]; exists && r.URL.Query().Get("ifGenerationMatch") == "0" {
				w.WriteHeader(412)
				_, _ = io.WriteString(w, `{"error":{"code":412,"message":"exists"}}`)
				return
			}
			data, err := parts.NextPart()
			if err != nil {
				t.Error(err)
				w.WriteHeader(400)
				return
			}
			objects[object.Name], err = io.ReadAll(data)
			if err != nil {
				t.Error(err)
			}
			w.Header().Set("Content-Type", "application/json")
			checksum := make([]byte, 4)
			binary.BigEndian.PutUint32(checksum, crc32.Checksum(objects[object.Name], crc32.MakeTable(crc32.Castagnoli)))
			_ = json.NewEncoder(w).Encode(map[string]string{"generation": "1", "crc32c": base64.StdEncoding.EncodeToString(checksum)})
			return
		}
		key := strings.TrimPrefix(r.URL.Path, "/b/bucket/o/")
		data, ok := objects[key]
		if !ok {
			w.WriteHeader(404)
			return
		}
		_, _ = w.Write(data)
	}))
	defer server.Close()
	store, err := newGCS(context.Background(), Settings{Bucket: "bucket", Prefix: "brain"}, option.WithEndpoint(server.URL), option.WithoutAuthentication())
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	objectContract(t, store)
}
