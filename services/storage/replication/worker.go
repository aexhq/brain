package replication

import (
	"context"
	"sort"
	"sync"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/archive"
	"go.opentelemetry.io/otel/metric"
	"log/slog"
)

type pendingSync struct {
	tenant   string
	id       string
	change   uint64
	attempts int
	due      time.Time
	err      string
	stopped  bool
}
type Options struct {
	Scan     func(context.Context, string, int) ([]core.Session, error)
	State    func(context.Context, core.Session) (*persistence.Manifest, int64, error)
	Flush    func(context.Context, string) (core.SyncStatus, error)
	Logger   *slog.Logger
	Pending  metric.Int64Gauge
	Duration metric.Float64Histogram
	Failures metric.Int64Counter
}
type Worker struct {
	ctx         context.Context
	opts        Options
	mu          sync.Mutex
	pending     map[string]*pendingSync
	capacity    int
	interval    time.Duration
	overflow    bool
	initialized bool
	Done        chan struct{}
}

func New(ctx context.Context, opts Options, capacity int, interval time.Duration) *Worker {
	s := &Worker{ctx: ctx, opts: opts, pending: make(map[string]*pendingSync), capacity: capacity, interval: interval, Done: make(chan struct{})}
	go s.run()
	return s
}
func (s *Worker) Admit() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.initialized && !s.overflow && len(s.pending) < s.capacity
}
func (s *Worker) Dirty(tenant, externalID string) {
	id := core.SessionKey(tenant, externalID)
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
	s.pending[id] = &pendingSync{change: 1, tenant: tenant, id: externalID}
}
func (s *Worker) Retry(id string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if p := s.pending[id]; p != nil {
		p.attempts = 0
		p.stopped = false
		p.due = time.Time{}
		p.err = ""
	}
}
func (s *Worker) Status(id string) (bool, string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if p := s.pending[id]; p != nil {
		return true, p.err
	}
	return false, ""
}

// Disk is the pending-data buffer. This queue holds only bounded references;
// startup and subsequent scans rediscover changes that could not be queued.
func (s *Worker) scan(ctx context.Context) error {
	s.mu.Lock()
	s.overflow = false
	s.mu.Unlock()
	var after string
	for {
		batch, err := s.opts.Scan(ctx, after, 100)
		if err != nil {
			return err
		}
		if len(batch) == 0 {
			return nil
		}
		for _, session := range batch {
			after = core.SessionKey(session.Tenant, session.ID)
			s.mu.Lock()
			known := s.pending[core.SessionKey(session.Tenant, session.ID)] != nil
			full := len(s.pending) >= s.capacity
			s.mu.Unlock()
			if known {
				continue
			}
			state, size, err := s.opts.State(ctx, session)
			if err != nil {
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
			s.Dirty(session.Tenant, session.ID)
		}
	}
}

func (s *Worker) run() {
	defer close(s.Done)
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()
	for {
		if err := s.scan(s.ctx); err != nil {
			if s.ctx.Err() != nil {
				return
			}
			s.mu.Lock()
			s.overflow = true
			s.mu.Unlock()
			s.opts.Logger.Error("scan pending synchronization", "error", err)
		}
		s.mu.Lock()
		s.initialized = true
		if s.opts.Pending != nil {
			s.opts.Pending.Record(s.ctx, int64(len(s.pending)))
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
			if s.ctx.Err() != nil {
				return
			}
			s.mu.Lock()
			p := s.pending[id]
			change := p.change
			s.mu.Unlock()
			ctx, cancel := context.WithTimeout(s.ctx, 45*time.Second)
			started := time.Now()
			_, err := s.opts.Flush(core.WithTenant(ctx, p.tenant), p.id)
			if s.opts.Duration != nil {
				s.opts.Duration.Record(ctx, time.Since(started).Seconds())
				if err != nil {
					s.opts.Failures.Add(ctx, 1)
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
			if err != nil && s.ctx.Err() == nil {
				s.opts.Logger.Error("synchronize session", "session", id, "error", err)
			}
		}
		select {
		case <-s.ctx.Done():
			return
		case <-ticker.C:
		}
	}
}
