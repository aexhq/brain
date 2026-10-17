package session

import (
	"context"
	"github.com/aexhq/brain/internal/core"
)

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
