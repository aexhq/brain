package session

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
	"github.com/aexhq/brain/internal/persistence"
)

func syncStatus(m *persistence.Manifest) core.SyncStatus {
	if m == nil {
		return core.SyncStatus{Pending: true}
	}
	return core.SyncStatus{Checkpoint: m.ID, HistoryBytes: m.HistoryBytes, ResumeOffset: m.Session.Checkpoint.Offset, NativeCheckpoint: m.Session.Checkpoint.ID, Time: m.Time}
}

func (e *Engine) dirty(id string) {
	if e.syncer != nil {
		e.syncer.dirty(id)
	}
}

func (e *Engine) flush(ctx context.Context, id string) (core.SyncStatus, error) {
	a, err := e.load(ctx, id)
	if err != nil {
		return core.SyncStatus{}, err
	}
	defer e.release(a)
	select {
	case a.syncGate <- struct{}{}:
		defer func() { <-a.syncGate }()
	case <-ctx.Done():
		return core.SyncStatus{}, ctx.Err()
	}
	a.nativeMu.RLock()
	defer a.nativeMu.RUnlock()
	a.mu.Lock()
	snapshot := persistence.Snapshot{Session: clone(a.meta), HistoryBytes: a.log.Size()}
	a.mu.Unlock()
	fresh, err := e.opts.Store.Get(ctx, id)
	if err != nil {
		return core.SyncStatus{}, err
	}
	snapshot.Session.Credentials = fresh.Credentials
	snapshot.Directory, err = e.opts.Store.Dir(id)
	if err != nil {
		return core.SyncStatus{}, err
	}
	m, err := e.opts.Archive.Publish(ctx, snapshot)
	if err != nil {
		return core.SyncStatus{}, err
	}
	return syncStatus(&m), nil
}

func (e *Engine) Flush(ctx context.Context, g core.Grant, id string) (core.SyncStatus, error) {
	if !g.Manage || g.ReadOnly {
		return core.SyncStatus{}, core.E(core.Forbidden, "management is not permitted")
	}
	if e.opts.Archive == nil {
		return core.SyncStatus{}, core.E(core.Unsupported, "external storage is not configured")
	}
	if _, err := e.inspect(ctx, g, id); err != nil {
		return core.SyncStatus{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, 45*time.Second)
	defer cancel()
	s, err := e.flush(ctx, id)
	if err == nil {
		e.syncer.retry(id)
	}
	return s, err
}

func (e *Engine) SyncStatus(ctx context.Context, g core.Grant, id string) (core.SyncStatus, error) {
	if e.opts.Archive == nil {
		return core.SyncStatus{}, core.E(core.Unsupported, "external storage is not configured")
	}
	s, err := e.inspect(ctx, g, id)
	if err != nil {
		return core.SyncStatus{}, err
	}
	e.hydrateMu.Lock()
	defer e.hydrateMu.Unlock()
	dir, err := e.opts.Store.Dir(id)
	if err != nil {
		return core.SyncStatus{}, err
	}
	m, err := persistence.ReadState(dir)
	if err != nil {
		return core.SyncStatus{}, err
	}
	status := syncStatus(m)
	status.Pending, status.Error = e.syncer.status(id)
	if m == nil || !m.Session.UpdatedAt.Equal(s.UpdatedAt) {
		status.Pending = true
	}
	info, err := os.Stat(filepath.Join(dir, "events.jsonl"))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return core.SyncStatus{}, err
	}
	if info != nil && info.Size() != status.HistoryBytes {
		status.Pending = true
	}
	return status, nil
}

func (e *Engine) Restore(ctx context.Context, g core.Grant, id, reference string) (core.Session, error) {
	if !g.Manage || g.ReadOnly {
		return core.Session{}, core.E(core.Forbidden, "management is not permitted")
	}
	if !core.ValidID(reference) {
		return core.Session{}, core.E(core.Invalid, "an explicit checkpoint reference is required")
	}
	return e.hydrate(ctx, g, id, reference)
}

func (e *Engine) hydrate(ctx context.Context, g core.Grant, id, reference string) (core.Session, error) {
	if e.opts.Archive == nil {
		return core.Session{}, core.E(core.Unsupported, "external storage is not configured")
	}
	if !core.ValidID(id) || (g.Session != "" && g.Session != id) {
		return core.Session{}, core.E(core.NotFound, "session not found")
	}
	e.hydrateMu.Lock()
	defer e.hydrateMu.Unlock()
	if tenant, err := e.opts.Store.Deleted(ctx, id); err != nil {
		return core.Session{}, err
	} else if tenant != "" {
		return core.Session{}, core.E(core.NotFound, "session not found")
	}
	previous, err := e.opts.Store.Get(ctx, id)
	if err == nil {
		if !g.Allows(previous) {
			return core.Session{}, core.E(core.NotFound, "session not found")
		}
		if reference == "" {
			return previous, nil
		}
	} else if core.ErrorCode(err) != core.NotFound {
		return core.Session{}, err
	}
	// Reserve the cache entry during replacement. Existing requests/streams must
	// finish first; restoration never switches their underlying transcript.
	e.mu.Lock()
	if e.closed {
		e.mu.Unlock()
		return core.Session{}, core.E(core.Overloaded, "server is draining")
	}
	old := e.entries[id]
	if old != nil {
		if old.refs != 0 || !old.mu.TryLock() {
			e.mu.Unlock()
			return core.Session{}, core.E(core.Conflict, "session is in use")
		}
		busy := old.working || old.executing
		old.mu.Unlock()
		if busy {
			e.mu.Unlock()
			return core.Session{}, core.E(core.Conflict, "interrupt and settle the session before restoring")
		}
	}
	reservation := &entry{ready: make(chan struct{}), refs: 1}
	e.entries[id] = reservation
	e.wg.Add(1)
	e.mu.Unlock()
	defer e.wg.Done()
	defer func() {
		e.mu.Lock()
		delete(e.entries, id)
		// Waiters receive a conflict and can explicitly retry after replacement.
		reservation.err = core.E(core.Conflict, "session cache changed; retry the request")
		e.mu.Unlock()
		close(reservation.ready)
	}()
	if old != nil {
		e.dropRunner(old)
		if old.log != nil {
			_ = old.log.Close()
		}
	}
	ctx, cancel := context.WithTimeout(ctx, 45*time.Second)
	defer cancel()
	m, staging, err := e.opts.Archive.Restore(ctx, g.Tenant, id, reference, e.opts.Store.Root())
	if errors.Is(err, os.ErrNotExist) {
		return core.Session{}, core.E(core.NotFound, "external checkpoint not found")
	}
	if err != nil {
		return core.Session{}, err
	}
	defer os.RemoveAll(staging)
	if err := credentials.Validate(ctx, e.opts.Keys, m.Session); err != nil {
		return core.Session{}, err
	}
	if _, ok := e.opts.Factories[m.Session.Harness]; !ok {
		return core.Session{}, core.E(core.Unsupported, "saved harness is not configured")
	}
	if m.Session.Status != "closed" {
		m.Session.Status = "interrupted"
	}
	dir, err := e.opts.Store.Dir(id)
	if err != nil {
		return core.Session{}, err
	}
	if err := os.MkdirAll(filepath.Dir(dir), 0700); err != nil {
		return core.Session{}, err
	}
	backup := dir + "-replaced-" + core.ID()
	hadLocal := false
	if _, err := os.Stat(dir); err == nil {
		if err := os.Rename(dir, backup); err != nil {
			return core.Session{}, err
		}
		hadLocal = true
	} else if !errors.Is(err, os.ErrNotExist) {
		return core.Session{}, err
	}
	if err := os.Rename(staging, dir); err != nil {
		if hadLocal {
			err = errors.Join(err, os.Rename(backup, dir))
		}
		return core.Session{}, err
	}
	if err := e.opts.Store.Import(ctx, m.Session); err != nil {
		rollback := os.Rename(dir, staging)
		if rollback == nil && hadLocal {
			rollback = os.Rename(backup, dir)
		}
		return core.Session{}, errors.Join(err, rollback)
	}
	// Retain replaced local data for operator recovery; never hide unsynced loss.
	return e.opts.Store.Get(ctx, id)
}
