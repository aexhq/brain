// Package credentials encrypts private binding values before they reach storage.
package credentials

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain/internal/credentialcipher"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
	"regexp"
)

var alias = regexp.MustCompile(`^[a-zA-Z][a-zA-Z0-9_]{0,63}$`)

func Seal(ctx context.Context, keys credentialcipher.Keys, s core.Session, inputs []core.CredentialInput) ([]credential.Record, error) {
	if len(inputs) == 0 {
		return nil, nil
	}
	if keys == nil {
		return nil, core.E(core.Unsupported, "private credentials are not configured")
	}
	allowed := map[string]bool{}
	for _, b := range s.Config.Bindings {
		if b.Provider != "" && alias.MatchString(b.Alias) {
			allowed[b.Alias] = true
		}
	}
	result := []credential.Record{}
	for _, input := range inputs {
		if !allowed[input.Binding] || len(input.Value) == 0 || len(input.Value) > 64<<10 || !json.Valid(input.Value) {
			return nil, core.E(core.Invalid, "invalid or duplicate private credential binding")
		}
		delete(allowed, input.Binding)
		record, err := credentialcipher.Seal(ctx, keys, scope(s, input.Binding), input.Value)
		if err != nil {
			return nil, err
		}
		result = append(result, record)
	}
	return result, nil
}

func Open(ctx context.Context, keys credentialcipher.Keys, s core.Session, binding string) (json.RawMessage, error) {
	for _, record := range s.Credentials {
		if record.Binding != binding {
			continue
		}
		if record.Revoked {
			return nil, core.E(core.Forbidden, "binding credential is revoked")
		}
		value, err := credentialcipher.Open(ctx, keys, scope(s, binding), record)
		if err != nil {
			return nil, err
		}
		return value, nil
	}
	for _, required := range s.CredentialBindings {
		if required == binding {
			return nil, errors.New("private credential record is missing")
		}
	}
	return nil, nil
}

// Validate checks the complete private catalog, including missing recovery rows.
func Validate(ctx context.Context, keys credentialcipher.Keys, s core.Session) error {
	if err := credential.ValidateCatalog(s.CredentialBindings, s.Credentials); err != nil {
		return err
	}
	for _, binding := range s.CredentialBindings {
		if _, err := Open(ctx, keys, s, binding); err != nil && core.ErrorCode(err) != core.Forbidden {
			return err
		}
	}
	return nil
}

func scope(s core.Session, binding string) credentialcipher.Scope {
	return credentialcipher.Scope{Tenant: s.Tenant, Session: s.ID, Binding: binding}
}
