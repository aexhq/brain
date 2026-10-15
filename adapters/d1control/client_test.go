package d1control

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestClaimTransportLossReturnsNoPermissionAndNeverResends(t *testing.T) {
	key := make([]byte, 32)
	var claims atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Error(err)
			return
		}
		mac := hmac.New(sha256.New, key)
		_, _ = mac.Write([]byte("brain-d1/1\x00" + r.URL.Path + "\x00" + r.Header.Get("X-Brain-Request-Time") + "\x00"))
		_, _ = mac.Write(body)
		if r.Header.Get("X-Brain-Control-Signature") != hex.EncodeToString(mac.Sum(nil)) {
			t.Error("client signature did not cover the exact route, time and body")
		}
		if r.URL.Path == "/control/v1/Initialize" {
			_, _ = io.WriteString(w, `{"value":null}`)
			return
		}
		var envelope struct {
			Arguments []string `json:"arguments"`
		}
		if err = json.Unmarshal(body, &envelope); err != nil {
			t.Error(err)
		}
		var owner control.Owner
		if err = json.Unmarshal([]byte(envelope.Arguments[1]), &owner); err != nil || owner.Epoch != 9007199254740993 {
			t.Errorf("private ownership epoch was rounded: %+v %v", owner, err)
		}
		claims.Add(1)
		connection, _, err := w.(http.Hijacker).Hijack()
		if err != nil {
			t.Error(err)
			return
		}
		_ = connection.Close()
	}))
	defer server.Close()
	store, err := Open(context.Background(), server.URL, key, true)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	operation, err := store.ClaimDispatch(context.Background(), "session", control.Owner{Node: "owner", Epoch: 9007199254740993}, "stable-operation", "owner", "original-claim")
	if kernel.Response(err).Code != wire.ErrorCodeUncertain || operation.View.ID != "" {
		t.Fatalf("lost claim returned execution authority: %+v %v", operation, err)
	}
	if claims.Load() != 1 {
		t.Fatalf("claim transport resent %d times", claims.Load())
	}
}

func TestControlRedirectCannotAcquireAuthority(t *testing.T) {
	var redirected atomic.Int32
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		redirected.Add(1)
		_, _ = io.WriteString(w, `{"value":null}`)
	}))
	defer target.Close()
	redirect := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL, http.StatusTemporaryRedirect)
	}))
	defer redirect.Close()
	if store, err := Open(context.Background(), redirect.URL, make([]byte, 32), true); err == nil || store != nil {
		t.Fatal("redirected control bootstrap was accepted")
	}
	if redirected.Load() != 0 {
		t.Fatal("client followed a control redirect")
	}
}
