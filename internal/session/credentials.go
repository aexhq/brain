package session

import (
	"context"
	"encoding/json"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
	"github.com/aexhq/brain/internal/harness"
)

func (e *Engine) modelProvider(ctx context.Context, s core.Session) (harness.Provider, error) {
	value, err := credentials.Open(ctx, e.opts.Keys, s, credentials.ModelBinding)
	if err != nil {
		return harness.Provider{}, err
	}
	var credential *core.ModelCredential
	if value != nil {
		credential = new(core.ModelCredential)
		if err := json.Unmarshal(value, credential); err != nil {
			return harness.Provider{}, err
		}
	}
	return e.opts.Provider(ctx, s, credential)
}

// ChangeCredentials serializes application credential administration with local
// metadata writes. The mutation must atomically update private rows and UpdatedAt.
func (e *Engine) ChangeCredentials(ctx context.Context, tenant, id string, change func(core.Session) error) error {
	if _, err := e.inspect(ctx, core.Grant{Tenant: tenant, Manage: true}, id); err != nil {
		return err
	}
	a, err := e.load(ctx, id)
	if err != nil {
		return err
	}
	defer e.release(a)
	a.mu.Lock()
	defer a.mu.Unlock()
	fresh, err := e.opts.Store.Get(ctx, id)
	if err != nil {
		return err
	}
	if err := change(fresh); err != nil {
		return err
	}
	fresh, err = e.opts.Store.Get(ctx, id)
	if err != nil {
		return err
	}
	a.meta.UpdatedAt = fresh.UpdatedAt
	a.meta.Credentials = fresh.Credentials
	e.dirty(id)
	return nil
}
