package brain

import (
	"context"
	"strings"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
	"github.com/aexhq/brain/internal/harness"
)

// ModelContext carries authenticated scope, never model-supplied arguments.
type ModelContext struct {
	Tenant, Session, Model string
	Credential             *ModelCredential
}

// ModelProvider installs endpoint policy and an optional startup key. Authorize
// applies to defaults and overrides on admission and before each turn. Resolve
// resolves references in the supplied tenant scope; errors never select defaults.
type ModelProvider struct {
	APIKey, BaseURL, API string
	Authorize            func(context.Context, ModelContext) error
	Resolve              func(context.Context, ModelContext) (string, error)
}

func NewModelProvider(apiKey string) ModelProvider { return ModelProvider{APIKey: apiKey} }

func (o Options) resolveModel(ctx context.Context, s core.Session, credential *core.ModelCredential) (harness.Provider, error) {
	name, _, _ := strings.Cut(s.Config.Model, "/")
	provider := o.Models[name]
	if credential != nil && ((credential.APIKey == "") == (credential.Reference == "") || len(credential.APIKey) > 64<<10 || len(credential.Reference) > 1024) {
		return harness.Provider{}, core.E(core.Invalid, "model credential requires exactly one API key or reference")
	}
	scope := ModelContext{Tenant: s.Tenant, Session: s.ID, Model: s.Config.Model, Credential: credential}
	if provider.Authorize != nil {
		if err := provider.Authorize(ctx, scope); err != nil {
			return harness.Provider{}, core.E(core.Forbidden, "model selection is not authorized")
		}
	}
	var key *string
	if credential != nil {
		value := credential.APIKey
		if credential.Reference != "" {
			if provider.Resolve == nil {
				return harness.Provider{}, core.E(core.Forbidden, "model credential reference is not authorized")
			}
			resolved, err := provider.Resolve(ctx, scope)
			if err != nil || resolved == "" {
				return harness.Provider{}, core.E(core.Forbidden, "model credential reference is unavailable")
			}
			value = resolved
		}
		key = &value
	}
	return o.Config.ModelProvider(s.Config.Model, key)
}

// RevokeModelCredential retains a recovery tombstone like tool revocation.
func (s *Server) RevokeModelCredential(ctx context.Context, tenant, sessionID string) error {
	return s.RevokeCredential(ctx, tenant, sessionID, credentials.ModelBinding)
}

// NewTool declares an ordinary application function using the common tool contract.
// Tenants explicitly selects its authority; an empty list fails startup validation.
func NewTool(descriptor Tool, tenants []string, run Handler) ManagedTool {
	return ManagedTool{Tool: descriptor, Tenants: tenants, Handler: run}
}

func APIKey(value string) *ModelCredential              { return &ModelCredential{APIKey: value} }
func CredentialReference(value string) *ModelCredential { return &ModelCredential{Reference: value} }
