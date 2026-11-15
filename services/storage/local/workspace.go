package store

import (
	"context"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
	"os"
	"path/filepath"
)

func (s *Store) OpenLog(ctx context.Context, session core.Session, maxBytes int64) (*Log, error) {
	dir, e := s.Dir(core.SessionKey(session.Tenant, session.ID))
	if e != nil {
		return nil, e
	}
	return OpenLog(filepath.Join(dir, "events.jsonl"), maxBytes)
}
func (s *Store) RemoveWorkingCopy(ctx context.Context, id string) error {
	dir, e := s.Dir(core.SessionKey(core.Tenant(ctx), id))
	if e != nil {
		return e
	}
	return os.RemoveAll(dir)
}
func (s *Store) Snapshot(ctx context.Context, session core.Session, size int64) (api.Snapshot, error) {
	dir, e := s.Dir(core.SessionKey(session.Tenant, session.ID))
	return api.Snapshot{Session: session, Directory: dir, HistoryBytes: size}, e
}
func (s *Store) StageRestore(ctx context.Context, a api.Checkpoints, id, ref string) (api.Manifest, string, error) {
	return a.Restore(ctx, core.Tenant(ctx), id, ref, s.root)
}
func (s *Store) DiscardRestore(staging string) error { return os.RemoveAll(staging) }
func (s *Store) InstallRestore(ctx context.Context, session core.Session, staging string) error {
	dir, err := s.Dir(core.SessionKey(session.Tenant, session.ID))
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(dir), 0700); err != nil {
		return err
	}
	backup := dir + "-replaced-" + core.ID()
	hadLocal := false
	if _, err := os.Stat(dir); err == nil {
		if err := os.Rename(dir, backup); err != nil {
			return err
		}
		hadLocal = true
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if err := os.Rename(staging, dir); err != nil {
		if hadLocal {
			err = errors.Join(err, os.Rename(backup, dir))
		}
		return err
	}
	if err := s.Import(ctx, session); err != nil {
		rollback := os.Rename(dir, staging)
		if rollback == nil && hadLocal {
			rollback = os.Rename(backup, dir)
		}
		return errors.Join(err, rollback)
	}
	// Retain replaced local data for operator recovery.
	return nil
}
func (s *Store) SyncState(ctx context.Context, session core.Session) (*api.Manifest, int64, error) {
	dir, e := s.Dir(core.SessionKey(session.Tenant, session.ID))
	if e != nil {
		return nil, 0, e
	}
	var size int64
	info, e := os.Stat(filepath.Join(dir, "events.jsonl"))
	if e == nil {
		size = info.Size()
	} else if !errors.Is(e, os.ErrNotExist) {
		return nil, 0, e
	}
	m, e := ReadState(dir)
	return m, size, e
}
