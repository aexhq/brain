package identity

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"math/big"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/golang-jwt/jwt/v5"
)

func publicRSA(key *rsa.PrivateKey, kid string) jwk {
	return jwk{Kid: kid, Kty: "RSA", Alg: "RS256", Use: "sig",
		N: base64.RawURLEncoding.EncodeToString(key.N.Bytes()),
		E: base64.RawURLEncoding.EncodeToString(big.NewInt(int64(key.E)).Bytes())}
}

func publicEC(key *ecdsa.PrivateKey, kid string) jwk {
	return jwk{Kid: kid, Kty: "EC", Alg: "ES256", Use: "sig", Crv: "P-256",
		X: base64.RawURLEncoding.EncodeToString(key.X.FillBytes(make([]byte, 32))),
		Y: base64.RawURLEncoding.EncodeToString(key.Y.FillBytes(make([]byte, 32)))}
}

func signed(t *testing.T, method jwt.SigningMethod, key any, kid string, claims jwt.MapClaims) string {
	t.Helper()
	token := jwt.NewWithClaims(method, claims)
	token.Header["kid"] = kid
	value, err := token.SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	return value
}

func TestConfiguredJWKSAuthenticatesBothAlgorithmsWithoutRoleAmplification(t *testing.T) {
	rsaKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	ecKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	var requests atomic.Int64
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		if r.Method != http.MethodGet || r.URL.Path != "/keys" {
			t.Error("JWKS destination was not the configured read-only endpoint")
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"keys": []jwk{publicRSA(rsaKey, "rsa"), publicEC(ecKey, "ec")}})
	}))
	defer server.Close()
	config := wire.IdentityIssuer{Issuer: "https://issuer.example", Audience: "brain", JwksUrl: server.URL + "/keys",
		TenantClaim: kernel.Ptr("tenant_id"), Permissions: []string{"session:create", "session:read"}}
	verifier, err := New([]wire.IdentityIssuer{config}, true)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().Truncate(time.Second)
	base := jwt.MapClaims{"iss": config.Issuer, "aud": config.Audience, "sub": "user@example.test", "tenant_id": "tenant-a",
		"iat": now.Unix(), "exp": now.Add(time.Minute).Unix(), "operator": true, "scope": "*", "roles": []string{"operator"}}
	for _, value := range []string{signed(t, jwt.SigningMethodRS256, rsaKey, "rsa", base), signed(t, jwt.SigningMethodES256, ecKey, "ec", base)} {
		principal, err := verifier.Authenticate(context.Background(), value)
		if err != nil || principal.Tenant != "tenant-a" || !kernel.Identifier(principal.Subject) ||
			principal.Operator || principal.Allows("", "operator") || principal.Allows("", "session:delete") ||
			!principal.Allows("", "session:create") || principal.Expires != now.Add(time.Minute).UnixMilli() {
			t.Fatalf("configured authority was not preserved: %+v %v", principal, err)
		}
	}
	for name, changes := range map[string]jwt.MapClaims{
		"unknown issuer": {"iss": "https://untrusted.example"},
		"wrong audience": {"aud": "another-service"},
		"expired":        {"exp": now.Add(-time.Second).Unix()},
		"no expiry":      {"exp": nil},
		"no issue time":  {"iat": nil},
		"future issue":   {"iat": now.Add(time.Minute).Unix(), "exp": now.Add(2 * time.Minute).Unix()},
		"too long":       {"iat": now.Add(-2 * time.Hour).Unix()},
		"not active":     {"nbf": now.Add(time.Minute).Unix()},
		"wrong tenant":   {"tenant_id": []string{"tenant-a"}},
		"empty subject":  {"sub": ""},
	} {
		t.Run(name, func(t *testing.T) {
			claims := jwt.MapClaims{}
			for key, value := range base {
				claims[key] = value
			}
			for key, value := range changes {
				if value == nil {
					delete(claims, key)
				} else {
					claims[key] = value
				}
			}
			if _, err := verifier.Authenticate(context.Background(), signed(t, jwt.SigningMethodRS256, rsaKey, "rsa", claims)); kernel.Response(err).Code != wire.ErrorCodeUnauthorized {
				t.Fatalf("invalid identity was authenticated: %v", err)
			}
		})
	}
	for _, value := range []string{
		signed(t, jwt.SigningMethodHS256, []byte("not-an-asymmetric-verification-key"), "rsa", base),
		signed(t, jwt.SigningMethodRS256, rsaKey, "ec", base),
		strings.Repeat("x", 16385),
	} {
		if _, err := verifier.Authenticate(context.Background(), value); kernel.Response(err).Code != wire.ErrorCodeUnauthorized {
			t.Fatalf("algorithm confusion or oversized input authenticated: %v", err)
		}
	}
	injected := jwt.NewWithClaims(jwt.SigningMethodRS256, base)
	injected.Header["kid"], injected.Header["jku"] = "rsa", server.URL+"/untrusted"
	value, _ := injected.SignedString(rsaKey)
	if _, err := verifier.Authenticate(context.Background(), value); kernel.Response(err).Code != wire.ErrorCodeUnauthorized {
		t.Fatal("token-controlled key discovery was accepted")
	}
	if requests.Load() != 1 {
		t.Fatalf("cached verification or invalid claims triggered key discovery: %d", requests.Load())
	}
}

func TestJWKSRotationIsBoundedAndExpiredCacheCannotAuthorize(t *testing.T) {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	var requests atomic.Int64
	var published atomic.Value
	published.Store(kernel.JSON(map[string]any{"keys": []jwk{publicEC(key, "original")}}))
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		requests.Add(1)
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write(published.Load().(json.RawMessage))
	}))
	defer server.Close()
	config := wire.IdentityIssuer{Issuer: "https://rotation.example", Audience: "brain", JwksUrl: server.URL,
		Tenant: kernel.Ptr("fixed-tenant"), Permissions: []string{"session:read"}, CacheMs: kernel.Ptr(uint64(2000)), RefreshMs: kernel.Ptr(uint64(1000))}
	verifier, err := New([]wire.IdentityIssuer{config}, true)
	if err != nil {
		t.Fatal(err)
	}
	var clock atomic.Int64
	clock.Store(time.Now().Truncate(time.Second).UnixMilli())
	verifier.now = func() time.Time { return time.UnixMilli(clock.Load()) }
	claims := jwt.MapClaims{"iss": config.Issuer, "aud": "brain", "sub": "reader",
		"iat": verifier.now().Unix(), "exp": verifier.now().Add(time.Minute).Unix(), "tenant": "ignored"}
	original := signed(t, jwt.SigningMethodES256, key, "original", claims)
	if p, err := verifier.Authenticate(context.Background(), original); err != nil || p.Tenant != "fixed-tenant" {
		t.Fatalf("static tenant mapping failed: %+v %v", p, err)
	}
	rotated := signed(t, jwt.SigningMethodES256, key, "rotated", claims)
	if _, err := verifier.Authenticate(context.Background(), rotated); kernel.Response(err).Code != wire.ErrorCodeUnauthorized {
		t.Fatal("unknown key identifier was accepted")
	}
	if requests.Load() != 1 {
		t.Fatal("unknown key bypassed the refresh-rate bound")
	}
	published.Store(kernel.JSON(map[string]any{"keys": []jwk{publicEC(key, "rotated")}}))
	clock.Add(1001)
	var group sync.WaitGroup
	for range 16 {
		group.Go(func() {
			if _, err := verifier.Authenticate(context.Background(), rotated); err != nil {
				t.Errorf("concurrent rotated key verification failed: %v", err)
			}
		})
	}
	group.Wait()
	if requests.Load() != 2 {
		t.Fatalf("key rotation caused concurrent unbounded refresh: %d", requests.Load())
	}
	clock.Add(2001)
	published.Store(json.RawMessage(`{"keys":[]}`))
	if _, err := verifier.Authenticate(context.Background(), rotated); kernel.Response(err).Code != wire.ErrorCodeUnauthorized {
		t.Fatal("expired verification cache survived a failed refresh")
	}
	if requests.Load() != 3 {
		t.Fatalf("failed refresh was retried: %d", requests.Load())
	}
}

func TestIdentityInstallationAndKeysFailClosed(t *testing.T) {
	valid := wire.IdentityIssuer{Issuer: "https://issuer.example", Audience: "brain", JwksUrl: "https://issuer.example/keys",
		Tenant: kernel.Ptr("tenant"), Permissions: []string{"session:read"}}
	for name, change := range map[string]func(*wire.IdentityIssuer){
		"public plaintext": func(c *wire.IdentityIssuer) { c.JwksUrl = "http://issuer.example/keys" },
		"private endpoint": func(c *wire.IdentityIssuer) { c.JwksUrl = "https://127.0.0.1/keys" },
		"operator scope":   func(c *wire.IdentityIssuer) { c.Permissions = []string{"operator"} },
		"wildcard scope":   func(c *wire.IdentityIssuer) { c.Permissions = []string{"*"} },
		"ambiguous tenant": func(c *wire.IdentityIssuer) { c.TenantClaim = kernel.Ptr("tenant_id") },
		"no tenant":        func(c *wire.IdentityIssuer) { c.Tenant = nil },
		"unbounded token":  func(c *wire.IdentityIssuer) { c.MaxTokenMs = kernel.Ptr(uint64(86400001)) },
		"busy refresh":     func(c *wire.IdentityIssuer) { c.RefreshMs = kernel.Ptr(uint64(1)) },
	} {
		t.Run(name, func(t *testing.T) {
			copy := valid
			change(&copy)
			if _, err := New([]wire.IdentityIssuer{copy}, false); err == nil {
				t.Fatal("unsafe identity installation was admitted")
			}
		})
	}
	if _, err := New([]wire.IdentityIssuer{valid, valid}, false); err == nil {
		t.Fatal("duplicate issuers were admitted")
	}
	key, _ := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	public := publicEC(key, "one")
	for _, keys := range [][]jwk{
		{}, {public, public}, {{Kid: "one", Kty: "EC", Crv: "P-256", X: "AA", Y: "AA"}},
		{{Kid: "private", Kty: "EC", D: "private-material"}},
	} {
		if _, err := decodeKeys(kernel.JSON(map[string]any{"keys": keys})); err == nil {
			t.Fatal("invalid or private JWKS was accepted")
		}
	}
}
