// Package adapters defines the storage operations shared by every backend.
package adapters

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"path"
	"strings"
)

var (
	ErrNotFound = errors.New("storage object not found")
	ErrConflict = errors.New("storage condition failed")
	ErrInvalid  = errors.New("invalid storage operation")
)

type Object struct {
	Key     string `json:"key"`
	Data    []byte `json:"data,omitempty"`
	Version string `json:"version"`
}

// Condition prevents a delayed replication write from replacing newer state.
// An empty condition is an unconditional write. CreateOnly and Version conflict.
type Condition struct {
	CreateOnly bool   `json:"create_only,omitempty"`
	Version    string `json:"version,omitempty"`
}

type Page struct {
	Objects []Object `json:"objects"`
	Next    string   `json:"next,omitempty"`
}

type Adapter interface {
	Get(context.Context, string) (Object, error)
	Put(context.Context, string, []byte, Condition) (Object, error)
	Delete(context.Context, string) error
	List(context.Context, string, string, int) (Page, error)
	Close() error
}

func ValidKey(key string) bool {
	if key == "" || len(key) > 2048 || path.Clean(key) != key || strings.ContainsAny(key, "\\\x00") || strings.HasPrefix(key, "/") {
		return false
	}
	for _, part := range strings.Split(key, "/") {
		if part == "." || part == ".." || len(part) > 180 || strings.ContainsAny(part, ":*?\"<>|") {
			return false
		}
	}
	return true
}

func Validate(key string, data []byte, condition Condition) error {
	if !ValidKey(key) || len(data) > 2<<20 || (condition.CreateOnly && condition.Version != "") {
		return ErrInvalid
	}
	return nil
}

func Check(old Object, found bool, condition Condition) error {
	if condition.CreateOnly && found {
		return ErrConflict
	}
	if condition.Version != "" && (!found || old.Version != condition.Version) {
		return ErrConflict
	}
	return nil
}

func Version() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic(err)
	}
	return hex.EncodeToString(b[:])
}

func ValidateList(prefix, after string, limit int) error {
	if limit < 1 || limit > 1000 || (prefix != "" && !ValidKey(strings.TrimSuffix(prefix, "/"))) || (after != "" && !ValidKey(after)) {
		return ErrInvalid
	}
	return nil
}
