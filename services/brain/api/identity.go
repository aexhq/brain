package api

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"strings"
	"unicode/utf8"
)

type tenantContext struct{}

func WithTenant(ctx context.Context, tenant string) context.Context {
	return context.WithValue(ctx, tenantContext{}, tenant)
}
func Tenant(ctx context.Context) string { v, _ := ctx.Value(tenantContext{}).(string); return v }

func ValidSessionID(id string) bool {
	if len(id) == 0 || len(id) > 128 || !utf8.ValidString(id) || strings.TrimSpace(id) == "" {
		return false
	}
	for _, r := range id {
		if r < 32 || r == 127 {
			return false
		}
	}
	return true
}

func ValidObjectReference(ref string) bool {
	if len(ref) != 64 {
		return false
	}
	_, err := hex.DecodeString(ref)
	return err == nil
}

// SessionKey prevents equal external IDs in different tenants from sharing a
// cache entry or database row. Each component is independently encoded.
func SessionKey(tenant, id string) string {
	return base64.RawURLEncoding.EncodeToString([]byte(tenant)) + "." + base64.RawURLEncoding.EncodeToString([]byte(id))
}
func SplitSessionKey(key string) (string, string, error) {
	t, i, ok := strings.Cut(key, ".")
	if !ok {
		return "", "", E(Invalid, "invalid session key")
	}
	tb, e := base64.RawURLEncoding.DecodeString(t)
	if e != nil {
		return "", "", E(Invalid, "invalid session key")
	}
	ib, e := base64.RawURLEncoding.DecodeString(i)
	if e != nil || len(tb) == 0 || !ValidSessionID(string(ib)) {
		return "", "", E(Invalid, "invalid session key")
	}
	return string(tb), string(ib), nil
}

// SessionDirectory maps untrusted identifiers to bounded, platform-safe names.
// The full original tenant and ID remain in the authoritative metadata record.
func SessionDirectory(key string) string {
	h := sha256.Sum256([]byte(key))
	return hex.EncodeToString(h[:])
}
