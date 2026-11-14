package memory

import (
	"context"
	"slices"
	"strings"
	"sync"

	"github.com/aexhq/brain/services/storage/internal/adapters"
)

type Store struct {
	mu      sync.RWMutex
	objects map[string]adapters.Object
}

func New() *Store                             { return &Store{objects: map[string]adapters.Object{}} }
func clone(o adapters.Object) adapters.Object { o.Data = slices.Clone(o.Data); return o }
func (s *Store) Get(ctx context.Context, key string) (adapters.Object, error) {
	if err := ctx.Err(); err != nil {
		return adapters.Object{}, err
	}
	if !adapters.ValidKey(key) {
		return adapters.Object{}, adapters.ErrInvalid
	}
	s.mu.RLock()
	defer s.mu.RUnlock()
	o, ok := s.objects[key]
	if !ok {
		return o, adapters.ErrNotFound
	}
	return clone(o), nil
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
	old, ok := s.objects[key]
	if err := adapters.Check(old, ok, c); err != nil {
		return adapters.Object{}, err
	}
	o := adapters.Object{Key: key, Data: slices.Clone(data), Version: adapters.Version()}
	s.objects[key] = o
	return clone(o), nil
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
	delete(s.objects, key)
	return nil
}
func (s *Store) List(ctx context.Context, prefix, after string, limit int) (adapters.Page, error) {
	if err := ctx.Err(); err != nil {
		return adapters.Page{}, err
	}
	if err := adapters.ValidateList(prefix, after, limit); err != nil {
		return adapters.Page{}, err
	}
	s.mu.RLock()
	defer s.mu.RUnlock()
	keys := []string{}
	for k := range s.objects {
		if strings.HasPrefix(k, prefix) && k > after {
			position, _ := slices.BinarySearch(keys, k)
			if position <= limit {
				keys = slices.Insert(keys, position, k)
				keys = keys[:min(len(keys), limit+1)]
			}
		}
	}
	p := adapters.Page{Objects: []adapters.Object{}}
	for _, k := range keys[:min(limit, len(keys))] {
		o := s.objects[k]
		o.Data = nil
		p.Objects = append(p.Objects, o)
	}
	if len(keys) > limit {
		p.Next = keys[limit-1]
	}
	return p, nil
}
func (s *Store) Close() error { return nil }
