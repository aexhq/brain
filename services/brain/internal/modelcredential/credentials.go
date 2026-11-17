// Package modelcredential owns persisted model overrides and their namespace.
package modelcredential

import (
	"encoding/json"
	"errors"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
)

const Binding = "$model"

func Prepare(value *core.ModelCredential) (credential.Record, error) {
	b, err := json.Marshal(value)
	if err != nil {
		return credential.Record{}, err
	}
	if len(b) > 64<<10 {
		return credential.Record{}, core.E(core.Invalid, "model credential is too large")
	}
	return credential.Record{Binding: Binding, Value: b}, nil
}

func Resolve(s core.Session) (*core.ModelCredential, error) {
	for _, record := range s.Credentials {
		if record.Binding != Binding {
			continue
		}
		if record.Revoked {
			return nil, core.E(core.Forbidden, "model credential is revoked")
		}
		var value core.ModelCredential
		if err := json.Unmarshal(record.Value, &value); err != nil {
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
