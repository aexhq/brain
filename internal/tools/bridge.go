package tools

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"github.com/aexhq/brain/internal/core"
	"sync"
	"time"
)

type ExecutorRegistration struct {
	Session string      `json:"session,omitempty"`
	Binding string      `json:"binding"`
	Tools   []core.Tool `json:"tools"`
}
type ExecutorCredential struct {
	ID    string `json:"id"`
	Token string `json:"token"`
}
type Command struct {
	Type       string          `json:"type"`
	Connection string          `json:"connection"`
	CallID     string          `json:"call_id,omitempty"`
	Tool       string          `json:"tool,omitempty"`
	Arguments  json.RawMessage `json:"arguments,omitempty"`
	Scope      *core.ToolScope `json:"scope,omitempty"`
}
type Reply struct {
	Connection string          `json:"connection"`
	CallID     string          `json:"call_id"`
	Result     *core.Result    `json:"result,omitempty"`
	Progress   json.RawMessage `json:"progress,omitempty"`
}
type pendingCall struct {
	result   chan core.Result
	progress func(json.RawMessage)
}
type Connection struct {
	ID       string
	Commands chan Command
	Done     chan struct{}
	pending  map[string]*pendingCall
}
type executor struct {
	tenant, session, binding string
	tools                    []core.Tool
	key                      [32]byte
	connection               *Connection
	created                  time.Time
}

// Bridge owns live callback routes, separately from replayable event history.
type Bridge struct {
	mu        sync.Mutex
	executors map[string]*executor
	tokens    map[[32]byte]string
	limit     int
	closed    bool
}

func NewBridge(limit int) *Bridge {
	return &Bridge{executors: map[string]*executor{}, tokens: map[[32]byte]string{}, limit: limit}
}

func (b *Bridge) Register(tenant string, input ExecutorRegistration, saved *core.Session) (ExecutorCredential, error) {
	if !toolName.MatchString(input.Binding) || len(input.Tools) == 0 || len(input.Tools) > 128 {
		return ExecutorCredential{}, core.E(core.Invalid, "invalid executor registration")
	}
	defs := []Binding{}
	for _, t := range input.Tools {
		defs = append(defs, Binding{Tool: t, Tenants: []string{tenant}, Handler: func(context.Context, Request, func(json.RawMessage)) core.Result { return core.Result{} }})
	}
	if _, err := NewWithTransports(defs, nil); err != nil {
		return ExecutorCredential{}, core.E(core.Invalid, "invalid runtime descriptors")
	}
	id := core.ID()
	if saved != nil {
		id = ""
		for _, binding := range saved.Config.Bindings {
			if binding.Alias == input.Binding && binding.Executor != "" && sameDescriptors(binding.Runtime, input.Tools) {
				id = binding.Executor
			}
		}
		if saved.Tenant != tenant || saved.Status == "closed" || id == "" {
			return ExecutorCredential{}, core.E(core.Forbidden, "runtime binding does not match saved session")
		}
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return ExecutorCredential{}, core.E(core.Overloaded, "runtime bridge is closed")
	}
	// Expire provisional registrations whose client failed before admission.
	for old, e := range b.executors {
		if e.session == "" && time.Since(e.created) > time.Minute {
			b.remove(old, e)
		}
	}
	if old := b.executors[id]; old != nil {
		if old.connection != nil {
			return ExecutorCredential{}, core.E(core.Conflict, "executor is connected")
		}
		b.remove(id, old)
	}
	if len(b.executors) >= b.limit {
		return ExecutorCredential{}, core.E(core.Overloaded, "executor capacity reached")
	}
	token := core.ID() + core.ID()
	key := sha256.Sum256([]byte(token))
	b.executors[id] = &executor{tenant: tenant, session: input.Session, binding: input.Binding, tools: input.Tools, key: key, created: time.Now()}
	b.tokens[key] = id
	return ExecutorCredential{ID: id, Token: token}, nil
}

func (b *Bridge) Authenticate(token string) (string, string, bool) {
	b.mu.Lock()
	defer b.mu.Unlock()
	id := b.tokens[sha256.Sum256([]byte(token))]
	e := b.executors[id]
	if e != nil && e.session == "" && time.Since(e.created) > time.Minute {
		b.remove(id, e)
		e = nil
	}
	if e == nil {
		return "", "", false
	}
	return id, e.tenant, true
}
func (b *Bridge) Attach(id string) (*Connection, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	e := b.executors[id]
	if e == nil {
		return nil, core.E(core.NotFound, "executor unavailable")
	}
	if e.connection != nil {
		return nil, core.E(core.Conflict, "executor is connected")
	}
	c := &Connection{ID: core.ID(), Commands: make(chan Command, 64), Done: make(chan struct{}), pending: map[string]*pendingCall{}}
	e.connection = c
	return c, nil
}
func (b *Bridge) Detach(id string, c *Connection) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if e := b.executors[id]; e != nil && e.connection == c {
		close(c.Done)
		e.connection = nil
	}
}
func (b *Bridge) remove(id string, e *executor) {
	if e.connection != nil {
		close(e.connection.Done)
	}
	delete(b.tokens, e.key)
	delete(b.executors, id)
}
func (b *Bridge) Unregister(id string) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if e := b.executors[id]; e != nil {
		b.remove(id, e)
	}
}
func (b *Bridge) CloseSession(id string) {
	b.mu.Lock()
	defer b.mu.Unlock()
	for key, e := range b.executors {
		if e.session == id {
			b.remove(key, e)
		}
	}
}
func (b *Bridge) Close() {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.closed = true
	for id, e := range b.executors {
		b.remove(id, e)
	}
}

func (b *Bridge) Resolve(s core.Session, binding core.ToolBinding) ([]Binding, error) {
	b.mu.Lock()
	e := b.executors[binding.Executor]
	if e != nil {
		if e.tenant != s.Tenant || e.binding != binding.Alias || (e.session != "" && e.session != s.ID) || !sameDescriptors(e.tools, binding.Runtime) {
			b.mu.Unlock()
			return nil, core.E(core.Forbidden, "executor scope or descriptor mismatch")
		}
		if s.Descriptors == nil && e.connection == nil {
			b.mu.Unlock()
			return nil, core.E(core.Conflict, "executor must connect before session creation")
		}
		e.session = s.ID
	} else if s.Descriptors == nil {
		b.mu.Unlock()
		return nil, core.E(core.Invalid, "executor is unavailable")
	}
	b.mu.Unlock()
	result := []Binding{}
	for _, tool := range binding.Runtime {
		result = append(result, Binding{Tool: tool, Tenants: []string{s.Tenant}, Handler: func(ctx context.Context, r Request, progress func(json.RawMessage)) core.Result {
			return b.call(ctx, binding.Executor, tool.Name, r, progress)
		}})
	}
	return result, nil
}

func (b *Bridge) call(ctx context.Context, id, name string, r Request, progress func(json.RawMessage)) core.Result {
	b.mu.Lock()
	e := b.executors[id]
	if e == nil || e.connection == nil || e.tenant != r.Tenant || e.session != r.Session {
		b.mu.Unlock()
		return core.ToolError("runtime executor unavailable before dispatch", false)
	}
	c := e.connection
	if len(c.pending) >= 32 {
		b.mu.Unlock()
		return core.ToolError("runtime executor call capacity reached", false)
	}
	p := &pendingCall{result: make(chan core.Result, 1), progress: progress}
	c.pending[r.Call] = p
	command := Command{Type: "call", Connection: c.ID, CallID: r.Call, Tool: name, Arguments: r.Arguments, Scope: &r.ToolScope}
	select {
	case c.Commands <- command:
	default:
		delete(c.pending, r.Call)
		b.mu.Unlock()
		return core.ToolError("runtime executor command capacity reached", false)
	}
	b.mu.Unlock()
	defer func() { b.mu.Lock(); delete(c.pending, r.Call); b.mu.Unlock() }()
	select {
	case result := <-p.result:
		return result
	case <-c.Done:
		return core.ToolError("runtime connection lost; action may already have happened", true)
	case <-ctx.Done():
		b.mu.Lock()
		if e.connection == c {
			select {
			case c.Commands <- Command{Type: "cancel", Connection: c.ID, CallID: r.Call}:
			default:
				close(c.Done)
				e.connection = nil
			}
		}
		b.mu.Unlock()
		return core.ToolError("runtime call cancelled; action may already have happened", true)
	}
}

func (b *Bridge) Reply(id string, reply Reply) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	e := b.executors[id]
	if e == nil || e.connection == nil || e.connection.ID != reply.Connection {
		return core.E(core.Conflict, "stale executor connection")
	}
	p := e.connection.pending[reply.CallID]
	if p == nil {
		return core.E(core.Conflict, "call is not outstanding")
	}
	if (reply.Result == nil) == (len(reply.Progress) == 0) {
		return core.E(core.Invalid, "reply requires one progress or result payload")
	}
	if reply.Result != nil {
		delete(e.connection.pending, reply.CallID)
		p.result <- *reply.Result
		return nil
	}
	if len(reply.Progress) > 64<<10 || !json.Valid(reply.Progress) {
		return core.E(core.Invalid, "invalid progress payload")
	}
	if p.progress != nil {
		p.progress(reply.Progress)
	}
	return nil
}
