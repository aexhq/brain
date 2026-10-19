// Package credentials encrypts private binding values before they reach storage.
package credentials

import (
	"context"
	"crypto/aes"
	"crypto/cipher"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain/internal/core"
)

// Keys is owned by the application, independently of session storage and archives.
// AES-256 keys must remain available for the lifetime of retained credentials.
type Keys interface {
	Active(context.Context) (string, []byte, error)
	Get(context.Context, string) ([]byte, error)
}

// ModelBinding is a private storage namespace, outside the tool alias syntax.
const ModelBinding = "$model"

func aead(key []byte) (cipher.AEAD, error) {
	if len(key) != 32 {
		return nil, errors.New("credential key must contain 32 bytes")
	}
	b, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	return cipher.NewGCMWithRandomNonce(b)
}

func ownership(s core.Session, binding string) []byte {
	b, _ := json.Marshal([]string{s.Tenant, s.ID, binding})
	return b
}

func Seal(ctx context.Context, keys Keys, s core.Session, inputs []core.CredentialInput) ([]core.CredentialRecord, error) {
	if len(inputs) == 0 {
		return nil, nil
	}
	if keys == nil {
		return nil, core.E(core.Unsupported, "private credentials are not configured")
	}
	id, key, err := keys.Active(ctx)
	if err != nil {
		return nil, errors.New("credential key unavailable")
	}
	if id == "" {
		return nil, errors.New("credential key identity is required")
	}
	c, err := aead(key)
	if err != nil {
		return nil, err
	}
	allowed := map[string]bool{}
	for _, binding := range s.CredentialBindings {
		if binding == ModelBinding {
			allowed[binding] = true
		}
	}
	for _, b := range s.Config.Bindings {
		if b.Provider != "" {
			allowed[b.Alias] = true
		}
	}
	result := []core.CredentialRecord{}
	for _, input := range inputs {
		if !allowed[input.Binding] || len(input.Value) == 0 || len(input.Value) > 64<<10 || !json.Valid(input.Value) {
			return nil, core.E(core.Invalid, "invalid or duplicate private credential binding")
		}
		delete(allowed, input.Binding)
		result = append(result, core.CredentialRecord{Binding: input.Binding, Key: id, Ciphertext: c.Seal(nil, nil, input.Value, ownership(s, input.Binding))})
	}
	return result, nil
}

func Open(ctx context.Context, keys Keys, s core.Session, binding string) (json.RawMessage, error) {
	for _, record := range s.Credentials {
		if record.Binding != binding {
			continue
		}
		if record.Revoked {
			return nil, core.E(core.Forbidden, "binding credential is revoked")
		}
		if keys == nil {
			return nil, errors.New("credential keys unavailable")
		}
		key, err := keys.Get(ctx, record.Key)
		if err != nil {
			return nil, errors.New("credential key unavailable")
		}
		c, err := aead(key)
		if err != nil {
			return nil, err
		}
		value, err := c.Open(nil, nil, record.Ciphertext, ownership(s, binding))
		if err != nil {
			return nil, errors.New("credential authentication failed")
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
func Validate(ctx context.Context, keys Keys, s core.Session) error {
	if len(s.CredentialBindings) != len(s.Credentials) {
		return errors.New("private credential catalog is incomplete")
	}
	seen := map[string]bool{}
	for _, binding := range s.CredentialBindings {
		if seen[binding] {
			return errors.New("duplicate private credential binding")
		}
		seen[binding] = true
		if _, err := Open(ctx, keys, s, binding); err != nil && core.ErrorCode(err) != core.Forbidden {
			return err
		}
	}
	for _, record := range s.Credentials {
		if !seen[record.Binding] {
			return errors.New("unexpected private credential record")
		}
		delete(seen, record.Binding)
	}
	return nil
}
