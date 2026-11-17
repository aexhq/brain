// Package modelcredential owns persisted model overrides and their namespace.
package modelcredential

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain/internal/credentialcipher"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
)

const Binding = "$model"

func scope(s core.Session) credentialcipher.Scope {
	return credentialcipher.Scope{Tenant: s.Tenant, Session: s.ID, Binding: Binding}
}

func Seal(ctx context.Context, keys credentialcipher.Keys, s core.Session, value *core.ModelCredential) (credential.Record, error) {
	if keys == nil {
		return credential.Record{}, core.E(core.Unsupported, "private model credentials are not configured")
	}
	b, err := json.Marshal(value)
	if err != nil {
		return credential.Record{}, err
	}
	if len(b) > 64<<10 {
		return credential.Record{}, core.E(core.Invalid, "model credential is too large")
	}
	return credentialcipher.Seal(ctx, keys, scope(s), b)
}

func Open(ctx context.Context, keys credentialcipher.Keys, s core.Session) (*core.ModelCredential, error) {
	for _, record := range s.Credentials {
		if record.Binding != Binding {
			continue
		}
		if record.Revoked {
			return nil, core.E(core.Forbidden, "model credential is revoked")
		}
		b, err := credentialcipher.Open(ctx, keys, scope(s), record)
		if err != nil {
			return nil, err
		}
		var value core.ModelCredential
		if err := json.Unmarshal(b, &value); err != nil {
			return nil, err
		}
		return &value, nil
	}
	for _, binding := range s.CredentialBindings {
		if binding == Binding {
			return nil, errors.New("private model credential record is missing")
		}
	}
	return nil, nil
}
