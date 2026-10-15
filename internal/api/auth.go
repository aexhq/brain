package api

import (
	"crypto/sha256"
	"net/http"
	"os"
	"strings"
	"sync"

	"github.com/aexhq/brain/internal/config"
	"github.com/aexhq/brain/internal/core"
)

type Auth struct {
	mu   sync.RWMutex
	keys map[[32]byte]core.Grant
}

func NewAuth(tokens []config.Token) *Auth {
	a := &Auth{keys: make(map[[32]byte]core.Grant)}
	for _, t := range tokens {
		a.keys[sha256.Sum256([]byte(os.Getenv(t.Env)))] = t.Grant
	}
	return a
}
func (a *Auth) Authenticate(r *http.Request) (core.Grant, bool) {
	value := r.Header.Get("Authorization")
	if !strings.HasPrefix(value, "Bearer ") || len(value) > 1024 {
		return core.Grant{}, false
	}
	key := sha256.Sum256([]byte(strings.TrimPrefix(value, "Bearer ")))
	a.mu.RLock()
	g, ok := a.keys[key]
	a.mu.RUnlock()
	return g, ok
}

// Delegate issues an ephemeral, read-only credential for one active tool call.
// The caller must revoke it when that invocation ends.
func (a *Auth) Delegate(session core.Session) (string, func()) {
	token := core.ID() + core.ID()
	key := sha256.Sum256([]byte(token))
	a.mu.Lock()
	a.keys[key] = core.Grant{Tenant: session.Tenant, Session: session.ID, ReadOnly: true}
	a.mu.Unlock()
	return token, func() { a.mu.Lock(); delete(a.keys, key); a.mu.Unlock() }
}
