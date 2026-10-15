package network

import (
	"bytes"
	"compress/gzip"
	"io"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestPublicDestinationRestrictions(t *testing.T) {
	for _, value := range []string{"127.0.0.1", "10.2.3.4", "169.254.169.254", "100.64.0.1", "192.168.1.1", "198.18.0.1", "0.0.0.0", "224.0.0.1", "::1", "fc00::1", "fe80::1", "::ffff:127.0.0.1", "64:ff9b::7f00:1", "2002:7f00:1::1"} {
		if Public(netip.MustParseAddr(value)) {
			t.Errorf("admitted private or reserved address %s", value)
		}
	}
	for _, value := range []string{"1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"} {
		if !Public(netip.MustParseAddr(value)) {
			t.Errorf("rejected public address %s", value)
		}
	}
	for _, value := range []string{"file:///etc/passwd", "https://user:password@example.com", "http://127.0.0.1", "https://example.com:8443", "https://[::1]"} {
		if _, err := ValidateURL(value); err == nil {
			t.Errorf("admitted invalid URL %s", value)
		}
	}
	if _, err := ValidateURL("https://example.com/resource"); err != nil {
		t.Fatal(err)
	}
}

func TestDialRechecksPrivateDNSBeforeAnyRequest(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); w.WriteHeader(200) }))
	defer server.Close()
	client := Client(time.Second)
	for _, target := range []string{server.URL, strings.Replace(server.URL, "127.0.0.1", "localhost", 1)} {
		response, err := client.Get(target)
		if err == nil {
			response.Body.Close()
			t.Fatalf("private destination was contacted: %s", target)
		}
	}
	if calls.Load() != 0 {
		t.Fatal("denied destination observed an external request")
	}
}

func TestDecodedBodyLimit(t *testing.T) {
	var buffer bytes.Buffer
	writer := gzip.NewWriter(&buffer)
	_, _ = writer.Write(bytes.Repeat([]byte("x"), 65536))
	_ = writer.Close()
	response := &http.Response{Header: http.Header{"Content-Encoding": []string{"gzip"}}, Body: io.NopCloser(bytes.NewReader(buffer.Bytes()))}
	if _, err := Body(response, 1024); err == nil {
		t.Fatal("compressed bomb exceeded decoded bound without failure")
	}
	response = &http.Response{Header: http.Header{}, Body: io.NopCloser(strings.NewReader("bounded"))}
	body, err := Body(response, 7)
	if err != nil || string(body) != "bounded" {
		t.Fatalf("bounded response: %q %v", body, err)
	}
}
