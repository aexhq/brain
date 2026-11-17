// Package credentials validates and resolves private tool binding values.
package credentials

import (
	"encoding/json"
	"errors"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
	"regexp"
)

var alias = regexp.MustCompile(`^[a-zA-Z][a-zA-Z0-9_]{0,63}$`)

func Prepare(s core.Session, inputs []core.CredentialInput) ([]credential.Record, error) {
	if len(inputs) == 0 {
		return nil, nil
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
		result = append(result, credential.Record{Binding: input.Binding, Value: append(json.RawMessage(nil), input.Value...)})
	}
	return result, nil
}

func Resolve(s core.Session, binding string) (json.RawMessage, error) {
	for _, record := range s.Credentials {
		if record.Binding != binding {
			continue
		}
		if record.Revoked {
			return nil, core.E(core.Forbidden, "binding credential is revoked")
		}
		if !json.Valid(record.Value) {
			return nil, errors.New("invalid private credential value")
		}
		return record.Value, nil
	}
	for _, required := range s.CredentialBindings {
		if required == binding {
			return nil, errors.New("private credential record is missing")
		}
	}
	return nil, nil
}

// Validate checks the complete private catalog, including missing recovery rows.
func Validate(s core.Session) error {
	if err := credential.ValidateCatalog(s.CredentialBindings, s.Credentials); err != nil {
		return err
	}
	for _, binding := range s.CredentialBindings {
		if _, err := Resolve(s, binding); err != nil && core.ErrorCode(err) != core.Forbidden {
			return err
		}
	}
	return nil
}
