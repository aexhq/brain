// Package session coordinates local sessions without implementing an agent loop.
package session

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/store"
	"github.com/aexhq/brain/internal/telemetry"
	"github.com/aexhq/brain/internal/tools"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/trace"
)

type Options struct {
	Store         *store.Store
	Tools         *tools.Registry
	Factories     map[string]harness.Factory
	Provider      func(string) (harness.Provider, error)
	MaxActive     int
	MaxLoaded     int
	MaxQueue      int
	MaxInputBytes int
	MaxLogBytes   int64
	Logger        *slog.Logger
	Metrics       *telemetry.Metrics
	Inspection    func(core.Session) (string, string, func())
}

type Engine struct {
	opts         Options
	ctx          context.Context
	cancel       context.CancelFunc
	mu           sync.Mutex
	entries      map[string]*entry
	active       chan struct{}
	wg           sync.WaitGroup
	closed       bool
	shutdownOnce sync.Once
	stopped      chan struct{}
	shutdownErr  error
}

type queued struct {
	event core.Event
	input core.Input
	trace trace.SpanContext
}
type accepted struct {
	hash  [32]byte
	event core.Event
}
type entry struct {
	mu          sync.Mutex
	ready       chan struct{}
	err         error
	meta        core.Session
	log         *store.Log
	queue       []queued
	keys        map[string]accepted
	working     bool
	closing     bool
	interrupted bool
	current     string
	cancel      context.CancelFunc
	runner      harness.Runner
	restore     bool
}

func New(opts Options) (*Engine, error) {
	if opts.Store == nil || opts.Tools == nil || opts.Provider == nil || opts.MaxActive < 1 || opts.MaxLoaded < opts.MaxActive || opts.MaxQueue < 1 || opts.MaxInputBytes < 1 || opts.MaxLogBytes < 16<<20 {
		return nil, errors.New("invalid session engine options")
	}
	if opts.Logger == nil {
		opts.Logger = slog.Default()
	}
	ctx, cancel := context.WithCancel(context.Background())
	return &Engine{opts: opts, ctx: ctx, cancel: cancel, entries: make(map[string]*entry), active: make(chan struct{}, opts.MaxActive), stopped: make(chan struct{})}, nil
}

func (e *Engine) Ready() bool { e.mu.Lock(); defer e.mu.Unlock(); return !e.closed }

func (e *Engine) load(ctx context.Context, id string) (*entry, error) {
	if !core.ValidID(id) {
		return nil, core.E(core.NotFound, "session not found")
	}
	e.mu.Lock()
	if e.closed {
		e.mu.Unlock()
		return nil, core.E(core.Overloaded, "server is draining")
	}
	if a := e.entries[id]; a != nil {
		e.mu.Unlock()
		select {
		case <-a.ready:
			return a, a.err
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}
	if len(e.entries) >= e.opts.MaxLoaded {
		e.mu.Unlock()
		return nil, core.E(core.Overloaded, "loaded session limit reached")
	}
	a := &entry{ready: make(chan struct{}), keys: make(map[string]accepted)}
	e.entries[id] = a
	e.mu.Unlock()
	defer close(a.ready)
	a.meta, a.err = e.opts.Store.Get(ctx, id)
	if a.err != nil {
		e.mu.Lock()
		delete(e.entries, id)
		e.mu.Unlock()
		return nil, a.err
	}
	dir, err := e.opts.Store.Dir(id)
	if err != nil {
		a.err = err
		return nil, err
	}
	a.log, a.err = store.OpenLog(filepath.Join(dir, "events.jsonl"), e.opts.MaxLogBytes)
	if a.err != nil {
		return nil, a.err
	}
	var cursor string
	for {
		batch, err := a.log.Read(cursor, 1000, -1)
		if err != nil {
			a.err = err
			return nil, err
		}
		if len(batch) == 0 {
			break
		}
		for _, ev := range batch {
			if ev.Type == "user.message" {
				var input core.Input
				if err := json.Unmarshal(ev.Data, &input); err != nil {
					a.err = err
					return nil, err
				}
				if input.Key != "" {
					a.keys[input.Key] = accepted{sha256.Sum256(ev.Data), ev}
				}
			}
		}
		cursor = batch[len(batch)-1].Cursor
	}
	if a.meta.Status == "running" {
		a.meta.Status = "interrupted"
		a.meta.UpdatedAt = time.Now().UTC()
		a.err = e.opts.Store.Save(ctx, a.meta)
	}
	a.restore = true
	return a, a.err
}

func (e *Engine) inspect(ctx context.Context, g core.Grant, id string) (core.Session, error) {
	s, err := e.opts.Store.Get(ctx, id)
	if err != nil {
		return s, err
	}
	if !g.Allows(s) {
		return core.Session{}, core.E(core.NotFound, "session not found")
	}
	return s, nil
}

func (e *Engine) Get(ctx context.Context, g core.Grant, id string) (core.Session, error) {
	if _, err := e.inspect(ctx, g, id); err != nil {
		return core.Session{}, err
	}
	a, err := e.load(ctx, id)
	if err != nil {
		return core.Session{}, err
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	return clone(a.meta), nil
}

func clone(s core.Session) core.Session {
	s.Config.Tools = append([]string{}, s.Config.Tools...)
	if s.Checkpoint != nil {
		cp := *s.Checkpoint
		cp.Files = append([]core.NativeFile{}, cp.Files...)
		cp.Config.Tools = append([]string{}, cp.Config.Tools...)
		s.Checkpoint = &cp
	}
	return s
}

func (e *Engine) List(ctx context.Context, g core.Grant, after string, limit int) ([]core.Session, error) {
	if g.Session != "" {
		s, err := e.Get(ctx, g, g.Session)
		if err != nil {
			return nil, err
		}
		if s.ID <= after {
			return []core.Session{}, nil
		}
		return []core.Session{s}, nil
	}
	return e.opts.Store.List(ctx, g.Tenant, after, limit)
}

func (e *Engine) Create(ctx context.Context, g core.Grant, input core.CreateSession) (core.Session, error) {
	if !g.Manage || g.Session != "" {
		return core.Session{}, core.E(core.Forbidden, "session management is not permitted")
	}
	if _, ok := e.opts.Factories[input.Harness]; !ok {
		return core.Session{}, core.E(core.Unsupported, "harness is not configured")
	}
	if !strings.Contains(input.Config.Model, "/") || len(input.Config.Model) > 256 || len(input.Config.Instructions) > e.opts.MaxInputBytes || len(input.Config.Tools) > 128 {
		return core.Session{}, core.E(core.Invalid, "invalid session configuration")
	}
	switch input.Config.ReasoningEffort {
	case "", "off", "minimal", "low", "medium", "high", "xhigh", "max":
	default:
		return core.Session{}, core.E(core.Invalid, "invalid reasoning effort")
	}
	if _, err := e.opts.Provider(input.Config.Model); err != nil {
		return core.Session{}, err
	}
	if _, err := e.opts.Tools.Select(g.Tenant, input.Config.Tools); err != nil {
		return core.Session{}, err
	}
	e.mu.Lock()
	closed := e.closed
	e.mu.Unlock()
	if closed {
		return core.Session{}, core.E(core.Overloaded, "server is draining")
	}
	now := time.Now().UTC()
	s := core.Session{ID: core.ID(), Tenant: g.Tenant, Harness: input.Harness, Config: input.Config, Status: "idle", CreatedAt: now, UpdatedAt: now}
	if s.Config.Tools == nil {
		s.Config.Tools = []string{}
	}
	s.Checkpoint = &core.Checkpoint{ID: core.ID(), Config: s.Config, Time: now, Files: []core.NativeFile{}}
	if err := e.opts.Store.Create(ctx, s); err != nil {
		return core.Session{}, err
	}
	return s, nil
}

func (e *Engine) Log(ctx context.Context, g core.Grant, id string) (*store.Log, error) {
	if _, err := e.inspect(ctx, g, id); err != nil {
		return nil, err
	}
	a, err := e.load(ctx, id)
	if err != nil {
		return nil, err
	}
	return a.log, nil
}

func (e *Engine) emit(a *entry, kind, inputID string, data any) (core.Event, error) {
	ev, err := core.NewEvent(kind, inputID, data)
	if err != nil {
		return ev, err
	}
	if kind == "assistant.delta" || kind == "tool.progress" {
		a.log.Publish(ev)
		return ev, nil
	}
	return a.log.Append(ev)
}

func (e *Engine) status(a *entry, status, input string) error {
	if _, err := e.emit(a, "session.status", input, core.Status{Status: status}); err != nil {
		return err
	}
	a.meta.Status = status
	a.meta.UpdatedAt = time.Now().UTC()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	return e.opts.Store.Save(ctx, a.meta)
}

func (e *Engine) Submit(ctx context.Context, g core.Grant, id string, input core.Input) (core.Event, error) {
	if input.Type != "user.message" || strings.TrimSpace(input.Text) == "" || len(input.Text) > e.opts.MaxInputBytes || len(input.Key) > 128 {
		return core.Event{}, core.E(core.Invalid, "invalid message")
	}
	if input.Delivery == "" {
		input.Delivery = "queue"
	}
	if input.Delivery != "queue" && input.Delivery != "steer" {
		return core.Event{}, core.E(core.Invalid, "invalid delivery")
	}
	if _, err := e.inspect(ctx, g, id); err != nil {
		return core.Event{}, err
	}
	a, err := e.load(ctx, id)
	if err != nil {
		return core.Event{}, err
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	data, _ := json.Marshal(input)
	hash := sha256.Sum256(data)
	if input.Key != "" {
		if accepted, ok := a.keys[input.Key]; ok {
			if accepted.hash != hash {
				return core.Event{}, core.E(core.Conflict, "idempotency key was used with different input")
			}
			return accepted.event, nil
		}
	}
	if a.closing || a.meta.Status == "closed" {
		return core.Event{}, core.E(core.Conflict, "session is closed")
	}
	if a.meta.Status == "failed" || a.meta.Status == "interrupted" {
		return core.Event{}, core.E(core.Conflict, "resume the interrupted session before sending input")
	}
	if input.Delivery == "steer" {
		return core.Event{}, core.E(core.Unsupported, "steering is not enabled; use queue delivery or explicit interruption")
	}
	if len(a.queue) >= e.opts.MaxQueue || a.log.Size() > e.opts.MaxLogBytes-2*store.MaxRecord {
		return core.Event{}, core.E(core.Overloaded, "session admission limit reached")
	}
	start := !a.working
	e.mu.Lock()
	if e.closed {
		e.mu.Unlock()
		return core.Event{}, core.E(core.Overloaded, "server is draining")
	}
	if start {
		e.wg.Add(1)
	}
	e.mu.Unlock()
	launched := false
	defer func() {
		if start && !launched {
			e.wg.Done()
		}
	}()
	if !a.working {
		a.meta.Status = "running"
		a.meta.UpdatedAt = time.Now().UTC()
		if err := e.opts.Store.Save(ctx, a.meta); err != nil {
			a.meta.Status = "failed"
			return core.Event{}, err
		}
	}
	ev, err := e.emit(a, "user.message", "", input)
	if err != nil {
		a.meta.Status = "failed"
		if a.cancel != nil {
			a.cancel()
		}
		return ev, err
	}
	if input.Key != "" {
		a.keys[input.Key] = accepted{hash, ev}
	}
	a.queue = append(a.queue, queued{event: ev, input: input, trace: trace.SpanContextFromContext(ctx)})
	if start {
		a.working = true
		launched = true
		go e.work(a)
	}
	return ev, nil
}

func (e *Engine) work(a *entry) {
	defer e.wg.Done()
	for {
		a.mu.Lock()
		if len(a.queue) == 0 || a.closing || a.interrupted || e.ctx.Err() != nil {
			if len(a.queue) > 0 {
				for _, pending := range a.queue {
					_, _ = e.emit(a, "session.error", pending.event.ID, core.Error{Code: core.Conflict, Message: "queued input interrupted before execution"})
				}
				a.queue = nil
				status := "interrupted"
				if a.closing {
					status = "closed"
				}
				_ = e.status(a, status, "")
			}
			a.working = false
			a.mu.Unlock()
			return
		}
		q := a.queue[0]
		a.queue = a.queue[1:]
		a.current = q.event.ID
		runCtx, cancel := context.WithCancel(trace.ContextWithSpanContext(e.ctx, q.trace))
		a.cancel = cancel
		err := e.status(a, "running", q.event.ID)
		a.mu.Unlock()
		if err == nil {
			select {
			case e.active <- struct{}{}:
				err = e.run(runCtx, a, q)
				<-e.active
			case <-runCtx.Done():
				err = runCtx.Err()
			}
		}
		cancel()
		a.mu.Lock()
		a.cancel = nil
		a.current = ""
		if err != nil || a.interrupted || a.closing || e.ctx.Err() != nil {
			status := "failed"
			code := core.ErrorCode(err)
			message := "execution halted due to a Brain failure"
			if a.interrupted || a.closing || errors.Is(err, context.Canceled) || e.ctx.Err() != nil {
				status = "interrupted"
				code = core.Conflict
				message = "execution interrupted; remote work may still exist"
			} else if code == core.ProviderError {
				status = "interrupted"
				message = err.Error()
			}
			if err != nil {
				e.opts.Logger.Error("session execution stopped", "session", a.meta.ID, "error", err)
			}
			_, _ = e.emit(a, "session.error", q.event.ID, core.Error{Code: code, Message: message})
			for _, pending := range a.queue {
				_, _ = e.emit(a, "session.error", pending.event.ID, core.Error{Code: core.Conflict, Message: "queued input interrupted before execution"})
			}
			a.queue = nil
			if a.closing {
				status = "closed"
			}
			_ = e.status(a, status, q.event.ID)
			a.meta.Status = status
			a.working = false
			a.restore = true
			a.mu.Unlock()
			return
		}
		if len(a.queue) == 0 {
			if err := e.status(a, "idle", q.event.ID); err != nil {
				a.meta.Status = "failed"
				e.opts.Logger.Error("save session status", "session", a.meta.ID, "error", err)
			}
		}
		a.mu.Unlock()
	}
}

func (e *Engine) run(ctx context.Context, a *entry, q queued) error {
	ctx, span := otel.Tracer("github.com/aexhq/brain").Start(ctx, "session.execute", trace.WithAttributes(attribute.String("session.id", a.meta.ID), attribute.String("harness", a.meta.Harness)))
	defer span.End()
	if e.opts.Metrics != nil {
		e.opts.Metrics.Active.Add(ctx, 1)
		defer e.opts.Metrics.Active.Add(ctx, -1)
	}
	dir, err := e.opts.Store.Dir(a.meta.ID)
	if err != nil {
		return err
	}
	nativeDir := filepath.Join(dir, "native")
	a.mu.Lock()
	needsRestore := a.restore
	checkpoint := a.meta.Checkpoint
	cfg := a.meta.Config
	a.mu.Unlock()
	if needsRestore {
		if err := restoreNative(nativeDir, checkpoint); err != nil {
			return err
		}
		a.mu.Lock()
		a.restore = false
		a.mu.Unlock()
	}
	provider, err := e.opts.Provider(cfg.Model)
	if err != nil {
		return err
	}
	selected, err := e.opts.Tools.Select(a.meta.Tenant, cfg.Tools)
	if err != nil {
		return err
	}
	selectedNames := make(map[string]bool)
	for _, tool := range selected {
		selectedNames[tool.Name] = true
	}
	runner, err := e.opts.Factories[a.meta.Harness].Open(ctx, harness.Options{SessionID: a.meta.ID, Dir: nativeDir, Config: cfg, Tools: selected, Provider: provider,
		Event: func(kind string, data json.RawMessage) error {
			if kind != "assistant.message" && kind != "assistant.delta" {
				return errors.New("unsupported native event")
			}
			_, err := e.emit(a, kind, q.event.ID, data)
			return err
		},
		Call: func(callCtx context.Context, call core.Call) (core.Result, error) {
			callCtx, span := otel.Tracer("github.com/aexhq/brain").Start(callCtx, "tool.call", trace.WithAttributes(attribute.String("tool.name", call.Name)))
			defer span.End()
			if !selectedNames[call.Name] {
				return core.Result{}, errors.New("harness called an unbound tool")
			}
			call.ID = core.ID()
			if _, err := e.emit(a, "tool.call", q.event.ID, call); err != nil {
				return core.Result{}, err
			}
			scope := tools.Scope{Tenant: a.meta.Tenant, Session: a.meta.ID, Call: call.ID}
			revoke := func() {}
			if e.opts.Inspection != nil && e.opts.Tools.NeedsInspection(call.Name) {
				scope.InspectionToken, scope.BrainURL, revoke = e.opts.Inspection(a.meta)
			}
			defer revoke()
			started := time.Now()
			result := e.opts.Tools.Call(callCtx, scope, call, func(data json.RawMessage) {
				_, _ = e.emit(a, "tool.progress", q.event.ID, map[string]any{"call_id": call.ID, "data": data})
			})
			if e.opts.Metrics != nil {
				outcome := "success"
				if result.IsError {
					outcome = "tool_error"
				}
				attrs := metric.WithAttributes(attribute.String("tool", call.Name), attribute.String("outcome", outcome))
				e.opts.Metrics.ToolCalls.Add(callCtx, 1, attrs)
				e.opts.Metrics.ToolDuration.Record(callCtx, time.Since(started).Seconds(), attrs)
			}
			if _, err := e.emit(a, "tool.result", q.event.ID, core.ToolResult{CallID: call.ID, Result: result}); err != nil {
				return core.Result{}, err
			}
			return result, nil
		},
	})
	if err != nil {
		return err
	}
	defer runner.Close()
	a.mu.Lock()
	a.runner = runner
	a.mu.Unlock()
	defer func() { a.mu.Lock(); a.runner = nil; a.mu.Unlock() }()
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := runner.Prompt(ctx, q.input.Text); err != nil {
		return err
	}
	a.mu.Lock()
	interrupted := a.interrupted || a.closing
	a.mu.Unlock()
	if interrupted {
		return context.Canceled
	}
	files, err := runner.Checkpoint(ctx)
	if err != nil {
		return err
	}
	for _, f := range files {
		p, err := nativePath(nativeDir, f.Path)
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
	a.mu.Lock()
	defer a.mu.Unlock()
	a.meta.Checkpoint = &core.Checkpoint{ID: core.ID(), Offset: a.log.Size(), Files: files, Config: cfg, Time: time.Now().UTC()}
	return e.opts.Store.Save(ctx, a.meta)
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

func (e *Engine) Interrupt(ctx context.Context, g core.Grant, id string) error {
	if _, err := e.inspect(ctx, g, id); err != nil {
		return err
	}
	a, err := e.load(ctx, id)
	if err != nil {
		return err
	}
	a.mu.Lock()
	if !a.working {
		a.mu.Unlock()
		return nil
	}
	a.interrupted = true
	runner := a.runner
	cancel := a.cancel
	a.mu.Unlock()
	if runner != nil {
		stopCtx, stop := context.WithTimeout(ctx, 3*time.Second)
		err = runner.Interrupt(stopCtx)
		stop()
	}
	if cancel != nil {
		cancel()
	}
	return err
}

func (e *Engine) Resume(ctx context.Context, g core.Grant, id string) (core.Session, error) {
	if !g.Manage {
		return core.Session{}, core.E(core.Forbidden, "management is not permitted")
	}
	if _, err := e.inspect(ctx, g, id); err != nil {
		return core.Session{}, err
	}
	a, err := e.load(ctx, id)
	if err != nil {
		return core.Session{}, err
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.working || a.closing || a.meta.Status == "closed" {
		return core.Session{}, core.E(core.Conflict, "session cannot resume in its current state")
	}
	dir, _ := e.opts.Store.Dir(id)
	if err := restoreNative(filepath.Join(dir, "native"), a.meta.Checkpoint); err != nil {
		return core.Session{}, err
	}
	a.restore = false
	a.interrupted = false
	a.meta.Config = a.meta.Checkpoint.Config
	if err := e.status(a, "idle", ""); err != nil {
		return core.Session{}, err
	}
	return clone(a.meta), nil
}

func (e *Engine) CloseSession(ctx context.Context, g core.Grant, id string) error {
	if !g.Manage {
		return core.E(core.Forbidden, "management is not permitted")
	}
	if _, err := e.inspect(ctx, g, id); err != nil {
		return err
	}
	a, err := e.load(ctx, id)
	if err != nil {
		return err
	}
	a.mu.Lock()
	a.closing = true
	working := a.working
	if !working {
		err = e.status(a, "closed", "")
	}
	a.mu.Unlock()
	if working {
		return e.Interrupt(ctx, g, id)
	}
	return err
}

func (e *Engine) Shutdown(ctx context.Context) error {
	e.shutdownOnce.Do(func() {
		e.mu.Lock()
		e.closed = true
		e.cancel()
		e.mu.Unlock()
		go func() {
			e.wg.Wait()
			e.mu.Lock()
			entries := make([]*entry, 0, len(e.entries))
			for _, a := range e.entries {
				entries = append(entries, a)
			}
			e.mu.Unlock()
			var errs []error
			for _, a := range entries {
				<-a.ready
				if a.log != nil {
					errs = append(errs, a.log.Close())
				}
			}
			e.shutdownErr = errors.Join(errs...)
			close(e.stopped)
		}()
	})
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-e.stopped:
		return e.shutdownErr
	}
}
