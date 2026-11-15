package store

import (
	"context"
	"errors"
	"fmt"
	core "github.com/aexhq/brain/services/brain/api"
	"os"
	"path/filepath"
)

func (s *Store) PrepareNative(ctx context.Context, session core.Session, restore bool) (string, error) {
	dir, err := s.Dir(core.SessionKey(session.Tenant, session.ID))
	if err != nil {
		return "", err
	}
	dir = filepath.Join(dir, "native")
	if restore {
		err = restoreNative(dir, session.Checkpoint)
	}
	return dir, err
}
func (s *Store) ValidateNative(ctx context.Context, session core.Session, files []core.NativeFile) error {
	dir, err := s.PrepareNative(ctx, session, false)
	if err != nil {
		return err
	}
	for _, f := range files {
		p, err := nativePath(dir, f.Path)
		if err != nil {
			return err
		}
		info, err := os.Lstat(p)
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() || info.Size() != f.Size {
			return errors.New("inconsistent native checkpoint")
		}
	}
	return nil
}
func nativePath(dir, name string) (string, error) {
	if name == "" || !filepath.IsLocal(filepath.FromSlash(name)) {
		return "", errors.New("invalid native artifact path")
	}
	return filepath.Join(dir, filepath.FromSlash(name)), nil
}

func restoreNative(dir string, cp *core.Checkpoint) error {
	if cp == nil {
		return errors.New("session has no usable checkpoint")
	}
	if len(cp.Files) == 0 {
		entries, err := os.ReadDir(dir)
		if err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
		}
		if len(entries) > 0 {
			if err := os.Rename(dir, dir+"-interrupted-"+core.ID()); err != nil {
				return err
			}
		}
		return os.MkdirAll(dir, 0700)
	}
	for _, f := range cp.Files {
		p, err := nativePath(dir, f.Path)
		if err != nil {
			return err
		}
		info, err := os.Lstat(p)
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() || info.Size() < f.Size {
			return fmt.Errorf("native state is incomplete: %s", f.Path)
		}
		if info.Size() != f.Size {
			if !f.AppendOnly {
				return errors.New("native state differs from saved checkpoint")
			}
			if err := os.Truncate(p, f.Size); err != nil {
				return err
			}
		}
	}
	return nil
}
