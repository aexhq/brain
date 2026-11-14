package credentials

import (
	"context"
	"encoding/base64"
	"errors"
	"os"
)

// KeyConfig names deployment environment variables, never encryption key values.
type KeyConfig struct {
	ActiveID     string            `json:"active"`
	Environments map[string]string `json:"keys"`
}

func (k *KeyConfig) Active(ctx context.Context) (string, []byte, error) {
	key, err := k.Get(ctx, k.ActiveID)
	return k.ActiveID, key, err
}
func (k *KeyConfig) Get(_ context.Context, id string) ([]byte, error) {
	env := k.Environments[id]
	if env == "" {
		return nil, errors.New("credential key is not configured")
	}
	key, err := base64.StdEncoding.DecodeString(os.Getenv(env))
	if err != nil || len(key) != 32 {
		return nil, errors.New("credential key must be base64-encoded 32 bytes")
	}
	return key, nil
}
