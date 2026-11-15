package store

import (
	"context"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
	"os"
	"path/filepath"
	"strings"
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
	m, staging, err := a.Restore(ctx, core.Tenant(ctx), id, ref, s.root)
	if err != nil {
		return m, "", err
	}
	name, err := filepath.Rel(s.root, staging)
	if err != nil {
		return m, "", err
	}
	if !restoreHandle(name) {
		return m, "", errors.New("restore staging is outside the storage namespace")
	}
	return m, name, nil
}
func restoreHandle(name string) bool {
	return filepath.IsLocal(name) && filepath.Base(name) == name && strings.HasPrefix(name, ".restore-")
}
func (s *Store) DiscardRestore(staging string) error {
	if !restoreHandle(staging) {
		return errors.New("invalid restore handle")
	}
	root, err := os.OpenRoot(s.root)
	if err != nil {
		return err
	}
	defer root.Close()
	return root.RemoveAll(staging)
}
func (s *Store) InstallRestore(ctx context.Context, session core.Session, staging string) error {
	if !restoreHandle(staging) {
		return errors.New("invalid restore handle")
	}
	absolute, err := s.Dir(core.SessionKey(session.Tenant, session.ID))
	if err != nil {
		return err
	}
	dir, err := filepath.Rel(s.root, absolute)
	if err != nil {
		return err
	}
	root, err := os.OpenRoot(s.root)
	if err != nil {
		return err
	}
	defer root.Close()
	info, err := root.Lstat(staging)
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return errors.New("restore staging is not a directory")
	}
	if err = root.MkdirAll(filepath.Dir(dir), 0700); err != nil {
		return err
	}
	backup := dir + "-replaced-" + core.ID()
	hadLocal := false
	if _, err = root.Stat(dir); err == nil {
		if err = root.Rename(dir, backup); err != nil {
			return err
		}
		hadLocal = true
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if err = root.Rename(staging, dir); err != nil {
		if hadLocal {
			err = errors.Join(err, root.Rename(backup, dir))
		}
		return err
	}
	if err = s.Import(ctx, session); err != nil {
		rollback := root.Rename(dir, staging)
		if rollback == nil && hadLocal {
			rollback = root.Rename(backup, dir)
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
