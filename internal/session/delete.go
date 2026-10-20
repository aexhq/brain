package session

import (
	"context"
	"github.com/aexhq/brain/internal/core"
	"os"
	"time"
)

// Delete settles execution before erasing state. Tombstones survive partial cleanup
// so a retry can finish without allowing reads or checkpoint resurrection.
func (e *Engine) Delete(ctx context.Context, g core.Grant, id string) error {
	if !g.Manage || g.ReadOnly {
		return core.E(core.Forbidden, "management is not permitted")
	}
	if !core.ValidID(id) || (g.Session != "" && g.Session != id) {
		return core.E(core.NotFound, "session not found")
	}
	tenant, err := e.opts.Store.Deleted(ctx, id)
	if err != nil {
		return err
	}
	if tenant == "" {
		if _, err := e.inspect(ctx, g, id); err != nil {
			return err
		}
	} else if tenant != g.Tenant {
		return core.E(core.NotFound, "session not found")
	}
	e.hydrateMu.Lock()
	defer e.hydrateMu.Unlock()
	tenant, err = e.opts.Store.Deleted(ctx, id)
	if err != nil {
		return err
	}
	if tenant == "" {
		if err := e.CloseSession(ctx, g, id); err != nil {
			return err
		}
		a, err := e.load(ctx, id)
		if err != nil {
			return err
		}
		defer e.release(a)
		ticker := time.NewTicker(10 * time.Millisecond)
		defer ticker.Stop()
		for {
			a.mu.Lock()
			working := a.working || a.executing
			a.mu.Unlock()
			if !working {
				break
			}
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-ticker.C:
			}
		}
		select {
		case a.syncGate <- struct{}{}:
			defer func() { <-a.syncGate }()
		case <-ctx.Done():
			return ctx.Err()
		}
		a.mu.Lock()
		metadata := clone(a.meta)
		a.mu.Unlock()
		if err := e.opts.Store.Delete(ctx, metadata); err != nil {
			return err
		}
		e.mu.Lock()
		delete(e.entries, id)
		e.mu.Unlock()
		e.dropRunner(a)
		if err := a.log.Close(); err != nil {
			return err
		}
	}
	if e.opts.Archive != nil {
		if err := e.opts.Archive.Delete(ctx, g.Tenant, id); err != nil {
			return err
		}
	}
	dir, err := e.opts.Store.Dir(id)
	if err != nil {
		return err
	}
	return os.RemoveAll(dir)
}
