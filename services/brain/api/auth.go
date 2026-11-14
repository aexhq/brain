package api

import (
	"crypto/sha256"
	"net/http"
	"os"
	"strings"
	"sync"
)

type Auth struct {
	Application func(*http.Request) (Grant, bool)
	mu          sync.RWMutex
	keys        map[[32]byte]Grant
}

func NewAuth(tokens []Token) *Auth {
	a := &Auth{keys: make(map[[32]byte]Grant)}
	for _, t := range tokens {
		a.keys[sha256.Sum256([]byte(os.Getenv(t.Env)))] = t.Grant
	}
	return a
}
func (a *Auth) Authenticate(r *http.Request) (Grant, bool) {
	value := r.Header.Get("Authorization")
	var g Grant
	var ok bool
	if strings.HasPrefix(value, "Bearer ") && len(value) <= 1024 {
		key := sha256.Sum256([]byte(strings.TrimPrefix(value, "Bearer ")))
		a.mu.RLock()
		g, ok = a.keys[key]
		a.mu.RUnlock()
	}
	if !ok && a.Application != nil {
		g, ok = a.Application(r)
		ok = ok && g.Tenant != "" && len(g.Tenant) <= 128 && (g.Session == "" || ValidSessionID(g.Session)) && g.Executor == ""
	}
	return g, ok
}

// Delegate issues an ephemeral, read-only credential for one active tool call.
// The caller must revoke it when that invocation ends.
func (a *Auth) Delegate(session Session) (string, func()) {
	token := ID() + ID()
	key := sha256.Sum256([]byte(token))
	a.mu.Lock()
	a.keys[key] = Grant{Tenant: session.Tenant, Session: session.ID, ReadOnly: true}
	a.mu.Unlock()
	return token, func() { a.mu.Lock(); delete(a.keys, key); a.mu.Unlock() }
}

// Token names a deployment credential and its authorization grant.
type Token struct {
	Env string `json:"token_env"`
	Grant
}
