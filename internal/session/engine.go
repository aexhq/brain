// Package session coordinates local sessions without implementing an agent loop.
package session

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/url"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
	"github.com/aexhq/brain/internal/harness"
	"github.com/aexhq/brain/internal/store"
	"github.com/aexhq/brain/internal/telemetry"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/trace"
)

type Options struct {
	MediaOrigins  []string
	Store         Repository
	Keys          credentials.Keys
	Tools         ToolExecutor
	Factories     map[string]harness.Factory
	Provider      func(context.Context, core.Session, *core.ModelCredential) (harness.Provider, error)
	MaxActive     int
	MaxLoaded     int
	MaxQueue      int
	MaxInputBytes int
	MaxLogBytes   int64
	Logger        *slog.Logger
	Metrics       *telemetry.Metrics
	Inspection    func(core.Session) (string, string, func())
	Archive       Checkpoints
	SyncQueue     int
	SyncInterval  time.Duration
}

type Engine struct {
	opts         Options
	ctx          context.Context
	cancel       context.CancelFunc
	mu           sync.Mutex
	hydrateMu    sync.Mutex
	entries      map[string]*entry
	active       chan struct{}
	resident     chan struct{}
	wg           sync.WaitGroup
	closed       bool
	shutdownOnce sync.Once
	stopped      chan struct{}
	shutdownErr  error
	syncer       *syncer
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
	nativeMu    sync.RWMutex
	syncGate    chan struct{}
	refs        int // protected by Engine.mu; includes open event streams
	ready       chan struct{}
	err         error
	meta        core.Session
	log         *store.Log
	queue       []queued
	keys        map[string]accepted
	requests    map[string]requestObservation
	working     bool
	closing     bool
	interrupted bool
	current     string
	cancel      context.CancelFunc
	runner      harness.Runner
	provider    harness.Provider
	executing   bool
	runCtx      context.Context
	restore     bool
	firstOutput time.Time
}

func New(opts Options) (*Engine, error) {
	if opts.Store == nil || opts.Tools == nil || opts.Provider == nil || opts.MaxActive < 1 || opts.MaxLoaded < opts.MaxActive || opts.MaxQueue < 1 || opts.MaxInputBytes < 1 || opts.MaxLogBytes < 16<<20 {
		return nil, errors.New("invalid session engine options")
	}
	if opts.Logger == nil {
		opts.Logger = slog.Default()
	}
	ctx, cancel := context.WithCancel(context.Background())
	e := &Engine{opts: opts, ctx: ctx, cancel: cancel, entries: make(map[string]*entry), active: make(chan struct{}, opts.MaxActive), resident: make(chan struct{}, opts.MaxActive), stopped: make(chan struct{})}
	if opts.Archive != nil {
		if opts.SyncQueue < 1 || opts.SyncInterval <= 0 {
			cancel()
			return nil, errors.New("invalid synchronization limits")
		}
		e.syncer = newSyncer(e, opts.SyncQueue, opts.SyncInterval)
		go e.syncer.run()
	}
	return e, nil
}

func (e *Engine) Ready() bool {
	e.mu.Lock()
	closed := e.closed
	e.mu.Unlock()
	return !closed && (e.syncer == nil || e.syncer.admit())
}

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
		a.refs++
		e.mu.Unlock()
		select {
		case <-a.ready:
			if a.err != nil {
				e.release(a)
			}
			return a, a.err
		case <-ctx.Done():
			e.release(a)
			return nil, ctx.Err()
		}
	}
	var evicted *entry
	if len(e.entries) >= e.opts.MaxLoaded {
		for oldID, old := range e.entries {
			if old.refs != 0 {
				continue
			}
			select {
			case <-old.ready:
			default:
				continue
			}
			if !old.mu.TryLock() {
				continue
			}
			idle := !old.working && !old.executing
			old.mu.Unlock()
			if idle {
				delete(e.entries, oldID)
				evicted = old
				break
			}
		}
		if evicted == nil {
			e.mu.Unlock()
			return nil, core.E(core.Overloaded, "loaded session limit reached")
		}
	}
	a := &entry{ready: make(chan struct{}), keys: make(map[string]accepted), requests: make(map[string]requestObservation), refs: 1, syncGate: make(chan struct{}, 1)}
	e.entries[id] = a
	e.mu.Unlock()
	if evicted != nil {
		e.dropRunner(evicted)
		if evicted.log != nil {
			_ = evicted.log.Close()
		}
	}
	defer func() {
		if a.err != nil {
			if a.log != nil {
				_ = a.log.Close()
			}
			e.mu.Lock()
			delete(e.entries, id)
			a.refs--
			e.mu.Unlock()
		}
		close(a.ready)
	}()
	a.meta, a.err = e.opts.Store.Get(ctx, id)
	if a.err != nil {
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
			if ev.Type == "model.request" {
				var request core.ModelRequest
				if err := json.Unmarshal(ev.Data, &request); err != nil {
					a.err = err
					return nil, err
				}
				if err := request.Validate(); err != nil {
					a.err = err
					return nil, err
				}
				a.requests[requestKey(request)] = requestObservation{request: request, input: ev.InputID}
			}
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
	if a.err == nil {
		_, a.err = e.settleModelRequests(a, "")
	}
	a.restore = true
	return a, a.err
}

func (e *Engine) release(a *entry) { e.mu.Lock(); a.refs--; e.mu.Unlock() }

func (e *Engine) inspect(ctx context.Context, g core.Grant, id string) (core.Session, error) {
	s, err := e.opts.Store.Get(ctx, id)
	if core.ErrorCode(err) == core.NotFound && e.opts.Archive != nil && (g.Session == "" || g.Session == id) {
		s, err = e.hydrate(ctx, g, id, "")
	}
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
	defer e.release(a)
	a.mu.Lock()
	defer a.mu.Unlock()
	return clone(a.meta), nil
}

func clone(s core.Session) core.Session {
	s.Config = cloneConfig(s.Config)
	s.Descriptors = cloneTools(s.Descriptors)
	s.CredentialBindings = slices.Clone(s.CredentialBindings)
	s.Credentials = slices.Clone(s.Credentials)
	for i := range s.Credentials {
		s.Credentials[i].Ciphertext = slices.Clone(s.Credentials[i].Ciphertext)
	}
	if s.Checkpoint != nil {
		cp := *s.Checkpoint
		cp.Files = append([]core.NativeFile{}, cp.Files...)
		cp.Config = cloneConfig(cp.Config)
		s.Checkpoint = &cp
	}
	return s
}

func cloneTools(tools []core.Tool) []core.Tool {
	result := slices.Clone(tools)
	for i := range result {
		result[i].InputSchema = slices.Clone(result[i].InputSchema)
		result[i].OutputSchema = slices.Clone(result[i].OutputSchema)
	}
	return result
}
func cloneConfig(cfg core.Config) core.Config {
	cfg.Tools = append([]string{}, cfg.Tools...)
	cfg.Bindings = slices.Clone(cfg.Bindings)
	for i := range cfg.Bindings {
		b := &cfg.Bindings[i]
		b.Operations = slices.Clone(b.Operations)
		b.Parameters = slices.Clone(b.Parameters)
		b.Runtime = cloneTools(b.Runtime)
	}
	return cfg
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
	local, err := e.opts.Store.List(ctx, g.Tenant, after, limit)
	if err != nil || e.opts.Archive == nil {
		return local, err
	}
	remote, err := e.opts.Archive.List(ctx, g.Tenant, after, limit)
	if err != nil {
		return nil, err
	}
	seen := make(map[string]bool)
	for _, s := range local {
		seen[s.ID] = true
	}
	for _, s := range remote {
		if !seen[s.ID] {
			if tenant, err := e.opts.Store.Deleted(ctx, s.ID); err != nil {
				return nil, err
			} else if tenant != "" {
				continue
			}
			local = append(local, s)
		}
	}
	sort.Slice(local, func(i, j int) bool { return local[i].ID < local[j].ID })
	return local[:min(len(local), limit)], nil
}

func (e *Engine) Create(ctx context.Context, g core.Grant, input core.CreateSession) (core.Session, error) {
	if !g.Manage || g.Session != "" {
		return core.Session{}, core.E(core.Forbidden, "session management is not permitted")
	}
	if _, ok := e.opts.Factories[input.Harness]; !ok {
		return core.Session{}, core.E(core.Unsupported, "harness is not configured")
	}
	if !strings.Contains(input.Config.Model, "/") || len(input.Config.Model) > 256 || len(input.Config.Instructions) > e.opts.MaxInputBytes || len(input.Config.Tools)+len(input.Config.Bindings) > 128 {
		return core.Session{}, core.E(core.Invalid, "invalid session configuration")
	}
	switch input.Config.ReasoningEffort {
	case "", "off", "minimal", "low", "medium", "high", "xhigh", "max":
	default:
		return core.Session{}, core.E(core.Invalid, "invalid reasoning effort")
	}
	e.mu.Lock()
	closed := e.closed
	e.mu.Unlock()
	if closed {
		return core.Session{}, core.E(core.Overloaded, "server is draining")
	}
	if e.syncer != nil && !e.syncer.admit() {
		return core.Session{}, core.E(core.Overloaded, "external synchronization backlog is full")
	}
	now := time.Now().UTC()
	s := core.Session{ID: core.ID(), Tenant: g.Tenant, Harness: input.Harness, Config: input.Config, Status: "idle", CreatedAt: now, UpdatedAt: now}
	admitted := false
	defer func() {
		if !admitted {
			if closer, ok := e.opts.Tools.(interface{ CloseSession(string) }); ok {
				closer.CloseSession(s.ID)
			}
		}
	}()
	if s.Config.Tools == nil {
		s.Config.Tools = []string{}
	}
	var err error
	if input.ModelCredential != nil {
		s.CredentialBindings = append(s.CredentialBindings, credentials.ModelBinding)
		value, _ := json.Marshal(input.ModelCredential)
		// Copy before appending: admission must not mutate the caller's inputs.
		input.Credentials = append(slices.Clone(input.Credentials), core.CredentialInput{Binding: credentials.ModelBinding, Value: value})
	}
	for _, credential := range input.Credentials {
		if credential.Binding == credentials.ModelBinding && input.ModelCredential == nil {
			return core.Session{}, core.E(core.Invalid, "model credentials require model_credential")
		}
	}
	s.Credentials, err = credentials.Seal(ctx, e.opts.Keys, s, input.Credentials)
	if err != nil {
		return core.Session{}, err
	}
	s.CredentialBindings = nil
	for _, record := range s.Credentials {
		s.CredentialBindings = append(s.CredentialBindings, record.Binding)
	}
	provider, err := e.modelProvider(ctx, s)
	if err != nil {
		return core.Session{}, err
	}
	if factory, ok := e.opts.Factories[s.Harness].(harness.Validator); ok {
		if err := factory.ValidateSession(s.Config, provider); err != nil {
			return core.Session{}, err
		}
	}
	if err := e.opts.Tools.Bind(ctx, &s); err != nil {
		return core.Session{}, err
	}
	s.Checkpoint = &core.Checkpoint{ID: core.ID(), Config: s.Config, Time: now, Files: []core.NativeFile{}}
	if err := e.opts.Store.Create(ctx, s); err != nil {
		return core.Session{}, err
	}
	admitted = true
	e.dirty(s.ID)
	return s, nil
}

func (e *Engine) Log(ctx context.Context, g core.Grant, id string) (*store.Log, func(), error) {
	if _, err := e.inspect(ctx, g, id); err != nil {
		return nil, nil, err
	}
	a, err := e.load(ctx, id)
	if err != nil {
		return nil, nil, err
	}
	return a.log, func() { e.release(a) }, nil
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
	ev, err = a.log.Append(ev)
	if err == nil {
		e.dirty(a.meta.ID)
	}
	return ev, err
}

func (e *Engine) status(a *entry, status, input string) error {
	if _, err := e.emit(a, "session.status", input, core.Status{Status: status}); err != nil {
		return err
	}
	a.meta.Status = status
	if status == "closed" {
		if closer, ok := e.opts.Tools.(interface{ CloseSession(string) }); ok {
			closer.CloseSession(a.meta.ID)
		}
	}
	a.meta.UpdatedAt = time.Now().UTC()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	return e.opts.Store.Save(ctx, a.meta)
}

func (e *Engine) Submit(ctx context.Context, g core.Grant, id string, input core.Input) (core.Event, error) {
	if input.Type != "user.message" || (strings.TrimSpace(input.Text) == "" && len(input.Media) == 0) || len(input.Text) > e.opts.MaxInputBytes || len(input.Key) > 128 {
		return core.Event{}, core.E(core.Invalid, "invalid message")
	}
	if len(input.Media) > 64 {
		return core.Event{}, core.E(core.Invalid, "too many media inputs")
	}
	for _, media := range input.Media {
		u, err := url.Parse(media.URL)
		if err != nil || media.Type != "image" || len(media.URL) > 4096 || u.User != nil || u.Fragment != "" {
			return core.Event{}, core.E(core.Invalid, "invalid image input")
		}
		if !slices.Contains(e.opts.MediaOrigins, u.Scheme+"://"+u.Host) {
			return core.Event{}, core.E(core.Forbidden, "media origin is not authorized")
		}
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
	defer e.release(a)
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
	if e.syncer != nil && !e.syncer.admit() {
		return core.Event{}, core.E(core.Overloaded, "external synchronization backlog is full")
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
		if pending, evidenceErr := e.settleModelRequests(a, q.event.ID); evidenceErr != nil {
			err = errors.Join(err, evidenceErr)
		} else if pending && err == nil {
			err = errors.New("harness left model requests unsettled")
		}
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
			e.dropRunner(a)
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

func (e *Engine) run(ctx context.Context, a *entry, q queued) (runErr error) {
	ctx, span := otel.Tracer("github.com/aexhq/brain").Start(ctx, "session.execute", trace.WithAttributes(attribute.String("session.id", a.meta.ID), attribute.String("harness", a.meta.Harness)))
	defer span.End()
	a.mu.Lock()
	a.executing = true
	a.runCtx = ctx
	a.firstOutput = q.event.Time
	a.mu.Unlock()
	defer func() {
		if runErr != nil {
			e.dropRunner(a)
		}
		a.mu.Lock()
		a.executing = false
		a.runCtx = nil
		a.mu.Unlock()
	}()
	if e.opts.Metrics != nil {
		e.opts.Metrics.QueueDelay.Record(ctx, time.Since(q.event.Time).Seconds())
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
	metadata := clone(a.meta)
	a.mu.Unlock()
	if needsRestore {
		a.nativeMu.Lock()
		err := restoreNative(nativeDir, checkpoint)
		a.nativeMu.Unlock()
		if err != nil {
			return err
		}
		a.mu.Lock()
		a.restore = false
		a.mu.Unlock()
	}
	provider, err := e.modelProvider(ctx, metadata)
	if err != nil {
		return err
	}
	selected, err := e.opts.Tools.Describe(ctx, metadata)
	if err != nil {
		return err
	}
	selectedNames := make(map[string]bool)
	for _, tool := range selected {
		selectedNames[tool.Name] = true
	}
	a.mu.Lock()
	runner := a.runner
	credentialsChanged := runner != nil && a.provider != provider
	a.mu.Unlock()
	if credentialsChanged {
		e.dropRunner(a)
		runner = nil
	}
	if runner == nil {
		if err := e.acquireRunner(ctx); err != nil {
			return err
		}
		started := time.Now()
		runner, err = e.opts.Factories[a.meta.Harness].Open(ctx, harness.Options{MediaOrigins: e.opts.MediaOrigins, Lifetime: e.ctx, SessionID: a.meta.ID, Dir: nativeDir, Config: cfg, Tools: selected, Provider: provider,
			Event: func(kind string, data json.RawMessage) error {
				if kind == "model.request" {
					return e.observeModel(a, data)
				}
				if kind != "assistant.message" && kind != "assistant.delta" {
					return errors.New("unsupported native event")
				}
				a.mu.Lock()
				inputID := a.current
				first := a.firstOutput
				a.firstOutput = time.Time{}
				a.mu.Unlock()
				if inputID == "" {
					return errors.New("harness emitted output without active input")
				}
				if !first.IsZero() && e.opts.Metrics != nil {
					e.opts.Metrics.FirstOutput.Record(e.ctx, time.Since(first).Seconds())
				}
				_, err := e.emit(a, kind, inputID, data)
				return err
			},
			Call: func(callCtx context.Context, call core.Call) (core.Result, error) {
				a.mu.Lock()
				inputID := a.current
				hostCtx := callCtx
				callCtx = a.runCtx
				a.mu.Unlock()
				if inputID == "" || callCtx == nil {
					return core.Result{}, errors.New("harness invoked a tool without active input")
				}
				callCtx, cancel := context.WithCancel(callCtx)
				stop := context.AfterFunc(hostCtx, cancel)
				defer func() { stop(); cancel() }()
				callCtx, span := otel.Tracer("github.com/aexhq/brain").Start(callCtx, "tool.call", trace.WithAttributes(attribute.String("tool.name", call.Name)))
				defer span.End()
				if !selectedNames[call.Name] {
					return core.Result{}, errors.New("harness called an unbound tool")
				}
				call.ID = core.ID()
				if _, err := e.emit(a, "tool.call", inputID, call); err != nil {
					return core.Result{}, err
				}
				scope := core.ToolScope{Tenant: a.meta.Tenant, Session: a.meta.ID, Call: call.ID}
				revoke := func() {}
				if e.opts.Inspection != nil && e.opts.Tools.NeedsInspection(call.Name) {
					scope.InspectionToken, scope.BrainURL, revoke = e.opts.Inspection(metadata)
				}
				defer revoke()
				started := time.Now()
				current, err := e.opts.Store.Get(callCtx, a.meta.ID)
				if err != nil {
					return core.Result{}, err
				}
				result, err := e.opts.Tools.Invoke(callCtx, current, scope, call, func(data json.RawMessage) {
					_, _ = e.emit(a, "tool.progress", inputID, map[string]any{"call_id": call.ID, "data": data})
				})
				if err != nil {
					return core.Result{}, err
				}
				if e.opts.Metrics != nil {
					outcome := "success"
					if result.IsError {
						outcome = "tool_error"
					}
					attrs := metric.WithAttributes(attribute.String("tool", call.Name), attribute.String("outcome", outcome))
					e.opts.Metrics.ToolCalls.Add(callCtx, 1, attrs)
					e.opts.Metrics.ToolDuration.Record(callCtx, time.Since(started).Seconds(), attrs)
				}
				if _, err := e.emit(a, "tool.result", inputID, core.ToolResult{CallID: call.ID, Result: result}); err != nil {
					return core.Result{}, err
				}
				return result, nil
			},
		})
		if e.opts.Metrics != nil {
			e.opts.Metrics.RunnerStartup.Record(ctx, time.Since(started).Seconds(), metric.WithAttributes(attribute.String("harness", a.meta.Harness)))
		}
		if err != nil {
			<-e.resident
			return err
		}
		a.mu.Lock()
		a.runner = runner
		a.provider = provider
		a.mu.Unlock()
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := runner.Prompt(ctx, core.Text{Text: q.input.Text, Media: q.input.Media}); err != nil {
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
	if e.opts.Metrics != nil {
		previous := map[string]int64{}
		if a.meta.Checkpoint != nil {
			for _, f := range a.meta.Checkpoint.Files {
				previous[f.Path] = f.Size
			}
		}
		var growth int64
		for _, f := range files {
			growth += max(0, f.Size-previous[f.Path])
		}
		e.opts.Metrics.NativeBytes.Add(ctx, growth, metric.WithAttributes(attribute.String("harness", a.meta.Harness)))
	}
	a.meta.Checkpoint = &core.Checkpoint{ID: core.ID(), Offset: a.log.Size(), Files: files, Config: cfg, Time: time.Now().UTC()}
	err = e.opts.Store.Save(ctx, a.meta)
	if err == nil {
		e.dirty(a.meta.ID)
	}
	return err
}

func (e *Engine) dropRunner(a *entry) {
	a.mu.Lock()
	runner := a.runner
	a.runner = nil
	a.mu.Unlock()
	if runner != nil {
		runner.Close()
		<-e.resident
	}
}

func (e *Engine) acquireRunner(ctx context.Context) error {
	select {
	case e.resident <- struct{}{}:
		return nil
	default:
	}
	e.mu.Lock()
	entries := make([]*entry, 0, len(e.entries))
	for _, a := range e.entries {
		entries = append(entries, a)
	}
	e.mu.Unlock()
	for _, a := range entries {
		select {
		case <-a.ready:
		default:
			continue
		}
		a.mu.Lock()
		if a.executing || a.runner == nil {
			a.mu.Unlock()
			continue
		}
		runner := a.runner
		a.runner = nil
		a.mu.Unlock()
		runner.Close()
		<-e.resident
		break
	}
	select {
	case e.resident <- struct{}{}:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
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
	defer e.release(a)
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
	defer e.release(a)
	a.nativeMu.Lock()
	defer a.nativeMu.Unlock()
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.working || a.closing || a.meta.Status == "closed" {
		return core.Session{}, core.E(core.Conflict, "session cannot resume in its current state")
	}
	if a.meta.Status == "idle" {
		return clone(a.meta), nil
	}
	dir, err := e.opts.Store.Dir(id)
	if err != nil {
		return core.Session{}, err
	}
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
	defer e.release(a)
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
	e.dropRunner(a)
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
			if e.syncer != nil {
				<-e.syncer.done
			}
			e.mu.Lock()
			entries := make([]*entry, 0, len(e.entries))
			for _, a := range e.entries {
				entries = append(entries, a)
			}
			e.mu.Unlock()
			var errs []error
			for _, a := range entries {
				<-a.ready
				e.dropRunner(a)
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
