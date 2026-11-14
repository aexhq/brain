package storage

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"

	core "github.com/aexhq/brain/services/brain/api"
)

func TestExternalReferencesKeepConfiguredOriginAndDenyRedirects(t *testing.T) {
	var unexpected atomic.Int32
	other := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { unexpected.Add(1); w.Write([]byte("private")) }))
	defer other.Close()
	allowed := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/redirect" {
			http.Redirect(w, r, other.URL, http.StatusFound)
			return
		}
		if r.URL.RequestURI() != "/notes%2Fone?q=value" {
			t.Errorf("changed path/query: %s", r.URL.RequestURI())
		}
		w.Write([]byte("authorized content"))
	}))
	defer allowed.Close()
	w := &Workspace{MediaOrigins: []string{allowed.URL}}
	input := func(uri string) core.Input {
		return core.Input{References: []core.Reference{{URI: uri, MediaType: "text/plain"}}}
	}
	ctx := context.Background()
	got, err := w.ResolveInput(ctx, "session", input(allowed.URL+"/notes%2Fone?q=value"))
	if err != nil || !strings.Contains(got.Text, "authorized content") {
		t.Fatal(got, err)
	}
	if _, err = w.ResolveInput(ctx, "session", input(allowed.URL+"/redirect")); core.ErrorCode(err) != core.Invalid {
		t.Fatal("redirect accepted", err)
	}
	for _, uri := range []string{other.URL, strings.Replace(allowed.URL, "http://", "http://user@", 1), allowed.URL + "#fragment", strings.Replace(allowed.URL, "http:", "https:", 1)} {
		if err = w.ValidateInput(ctx, "session", input(uri)); err == nil {
			t.Fatalf("unapproved destination accepted: %s", uri)
		}
	}
	if unexpected.Load() != 0 {
		t.Fatal("request reached unauthorized origin")
	}
}
