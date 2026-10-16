// Package store owns local metadata and append-only event files.
package store

import (
	"context"
	"errors"
	"os"
	"path/filepath"

	"github.com/aexhq/brain/internal/core"
)

// Metadata is the authoritative local session database, not an eventual index.
// Writes are durable on return. Save and Import must never change a tenant.
// List and Scan return ascending IDs strictly after the supplied cursor.
// Missing Get/Save returns core.NotFound; conflicting Import returns core.Forbidden.
// Implementations must support concurrent calls and context cancellation.
type Metadata interface {
	Create(context.Context, core.Session) error
	Get(context.Context, string) (core.Session, error)
	Save(context.Context, core.Session) error
	List(context.Context, string, string, int) ([]core.Session, error)
	Scan(context.Context, string, int) ([]core.Session, error)
	Import(context.Context, core.Session) error
	Close() error
}

// Store combines a local filesystem workspace with its metadata adapter.
// Close closes the owned metadata adapter; it does not delete session files.
type Store struct {
	Metadata
	root string
}

func Open(root string) (*Store, error) {
	if err := os.MkdirAll(root, 0700); err != nil {
		return nil, err
	}
	db, err := OpenSQLite(filepath.Join(root, "metadata.sqlite"))
	if err != nil {
		return nil, err
	}
	s, err := New(root, db)
	if err != nil {
		db.Close()
	}
	return s, err
}

// New takes ownership of metadata on success. The adapter stores records only;
// native state, transcripts, and staged recovery always remain local files.
func New(root string, metadata Metadata) (*Store, error) {
	if metadata == nil {
		return nil, errors.New("metadata adapter is required")
	}
	root, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	if err := os.MkdirAll(root, 0700); err != nil {
		return nil, err
	}
	return &Store{Metadata: metadata, root: root}, nil
}

func (s *Store) Root() string { return s.root }
func (s *Store) Dir(id string) (string, error) {
	if !core.ValidID(id) {
		return "", core.E(core.Invalid, "invalid session id")
	}
	return filepath.Join(s.root, "sessions", id), nil
}

func (s *Store) Create(ctx context.Context, session core.Session) error {
	dir, err := s.Dir(session.ID)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(dir, "native"), 0700); err != nil {
		return err
	}
	return s.Metadata.Create(ctx, session)
}
