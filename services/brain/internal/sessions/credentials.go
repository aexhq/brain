package session

import (
	"context"
	"github.com/aexhq/brain/runtimes/host"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/brain/internal/modelcredential"
	"github.com/aexhq/brain/services/storage/api/credential"
)

func (e *Engine) modelProvider(ctx context.Context, s core.Session) (harness.Provider, error) {
	credential, err := modelcredential.Resolve(s)
	if err != nil {
		return harness.Provider{}, err
	}
	return e.opts.Provider(ctx, s, credential)
}

// ChangeCredentials serializes application credential administration with local
// metadata writes. The mutation must atomically update private rows and UpdatedAt.
func (e *Engine) ChangeCredentials(ctx context.Context, tenant, id string, change func(core.Session) error) error {
	ctx = core.WithTenant(ctx, tenant)
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
	e.dirty(tenant, id)
	return nil
}

func validateCredentialCatalog(s core.Session) error {
	if err := credential.ValidateCatalog(s.CredentialBindings, s.Credentials); err != nil {
		return err
	}
	allowed := map[string]bool{modelcredential.Binding: true}
	for _, binding := range s.Config.Bindings {
		if binding.Provider != "" {
			allowed[binding.Alias] = true
		}
	}
	for _, binding := range s.CredentialBindings {
		if !allowed[binding] {
			return core.E(core.Invalid, "unknown private credential binding")
		}
	}
	return nil
}

// ValidateCredentials checks values returned by Storage against domain rules.
func (e *Engine) ValidateCredentials(ctx context.Context, s core.Session) error {
	if err := validateCredentialCatalog(s); err != nil {
		return err
	}
	if _, err := modelcredential.Resolve(s); err != nil && core.ErrorCode(err) != core.Forbidden {
		return err
	}
	return e.opts.Tools.ValidateCredentials(ctx, s)
}
