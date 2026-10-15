package session

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"sort"
	"sync"
	"time"

	"github.com/aexhq/brain/internal/persistence"
)

type pendingSync struct {
	change   uint64
	attempts int
	due      time.Time
	err      string
	stopped  bool
}
type syncer struct {
	engine      *Engine
	mu          sync.Mutex
	pending     map[string]*pendingSync
	capacity    int
	interval    time.Duration
	overflow    bool
	initialized bool
	done        chan struct{}
}

func newSyncer(e *Engine, capacity int, interval time.Duration) *syncer {
	return &syncer{engine: e, pending: make(map[string]*pendingSync), capacity: capacity, interval: interval, done: make(chan struct{})}
}
func (s *syncer) admit() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.initialized && !s.overflow && len(s.pending) < s.capacity
}
func (s *syncer) dirty(id string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if p := s.pending[id]; p != nil {
		p.change++
		return
	}
	if len(s.pending) >= s.capacity {
		s.overflow = true
		return
	}
	s.pending[id] = &pendingSync{change: 1}
}
func (s *syncer) retry(id string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if p := s.pending[id]; p != nil {
		p.attempts = 0
		p.stopped = false
		p.due = time.Time{}
		p.err = ""
	}
}
func (s *syncer) status(id string) (bool, string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if p := s.pending[id]; p != nil {
		return true, p.err
	}
	return false, ""
}

// Disk is the pending-data buffer. This queue holds only bounded references;
// startup and subsequent scans rediscover changes that could not be queued.
func (s *syncer) scan(ctx context.Context) error {
	s.mu.Lock()
	s.overflow = false
	s.mu.Unlock()
	var after string
	for {
		batch, err := s.engine.opts.Store.Scan(ctx, after, 100)
		if err != nil {
			return err
		}
		if len(batch) == 0 {
			return nil
		}
		for _, session := range batch {
			after = session.ID
			s.mu.Lock()
			known := s.pending[session.ID] != nil
			full := len(s.pending) >= s.capacity
			s.mu.Unlock()
			if known {
				continue
			}
			dir, _ := s.engine.opts.Store.Dir(session.ID)
			state, err := persistence.ReadState(dir)
			if err != nil {
				return err
			}
			var size int64
			info, err := os.Stat(filepath.Join(dir, "events.jsonl"))
			if err == nil {
				size = info.Size()
			} else if !errors.Is(err, os.ErrNotExist) {
				return err
			}
			if state != nil && state.Session.UpdatedAt.Equal(session.UpdatedAt) && session.Checkpoint != nil && state.Session.Checkpoint.ID == session.Checkpoint.ID && state.HistoryBytes == size {
				continue
			}
			if full {
				s.mu.Lock()
				s.overflow = true
				s.mu.Unlock()
				return nil
			}
			s.dirty(session.ID)
		}
	}
}

func (s *syncer) run() {
	defer close(s.done)
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()
	for {
		if err := s.scan(s.engine.ctx); err != nil {
			if s.engine.ctx.Err() != nil {
				return
			}
			s.mu.Lock()
			s.overflow = true
			s.mu.Unlock()
			s.engine.opts.Logger.Error("scan pending synchronization", "error", err)
		}
		s.mu.Lock()
		s.initialized = true
		if s.engine.opts.Metrics != nil {
			s.engine.opts.Metrics.SyncPending.Record(s.engine.ctx, int64(len(s.pending)))
		}
		ids := make([]string, 0, len(s.pending))
		for id, p := range s.pending {
			if !p.stopped && !time.Now().Before(p.due) {
				ids = append(ids, id)
			}
		}
		s.mu.Unlock()
		sort.Strings(ids)
		for _, id := range ids {
			if s.engine.ctx.Err() != nil {
				return
			}
			s.mu.Lock()
			p := s.pending[id]
			change := p.change
			s.mu.Unlock()
			ctx, cancel := context.WithTimeout(s.engine.ctx, 45*time.Second)
			started := time.Now()
			_, err := s.engine.flush(ctx, id)
			if s.engine.opts.Metrics != nil {
				s.engine.opts.Metrics.SyncDuration.Record(ctx, time.Since(started).Seconds())
				if err != nil {
					s.engine.opts.Metrics.SyncFailures.Add(ctx, 1)
				}
			}
			cancel()
			s.mu.Lock()
			if err == nil {
				if p.change == change {
					delete(s.pending, id)
				} else {
					p.attempts = 0
					p.due = time.Time{}
					p.err = ""
				}
			} else {
				p.attempts++
				p.err = "external synchronization failed; inspect server logs"
				p.stopped = p.attempts >= 5 || persistence.PermanentError(err)
				p.due = time.Now().Add(min(5*time.Minute, s.interval*time.Duration(1<<p.attempts)))
			}
			s.mu.Unlock()
			if err != nil && s.engine.ctx.Err() == nil {
				s.engine.opts.Logger.Error("synchronize session", "session", id, "error", err)
			}
		}
		select {
		case <-s.engine.ctx.Done():
			return
		case <-ticker.C:
		}
	}
}
