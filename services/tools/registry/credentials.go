package tools

import (
	"context"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api/credential"
	"github.com/aexhq/brain/services/tools/internal/credentials"
)

// CredentialSession projects only provider credential records. Model credentials
// and their catalog never cross the Tools service boundary.
func CredentialSession(s core.Session) core.Session {
	allowed := map[string]bool{}
	for _, binding := range s.Config.Bindings {
		if binding.Provider != "" && toolName.MatchString(binding.Alias) {
			allowed[binding.Alias] = true
		}
	}
	bindings, records := s.CredentialBindings, s.Credentials
	s.CredentialBindings, s.Credentials = nil, nil
	for _, binding := range bindings {
		if allowed[binding] {
			s.CredentialBindings = append(s.CredentialBindings, binding)
		}
	}
	for _, record := range records {
		if allowed[record.Binding] {
			s.Credentials = append(s.Credentials, record)
		}
	}
	return s
}

func (r *Registry) SealCredentials(ctx context.Context, s core.Session, inputs []core.CredentialInput) ([]credential.Record, error) {
	return credentials.Seal(ctx, r.Keys, CredentialSession(s), inputs)
}

func (r *Registry) ValidateCredentials(ctx context.Context, s core.Session) error {
	return credentials.Validate(ctx, r.Keys, CredentialSession(s))
}

func (r *Registry) RotateCredentials(ctx context.Context, s core.Session) ([]credential.Record, error) {
	s = CredentialSession(s)
	if err := credential.ValidateCatalog(s.CredentialBindings, s.Credentials); err != nil {
		return nil, err
	}
	inputs := []core.CredentialInput{}
	var revoked []credential.Record
	for _, record := range s.Credentials {
		if record.Revoked {
			revoked = append(revoked, record)
			continue
		}
		value, err := credentials.Open(ctx, r.Keys, s, record.Binding)
		if err != nil {
			return nil, err
		}
		inputs = append(inputs, core.CredentialInput{Binding: record.Binding, Value: value})
	}
	sealed, err := credentials.Seal(ctx, r.Keys, s, inputs)
	return append(sealed, revoked...), err
}
