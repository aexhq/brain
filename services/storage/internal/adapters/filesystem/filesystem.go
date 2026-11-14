package filesystem

import (
	"context"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"

	"github.com/aexhq/brain/internal/volume"
	"github.com/aexhq/brain/services/storage/internal/adapters"
)

// Store is exclusively owned by one Storage process. Publication uses a synced
// temporary file and rename; readers never observe a partially written record.
type Store struct {
	owner    *os.File
	once     sync.Once
	closeErr error
	root     string
	mu       sync.Mutex
}

func New(root string) (*Store, error) {
	p, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	if err = os.MkdirAll(p, 0700); err != nil {
		return nil, err
	}
	owner, err := volume.Lock(p)
	if err != nil {
		return nil, err
	}
	return &Store{root: p, owner: owner}, nil
}
func (s *Store) filename(key string) string {
	return filepath.Join(s.root, filepath.FromSlash(key)+".object")
}
func (s *Store) get(key string) (adapters.Object, error) {
	b, err := os.ReadFile(s.filename(key))
	if errors.Is(err, os.ErrNotExist) {
		return adapters.Object{}, adapters.ErrNotFound
	}
	if err != nil {
		return adapters.Object{}, err
	}
	if len(b) < 33 || b[32] != '\n' {
		return adapters.Object{}, errors.New("corrupt storage object")
	}
	return adapters.Object{Key: key, Version: string(b[:32]), Data: b[33:]}, nil
}
func (s *Store) Get(ctx context.Context, key string) (adapters.Object, error) {
	if err := ctx.Err(); err != nil {
		return adapters.Object{}, err
	}
	if !adapters.ValidKey(key) {
		return adapters.Object{}, adapters.ErrInvalid
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.get(key)
}
func (s *Store) Put(ctx context.Context, key string, data []byte, c adapters.Condition) (adapters.Object, error) {
	if err := ctx.Err(); err != nil {
		return adapters.Object{}, err
	}
	if err := adapters.Validate(key, data, c); err != nil {
		return adapters.Object{}, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	old, err := s.get(key)
	if err != nil && !errors.Is(err, adapters.ErrNotFound) {
		return adapters.Object{}, err
	}
	if err := adapters.Check(old, err == nil, c); err != nil {
		return adapters.Object{}, err
	}
	p := s.filename(key)
	if err := os.MkdirAll(filepath.Dir(p), 0700); err != nil {
		return adapters.Object{}, err
	}
	f, err := os.CreateTemp(filepath.Dir(p), ".pending-")
	if err != nil {
		return adapters.Object{}, err
	}
	defer os.Remove(f.Name())
	v := adapters.Version()
	if _, err = f.Write(append([]byte(v+"\n"), data...)); err != nil {
		f.Close()
		return adapters.Object{}, err
	}
	if err = f.Sync(); err != nil {
		f.Close()
		return adapters.Object{}, err
	}
	if err = f.Close(); err != nil {
		return adapters.Object{}, err
	}
	if err = os.Rename(f.Name(), p); err != nil {
		return adapters.Object{}, err
	}
	if err = syncDirectory(filepath.Dir(p)); err != nil {
		return adapters.Object{}, err
	}
	return adapters.Object{Key: key, Version: v, Data: data}, nil
}
func (s *Store) Delete(ctx context.Context, key string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if !adapters.ValidKey(key) {
		return adapters.ErrInvalid
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	err := os.Remove(s.filename(key))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	return err
}
func (s *Store) List(ctx context.Context, prefix, after string, limit int) (adapters.Page, error) {
	if err := adapters.ValidateList(prefix, after, limit); err != nil {
		return adapters.Page{}, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	keys := []string{}
	err := filepath.WalkDir(s.root, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if err = ctx.Err(); err != nil {
			return err
		}
		if d.Type()&os.ModeSymlink != 0 {
			return errors.New("symlink in storage tree")
		}
		if d.IsDir() || !strings.HasSuffix(d.Name(), ".object") {
			return nil
		}
		rel, err := filepath.Rel(s.root, p)
		if err != nil {
			return err
		}
		key := strings.TrimSuffix(filepath.ToSlash(rel), ".object")
		if key > after && strings.HasPrefix(key, prefix) {
			position, _ := slices.BinarySearch(keys, key)
			if position <= limit {
				keys = slices.Insert(keys, position, key)
				keys = keys[:min(len(keys), limit+1)]
			}
		}
		return nil
	})
	if err != nil {
		return adapters.Page{}, err
	}
	p := adapters.Page{Objects: []adapters.Object{}}
	for _, key := range keys[:min(limit, len(keys))] {
		o, err := s.get(key)
		if err != nil {
			return p, err
		}
		o.Data = nil
		p.Objects = append(p.Objects, o)
	}
	if len(keys) > limit {
		p.Next = keys[limit-1]
	}
	return p, nil
}
func (s *Store) Close() error { s.once.Do(func() { s.closeErr = s.owner.Close() }); return s.closeErr }
