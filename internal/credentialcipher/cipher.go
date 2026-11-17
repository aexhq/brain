// Package credentialcipher provides authenticated encryption without session or
// provider policy. Callers own authorization and credential lifecycle.
package credentialcipher

import (
	"context"
	"crypto/aes"
	"crypto/cipher"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain/services/storage/api/credential"
)

// Keys remain independent of session storage and archives.
type Keys interface {
	Active(context.Context) (string, []byte, error)
	Get(context.Context, string) ([]byte, error)
}

type Scope struct{ Tenant, Session, Binding string }

func (s Scope) ownership() []byte {
	b, _ := json.Marshal([]string{s.Tenant, s.Session, s.Binding})
	return b
}

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

func Seal(ctx context.Context, keys Keys, scope Scope, value []byte) (credential.Record, error) {
	if keys == nil {
		return credential.Record{}, errors.New("private credentials are not configured")
	}
	id, key, err := keys.Active(ctx)
	if err != nil {
		return credential.Record{}, errors.New("credential key unavailable")
	}
	if id == "" {
		return credential.Record{}, errors.New("credential key identity is required")
	}
	c, err := aead(key)
	if err != nil {
		return credential.Record{}, err
	}
	return credential.Record{Binding: scope.Binding, Key: id, Ciphertext: c.Seal(nil, nil, value, scope.ownership())}, nil
}

func Open(ctx context.Context, keys Keys, scope Scope, record credential.Record) ([]byte, error) {
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
	value, err := c.Open(nil, nil, record.Ciphertext, scope.ownership())
	if err != nil {
		return nil, errors.New("credential authentication failed")
	}
	return value, nil
}
