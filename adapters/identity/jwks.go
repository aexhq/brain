package identity

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rsa"
	"crypto/tls"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"math/big"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/aexhq/brain/adapters/network"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/golang-jwt/jwt/v5"
)

type verificationKey struct {
	algorithm string
	public    any
}

type issuer struct {
	name, audience, endpoint, tenant, tenantClaim string
	permissions                                   []string
	maxToken, cache, refresh                      time.Duration
	client                                        *http.Client
	mu                                            sync.RWMutex
	keys                                          map[string]verificationKey
	cacheUntil, nextRefresh                       time.Time
	refreshing                                    chan struct{}
}

type Verifier struct {
	issuers map[string]*issuer
	now     func() time.Time
}

func denied() error {
	return kernel.Fail(wire.ErrorCodeUnauthorized, "identity bearer authority is invalid or expired")
}

func duration(value *uint64, fallback, minimum, maximum uint64) (time.Duration, error) {
	n := fallback
	if value != nil {
		n = *value
	}
	if n < minimum || n > maximum {
		return 0, fmt.Errorf("identity duration is outside its bounded configuration")
	}
	return time.Duration(n) * time.Millisecond, nil
}

func New(configurations []wire.IdentityIssuer, development bool) (*Verifier, error) {
	if len(configurations) > 16 {
		return nil, fmt.Errorf("at most sixteen identity issuers may be installed")
	}
	v := &Verifier{issuers: map[string]*issuer{}, now: time.Now}
	for _, config := range configurations {
		source, err := url.Parse(config.Issuer)
		if err != nil || source.Scheme != "https" || source.Hostname() == "" || source.User != nil ||
			source.RawQuery != "" || source.Fragment != "" || len(config.Issuer) > 2048 ||
			len(config.Audience) == 0 || len(config.Audience) > 512 || v.issuers[config.Issuer] != nil {
			return nil, fmt.Errorf("identity issuer and audience must be explicit, bounded and unique")
		}
		if (config.Tenant == nil) == (config.TenantClaim == nil) ||
			(config.Tenant != nil && !kernel.Identifier(*config.Tenant)) ||
			(config.TenantClaim != nil && (len(*config.TenantClaim) == 0 || len(*config.TenantClaim) > 256)) {
			return nil, fmt.Errorf("identity issuer requires exactly one static tenant or signed tenant claim")
		}
		if len(config.Permissions) == 0 || len(config.Permissions) > 64 {
			return nil, fmt.Errorf("identity issuer requires one to sixty-four explicit permissions")
		}
		for _, permission := range config.Permissions {
			if !kernel.Identifier(permission) || permission == "operator" || strings.HasPrefix(permission, "operator:") {
				return nil, fmt.Errorf("identity permissions cannot include wildcard or operator authority")
			}
		}
		maxToken, err := duration(config.MaxTokenMs, 3600000, 1000, 86400000)
		if err != nil {
			return nil, err
		}
		cache, err := duration(config.CacheMs, 300000, 1000, 3600000)
		if err != nil {
			return nil, err
		}
		refresh, err := duration(config.RefreshMs, 5000, 1000, 60000)
		if err != nil || refresh > cache {
			return nil, fmt.Errorf("identity refresh interval must fit its bounded key cache lifetime")
		}
		client, err := keyClient(config.JwksUrl, development)
		if err != nil {
			return nil, err
		}
		v.issuers[config.Issuer] = &issuer{name: config.Issuer, audience: config.Audience, endpoint: config.JwksUrl,
			tenant: kernel.Value(config.Tenant), tenantClaim: kernel.Value(config.TenantClaim),
			permissions: slices.Clone(config.Permissions), maxToken: maxToken, cache: cache, refresh: refresh,
			client: client, keys: map[string]verificationKey{}, refreshing: make(chan struct{}, 1)}
	}
	return v, nil
}

func keyClient(endpoint string, development bool) (*http.Client, error) {
	u, err := url.Parse(endpoint)
	if err != nil || len(endpoint) > 4096 || u.Hostname() == "" || u.User != nil || u.Fragment != "" {
		return nil, fmt.Errorf("identity JWKS endpoint is invalid")
	}
	client := network.Client(5 * time.Second)
	address, parseError := netip.ParseAddr(u.Hostname())
	loopback := u.Hostname() == "localhost" || (parseError == nil && address.IsLoopback())
	if development && loopback && (u.Scheme == "http" || u.Scheme == "https") {
		if u.Hostname() == "localhost" {
			address = netip.MustParseAddr("127.0.0.1")
		}
		port := u.Port()
		if port == "" {
			port = "443"
			if u.Scheme == "http" {
				port = "80"
			}
		}
		dialer := net.Dialer{Timeout: 5 * time.Second, KeepAlive: -1}
		client.Transport = &http.Transport{DisableKeepAlives: true, DisableCompression: true,
			MaxResponseHeaderBytes: 32768, ResponseHeaderTimeout: 5 * time.Second, TLSHandshakeTimeout: 5 * time.Second,
			TLSClientConfig: &tls.Config{MinVersion: tls.VersionTLS12},
			DialContext: func(ctx context.Context, kind, _ string) (net.Conn, error) {
				return dialer.DialContext(ctx, kind, net.JoinHostPort(address.String(), port))
			}}
	} else if u.Scheme != "https" {
		return nil, fmt.Errorf("identity JWKS requires HTTPS; development HTTP is loopback-only")
	} else if _, err := network.ValidateURL(endpoint); err != nil {
		return nil, fmt.Errorf("identity JWKS destination is not admitted")
	}
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return client, nil
}

func (v *Verifier) Authenticate(ctx context.Context, encoded string) (kernel.Principal, error) {
	if len(encoded) == 0 || len(encoded) > 16384 {
		return kernel.Principal{}, denied()
	}
	untrusted := jwt.MapClaims{}
	if _, _, err := jwt.NewParser(jwt.WithJSONNumber()).ParseUnverified(encoded, untrusted); err != nil {
		return kernel.Principal{}, denied()
	}
	name, _ := untrusted["iss"].(string)
	configured := v.issuers[name]
	if configured == nil {
		return kernel.Principal{}, denied()
	}
	claims := jwt.MapClaims{}
	parser := jwt.NewParser(jwt.WithValidMethods([]string{"RS256", "ES256"}), jwt.WithIssuer(configured.name),
		jwt.WithAudience(configured.audience), jwt.WithExpirationRequired(), jwt.WithIssuedAt(), jwt.WithJSONNumber(), jwt.WithTimeFunc(v.now))
	token, err := parser.ParseWithClaims(encoded, claims, func(token *jwt.Token) (any, error) {
		for _, name := range []string{"jku", "jwk", "x5u", "crit"} {
			if _, exists := token.Header[name]; exists {
				return nil, denied()
			}
		}
		kid, ok := token.Header["kid"].(string)
		if !ok || len(kid) == 0 || len(kid) > 256 {
			return nil, denied()
		}
		key, err := configured.key(ctx, kid, v.now)
		if err != nil || key.algorithm != token.Method.Alg() {
			return nil, denied()
		}
		return key.public, nil
	})
	if err != nil || !token.Valid {
		return kernel.Principal{}, denied()
	}
	issued, issuedError := claims.GetIssuedAt()
	expires, expiryError := claims.GetExpirationTime()
	subject, subjectError := claims.GetSubject()
	if issuedError != nil || expiryError != nil || subjectError != nil || issued == nil || expires == nil ||
		expires.UnixMilli() <= 0 || issued.UnixMilli() <= 0 || !expires.After(issued.Time) ||
		expires.Sub(issued.Time) > configured.maxToken || len(subject) == 0 || len(subject) > 512 {
		return kernel.Principal{}, denied()
	}
	tenant := configured.tenant
	if configured.tenantClaim != "" {
		tenant, _ = claims[configured.tenantClaim].(string)
	}
	if !kernel.Identifier(tenant) {
		return kernel.Principal{}, denied()
	}
	return kernel.Principal{Tenant: tenant, Subject: "identity:" + kernel.ID(configured.name, subject),
		Permissions: slices.Clone(configured.permissions), Issuer: configured.name, Expires: expires.UnixMilli()}, nil
}

func (i *issuer) cached(kid string, now time.Time) (verificationKey, bool) {
	i.mu.RLock()
	defer i.mu.RUnlock()
	key, exists := i.keys[kid]
	return key, exists && now.Before(i.cacheUntil)
}

func (i *issuer) key(ctx context.Context, kid string, now func() time.Time) (verificationKey, error) {
	if key, ok := i.cached(kid, now()); ok {
		return key, nil
	}
	select {
	case i.refreshing <- struct{}{}:
		defer func() { <-i.refreshing }()
	case <-ctx.Done():
		return verificationKey{}, denied()
	}
	if key, ok := i.cached(kid, now()); ok {
		return key, nil
	}
	i.mu.Lock()
	if now().Before(i.nextRefresh) {
		i.mu.Unlock()
		return verificationKey{}, denied()
	}
	i.nextRefresh = now().Add(i.refresh)
	i.mu.Unlock()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, i.endpoint, nil)
	if err != nil {
		return verificationKey{}, denied()
	}
	request.Header.Set("Accept", "application/json")
	response, err := i.client.Do(request)
	if err != nil {
		return verificationKey{}, denied()
	}
	if response.StatusCode != http.StatusOK {
		response.Body.Close()
		return verificationKey{}, denied()
	}
	body, err := network.Body(response, 256<<10)
	if err != nil {
		return verificationKey{}, denied()
	}
	keys, err := decodeKeys(body)
	if err != nil {
		return verificationKey{}, denied()
	}
	i.mu.Lock()
	i.keys, i.cacheUntil = keys, now().Add(i.cache)
	i.mu.Unlock()
	if key, ok := keys[kid]; ok {
		return key, nil
	}
	return verificationKey{}, denied()
}

type jwk struct {
	Kid string   `json:"kid"`
	Kty string   `json:"kty"`
	Alg string   `json:"alg"`
	Use string   `json:"use"`
	Ops []string `json:"key_ops"`
	N   string   `json:"n"`
	E   string   `json:"e"`
	Crv string   `json:"crv"`
	X   string   `json:"x"`
	Y   string   `json:"y"`
	D   string   `json:"d"`
}

func decodeKeys(body []byte) (map[string]verificationKey, error) {
	var document struct {
		Keys []jwk `json:"keys"`
	}
	if err := json.Unmarshal(body, &document); err != nil || len(document.Keys) == 0 || len(document.Keys) > 64 {
		return nil, denied()
	}
	keys, seen := map[string]verificationKey{}, map[string]bool{}
	for _, item := range document.Keys {
		if len(item.Kid) == 0 || len(item.Kid) > 256 || seen[item.Kid] || item.D != "" {
			return nil, denied()
		}
		seen[item.Kid] = true
		if (item.Use != "" && item.Use != "sig") || (item.Ops != nil && !slices.Contains(item.Ops, "verify")) {
			continue
		}
		switch item.Kty {
		case "RSA":
			if item.Alg != "" && item.Alg != "RS256" {
				continue
			}
			n, err := base64.RawURLEncoding.DecodeString(item.N)
			e, exponentError := base64.RawURLEncoding.DecodeString(item.E)
			if err != nil || exponentError != nil || len(n) < 256 || len(n) > 512 || len(e) == 0 || len(e) > 4 {
				return nil, denied()
			}
			modulus := new(big.Int).SetBytes(n)
			exponent := new(big.Int).SetBytes(e).Int64()
			if modulus.BitLen() < 2048 || modulus.BitLen() > 4096 || exponent < 3 || exponent > 2147483647 || exponent%2 == 0 {
				return nil, denied()
			}
			keys[item.Kid] = verificationKey{"RS256", &rsa.PublicKey{N: modulus, E: int(exponent)}}
		case "EC":
			if item.Crv != "P-256" || (item.Alg != "" && item.Alg != "ES256") {
				continue
			}
			x, xError := base64.RawURLEncoding.DecodeString(item.X)
			y, yError := base64.RawURLEncoding.DecodeString(item.Y)
			if xError != nil || yError != nil || len(x) != 32 || len(y) != 32 {
				return nil, denied()
			}
			public := &ecdsa.PublicKey{Curve: elliptic.P256(), X: new(big.Int).SetBytes(x), Y: new(big.Int).SetBytes(y)}
			if !public.Curve.IsOnCurve(public.X, public.Y) {
				return nil, denied()
			}
			keys[item.Kid] = verificationKey{"ES256", public}
		}
	}
	if len(keys) == 0 {
		return nil, denied()
	}
	return keys, nil
}
