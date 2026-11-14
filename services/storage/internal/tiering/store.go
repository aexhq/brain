// Package tiering owns cache policy and replication, independent of providers.
package tiering

import (
	"bytes"
	"container/list"
	"context"
	"database/sql"
	"errors"
	"fmt"
	core "github.com/aexhq/brain/services/brain/api"
	"hash/fnv"
	"log/slog"
	"path/filepath"
	"sync"
	"time"

	"github.com/aexhq/brain/services/storage/internal/adapters"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	_ "modernc.org/sqlite"
)

type Options struct {
	MinFreeBytes                 int64
	Disk, Remote                 adapters.Adapter
	Index                        string
	MemoryBytes, DiskBytes       int64
	MemoryTTL, DiskTTL, Interval time.Duration
	Logger                       *slog.Logger
}
type entry struct {
	object  adapters.Object
	touched time.Time
}
type Store struct {
	o        Options
	db       *sql.DB
	locks    [64]sync.Mutex
	capacity sync.Mutex
	mu       sync.Mutex
	lru      *list.List
	memory   map[string]*list.Element
	bytes    int64
	ctx      context.Context
	cancel   context.CancelFunc
	done     chan struct{}
	access   metric.Int64Counter
	resident metric.Int64Gauge
	lag      metric.Int64Gauge
}

func New(ctx context.Context, o Options) (*Store, error) {
	if o.Disk == nil || o.Index == "" || o.MemoryBytes < 0 || o.DiskBytes < 1 || o.MemoryTTL <= 0 || o.DiskTTL <= 0 || o.Interval <= 0 {
		return nil, adapters.ErrInvalid
	}
	if o.Logger == nil {
		o.Logger = slog.Default()
	}
	db, err := sql.Open("sqlite", o.Index)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	if _, err = db.Exec(`PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS cache(key TEXT PRIMARY KEY,bytes INTEGER NOT NULL,touched INTEGER NOT NULL,dirty INTEGER NOT NULL,version TEXT NOT NULL); CREATE INDEX IF NOT EXISTS cache_eviction ON cache(dirty,touched);`); err != nil {
		db.Close()
		return nil, err
	}
	c, cancel := context.WithCancel(ctx)
	s := &Store{o: o, db: db, lru: list.New(), memory: map[string]*list.Element{}, ctx: c, cancel: cancel, done: make(chan struct{})}
	m := otel.Meter("github.com/aexhq/brain/storage")
	s.access, err = m.Int64Counter("brain.storage.reads")
	if err == nil {
		s.resident, err = m.Int64Gauge("brain.storage.cache.bytes", metric.WithUnit("By"))
	}
	if err == nil {
		s.lag, err = m.Int64Gauge("brain.storage.sync.pending")
	}
	if err != nil {
		cancel()
		db.Close()
		return nil, err
	}
	// A crash between a disk write and its index update must rediscover dirty data.
	var after string
	for {
		p, err := o.Disk.List(ctx, "", after, 100)
		if err != nil {
			cancel()
			db.Close()
			return nil, err
		}
		for _, v := range p.Objects {
			var version string
			err := db.QueryRowContext(ctx, `SELECT version FROM cache WHERE key=?`, v.Key).Scan(&version)
			if err != nil && !errors.Is(err, sql.ErrNoRows) {
				cancel()
				db.Close()
				return nil, err
			}
			if err == nil && version == v.Version {
				continue
			}
			obj, err := o.Disk.Get(ctx, v.Key)
			if err != nil {
				cancel()
				db.Close()
				return nil, err
			}
			if err = s.index(ctx, obj, true); err != nil {
				cancel()
				db.Close()
				return nil, err
			}
		}
		if p.Next == "" {
			break
		}
		after = p.Next
	}
	go s.run()
	return s, nil
}
func (s *Store) lock(key string) *sync.Mutex {
	h := fnv.New32a()
	_, _ = h.Write([]byte(key))
	return &s.locks[h.Sum32()%uint32(len(s.locks))]
}
func (s *Store) index(ctx context.Context, o adapters.Object, dirty bool) error {
	_, err := s.db.ExecContext(ctx, `INSERT INTO cache(key,bytes,touched,dirty,version) VALUES(?,?,?,?,?) ON CONFLICT(key) DO UPDATE SET bytes=excluded.bytes,touched=excluded.touched,dirty=excluded.dirty,version=excluded.version`, o.Key, len(o.Data), time.Now().UnixNano(), dirty, o.Version)
	return err
}
func (s *Store) remember(o adapters.Object) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.forgetLocked(o.Key)
	if int64(len(o.Data)) > s.o.MemoryBytes {
		return
	}
	o.Data = bytes.Clone(o.Data)
	s.memory[o.Key] = s.lru.PushFront(entry{o, time.Now()})
	s.bytes += int64(len(o.Data))
	s.trimLocked(s.o.MemoryBytes, time.Now())
}
func (s *Store) forgetLocked(key string) {
	if e := s.memory[key]; e != nil {
		s.bytes -= int64(len(e.Value.(entry).object.Data))
		s.lru.Remove(e)
		delete(s.memory, key)
	}
}
func (s *Store) trimLocked(budget int64, now time.Time) {
	for e := s.lru.Back(); e != nil; e = s.lru.Back() {
		v := e.Value.(entry)
		if s.bytes <= budget && now.Sub(v.touched) < s.o.MemoryTTL {
			break
		}
		s.forgetLocked(v.object.Key)
	}
}
func (s *Store) hit(ctx context.Context, tier string) {
	s.access.Add(ctx, 1, metric.WithAttributes(attribute.String("tier", tier)))
}
func (s *Store) Get(ctx context.Context, key string) (adapters.Object, error) {
	if !adapters.ValidKey(key) {
		return adapters.Object{}, adapters.ErrInvalid
	}
	lock := s.lock(key)
	lock.Lock()
	defer lock.Unlock()
	s.mu.Lock()
	e := s.memory[key]
	if e != nil {
		v := e.Value.(entry)
		if time.Since(v.touched) < s.o.MemoryTTL {
			v.touched = time.Now()
			e.Value = v
			s.lru.MoveToFront(e)
			v.object.Data = bytes.Clone(v.object.Data)
			s.mu.Unlock()
			s.hit(ctx, "memory")
			return v.object, nil
		}
		s.forgetLocked(key)
	}
	s.mu.Unlock()
	o, err := s.o.Disk.Get(ctx, key)
	if err == nil {
		_, err = s.db.ExecContext(ctx, `UPDATE cache SET touched=? WHERE key=?`, time.Now().UnixNano(), key)
		if err != nil {
			return o, err
		}
		s.remember(o)
		s.hit(ctx, "disk")
		return o, nil
	}
	if !errors.Is(err, adapters.ErrNotFound) || s.o.Remote == nil {
		return o, err
	}
	o, err = s.o.Remote.Get(ctx, key)
	if err != nil {
		return o, err
	}
	s.capacity.Lock()
	defer s.capacity.Unlock()
	if err = s.room(ctx, int64(len(o.Data))); err != nil {
		return adapters.Object{}, err
	}
	o, err = s.o.Disk.Put(ctx, key, o.Data, adapters.Condition{})
	if err != nil {
		return o, err
	}
	if err = s.index(ctx, o, false); err != nil {
		return o, err
	}
	s.remember(o)
	s.hit(ctx, "external")
	return o, nil
}
func (s *Store) room(ctx context.Context, size int64) error {
	if s.o.MinFreeBytes > 0 {
		free, err := diskFree(filepath.Dir(s.o.Index))
		if err != nil {
			return err
		}
		if free-size < s.o.MinFreeBytes {
			return core.E(core.Overloaded, "storage disk reserve reached")
		}
	}
	var used int64
	if err := s.db.QueryRowContext(ctx, `SELECT COALESCE(SUM(bytes),0) FROM cache`).Scan(&used); err != nil {
		return err
	}
	if used+size > s.o.DiskBytes {
		return core.E(core.Overloaded, fmt.Sprintf("storage disk budget reached: %d + %d > %d", used, size, s.o.DiskBytes))
	}
	return nil
}
func (s *Store) Put(ctx context.Context, key string, data []byte, c adapters.Condition) (adapters.Object, error) {
	if err := adapters.Validate(key, data, c); err != nil {
		return adapters.Object{}, err
	}
	lock := s.lock(key)
	lock.Lock()
	defer lock.Unlock()
	old, err := s.o.Disk.Get(ctx, key)
	if err != nil && !errors.Is(err, adapters.ErrNotFound) {
		return adapters.Object{}, err
	}
	s.capacity.Lock()
	defer s.capacity.Unlock()
	if err = s.room(ctx, max(0, int64(len(data)-len(old.Data)))); err != nil {
		return adapters.Object{}, err
	}
	o, err := s.o.Disk.Put(ctx, key, data, c)
	if err != nil {
		return o, err
	}
	if err = s.index(ctx, o, true); err != nil {
		return o, err
	}
	s.remember(o)
	return o, nil
}

// Delete is permanent deletion, distinct from local cache eviction.
func (s *Store) Delete(ctx context.Context, key string) error {
	if !adapters.ValidKey(key) {
		return adapters.ErrInvalid
	}
	lock := s.lock(key)
	lock.Lock()
	defer lock.Unlock()
	if s.o.Remote != nil {
		if err := s.o.Remote.Delete(ctx, key); err != nil {
			return err
		}
	}
	if err := s.o.Disk.Delete(ctx, key); err != nil {
		return err
	}
	_, err := s.db.ExecContext(ctx, `DELETE FROM cache WHERE key=?`, key)
	s.mu.Lock()
	s.forgetLocked(key)
	s.mu.Unlock()
	return err
}
func (s *Store) List(ctx context.Context, prefix, after string, limit int) (adapters.Page, error) {
	return s.o.Disk.List(ctx, prefix, after, limit)
}

func (s *Store) DeletePrefix(ctx context.Context, prefix string) error {
	if prefix == "" {
		return adapters.ErrInvalid
	}
	for _, source := range []adapters.Adapter{s.o.Disk, s.o.Remote} {
		if source == nil {
			continue
		}
		for {
			p, err := source.List(ctx, prefix, "", 100)
			if err != nil {
				return err
			}
			if len(p.Objects) == 0 {
				break
			}
			for _, o := range p.Objects {
				if err = s.Delete(ctx, o.Key); err != nil {
					return err
				}
			}
		}
	}
	return nil
}
func (s *Store) dirty(ctx context.Context) ([]string, error) {
	rows, err := s.db.QueryContext(ctx, `SELECT key FROM cache WHERE dirty=1 ORDER BY touched LIMIT 100`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	keys := []string{}
	for rows.Next() {
		var key string
		if err = rows.Scan(&key); err != nil {
			return nil, err
		}
		keys = append(keys, key)
	}
	return keys, rows.Err()
}
func (s *Store) Flush(ctx context.Context) error {
	if s.o.Remote == nil {
		return errors.New("external storage is not configured")
	}
	for {
		keys, err := s.dirty(ctx)
		if err != nil {
			return err
		}
		if len(keys) == 0 {
			return nil
		}
		for _, key := range keys {
			if err = s.flush(ctx, key); err != nil {
				return err
			}
		}
	}
}

// Commit is the external durability barrier for one object. Snapshot publishers
// commit referenced blocks before publishing a manifest or metadata pointer.
func (s *Store) Commit(ctx context.Context, key string) error {
	if s.o.Remote == nil {
		return errors.New("external storage is not configured")
	}
	return s.flush(ctx, key)
}
func (s *Store) Committed(ctx context.Context, key string) (bool, error) {
	var dirty bool
	err := s.db.QueryRowContext(ctx, `SELECT dirty FROM cache WHERE key=?`, key).Scan(&dirty)
	if errors.Is(err, sql.ErrNoRows) {
		return false, nil
	}
	return err == nil && !dirty, err
}

// MarkCommitted lets the metadata publisher use the same disk capacity policy
// after its conditional external write, while keeping publication order above
// adapters. Only the acknowledged local version becomes evictable.
func (s *Store) MarkCommitted(ctx context.Context, key, version string) error {
	_, err := s.db.ExecContext(ctx, `UPDATE cache SET dirty=0 WHERE key=? AND version=?`, key, version)
	return err
}
func (s *Store) DirtyKeys(ctx context.Context) ([]string, error) { return s.dirty(ctx) }
func (s *Store) flush(ctx context.Context, key string) error {
	lock := s.lock(key)
	lock.Lock()
	defer lock.Unlock()
	var dirty bool
	err := s.db.QueryRowContext(ctx, `SELECT dirty FROM cache WHERE key=?`, key).Scan(&dirty)
	if errors.Is(err, sql.ErrNoRows) || err == nil && !dirty {
		return nil
	}
	if err != nil {
		return err
	}
	o, err := s.o.Disk.Get(ctx, key)
	if err != nil {
		return err
	}
	if _, err = s.o.Remote.Put(ctx, key, o.Data, adapters.Condition{}); err != nil {
		return err
	}
	_, err = s.db.ExecContext(ctx, `UPDATE cache SET dirty=0 WHERE key=? AND version=?`, key, o.Version)
	return err
}

// Maintain evicts only externally committed objects, never the dirty tail.
func (s *Store) Maintain(ctx context.Context) error {
	pressure := false
	if s.o.MinFreeBytes > 0 {
		free, err := diskFree(filepath.Dir(s.o.Index))
		if err != nil {
			return err
		}
		pressure = free < s.o.MinFreeBytes*2
	}
	budget := s.o.MemoryBytes
	if limit, used := memoryPressure(); limit > 0 && used > limit*8/10 {
		budget = max(0, budget-(used-limit*8/10))
	}
	s.mu.Lock()
	s.trimLocked(budget, time.Now())
	resident := s.bytes
	s.mu.Unlock()
	s.resident.Record(ctx, resident)
	var used, pending int64
	if err := s.db.QueryRowContext(ctx, `SELECT COALESCE(SUM(bytes),0),COALESCE(SUM(dirty),0) FROM cache`).Scan(&used, &pending); err != nil {
		return err
	}
	s.lag.Record(ctx, pending)
	rows, err := s.db.QueryContext(ctx, `SELECT key,touched FROM cache WHERE dirty=0 ORDER BY touched LIMIT 100`)
	if err != nil {
		return err
	}
	type candidate struct {
		key     string
		touched int64
	}
	items := []candidate{}
	for rows.Next() {
		var v candidate
		if err = rows.Scan(&v.key, &v.touched); err != nil {
			rows.Close()
			return err
		}
		items = append(items, v)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return err
	}
	for _, v := range items {
		if !pressure && used < s.o.DiskBytes*8/10 && time.Since(time.Unix(0, v.touched)) < s.o.DiskTTL {
			break
		}
		lock := s.lock(v.key)
		lock.Lock()
		var dirty bool
		var size, touched int64
		err = s.db.QueryRowContext(ctx, `SELECT dirty,bytes,touched FROM cache WHERE key=?`, v.key).Scan(&dirty, &size, &touched)
		if err == nil && !dirty && touched == v.touched {
			err = s.o.Disk.Delete(ctx, v.key)
			if err == nil {
				_, err = s.db.ExecContext(ctx, `DELETE FROM cache WHERE key=?`, v.key)
				used -= size
				s.mu.Lock()
				s.forgetLocked(v.key)
				s.mu.Unlock()
			}
		}
		lock.Unlock()
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return err
		}
	}
	return nil
}
func (s *Store) Pending(ctx context.Context) (int64, error) {
	var n int64
	err := s.db.QueryRowContext(ctx, `SELECT COUNT(*) FROM cache WHERE dirty=1`).Scan(&n)
	return n, err
}
func (s *Store) run() {
	defer close(s.done)
	t := time.NewTicker(s.o.Interval)
	defer t.Stop()
	for {
		select {
		case <-s.ctx.Done():
			return
		case <-t.C:
			ctx, cancel := context.WithTimeout(s.ctx, 30*time.Second)
			// Domain publishers own dependency order. Independently flushing a
			// manifest here could expose it before its referenced objects.
			if err := s.Maintain(ctx); err != nil && ctx.Err() == nil {
				s.o.Logger.Error("storage capacity maintenance failed", "error", err)
			}
			cancel()
		}
	}
}
func (s *Store) Close() error {
	s.cancel()
	<-s.done
	return errors.Join(s.db.Close(), s.o.Disk.Close())
}
