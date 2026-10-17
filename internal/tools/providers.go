package tools

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/credentials"
	"reflect"
	"regexp"
)

// BindingContext separates application-authorized resource configuration from
// model arguments. Bind must only resolve descriptors and routing, not actions.
type BindingContext struct {
	Tenant, Session, Alias string
	Parameters             json.RawMessage
	Credential             json.RawMessage
}

type Provider struct {
	Parameters json.RawMessage
	Authorize  func(context.Context, BindingContext) error
	Bind       func(context.Context, BindingContext) ([]Binding, error)
}

// Unavailable marks a dependency outage before an action, distinct from invalid
// binding configuration. Saved descriptors remain valid while execution fails.
type Unavailable struct{ Message string }

func (e *Unavailable) Error() string { return e.Message }

type RuntimeProvider interface {
	Resolve(core.Session, core.ToolBinding) ([]Binding, error)
	CloseSession(string)
}

func (r *Registry) CloseSession(id string) {
	if r.Runtime != nil {
		r.Runtime.CloseSession(id)
	}
}

type handlerTransport struct{ handler Handler }

func (h handlerTransport) Call(ctx context.Context, _ Binding, r Request, progress func(json.RawMessage)) core.Result {
	return h.handler(ctx, r, progress)
}
func (handlerTransport) Close() {}

func ValidateProviders(providers map[string]Provider) error {
	for name, p := range providers {
		if !toolName.MatchString(name) || p.Authorize == nil || p.Bind == nil || len(p.Parameters) == 0 {
			return fmt.Errorf("invalid provider %q", name)
		}
		if _, err := compile(p.Parameters); err != nil {
			return fmt.Errorf("provider %s parameters: %w", name, err)
		}
	}
	return nil
}

var toolName = regexp.MustCompile(`^[a-zA-Z][a-zA-Z0-9_]{0,63}$`)

func (r *Registry) resolve(ctx context.Context, s core.Session) (*Registry, error) {
	bindings := []Binding{}
	for _, name := range s.Config.Tools {
		t, ok := r.tools[name]
		if !ok || !allowed(t.binding, s.Tenant) {
			return nil, core.E(core.Forbidden, "tool is not available: "+name)
		}
		bindings = append(bindings, t.binding)
	}
	aliases := map[string]bool{}
	for _, b := range s.Config.Bindings {
		if !toolName.MatchString(b.Alias) || aliases[b.Alias] {
			return nil, core.E(core.Invalid, "invalid or duplicate binding alias")
		}
		aliases[b.Alias] = true
		var available []Binding
		var err error
		if b.Provider != "" {
			if b.Executor != "" || len(b.Runtime) != 0 || len(b.Operations) == 0 {
				return nil, core.E(core.Invalid, "managed binding requires explicit operations")
			}
			p, ok := r.Providers[b.Provider]
			if !ok {
				return nil, core.E(core.Forbidden, "provider is not installed")
			}
			params := b.Parameters
			if len(params) == 0 {
				params = json.RawMessage(`{}`)
			}
			schema, err := compile(p.Parameters)
			if err != nil {
				return nil, err
			}
			var value any
			if json.Unmarshal(params, &value) != nil || schema == nil || schema.Validate(value) != nil {
				return nil, core.E(core.Invalid, "invalid provider parameters")
			}
			secret, err := credentials.Open(ctx, r.Keys, s, b.Alias)
			if err != nil {
				return nil, err
			}
			bc := BindingContext{Tenant: s.Tenant, Session: s.ID, Alias: b.Alias, Parameters: params, Credential: secret}
			if err := p.Authorize(ctx, bc); err != nil {
				return nil, core.E(core.Forbidden, "provider binding is not authorized")
			}
			available, err = p.Bind(ctx, bc)
			if err != nil {
				return nil, err
			}
		} else {
			if b.Executor == "" || len(b.Runtime) == 0 || len(b.Operations) != 0 || len(b.Parameters) != 0 || r.Runtime == nil {
				return nil, core.E(core.Invalid, "invalid runtime binding")
			}
			available, err = r.Runtime.Resolve(s, b)
			if err != nil {
				return nil, err
			}
		}
		byName := map[string]Binding{}
		for _, def := range available {
			if _, ok := byName[def.Name]; ok {
				return nil, errors.New("provider returned duplicate operation")
			}
			byName[def.Name] = def
		}
		operations := b.Operations
		if b.Provider == "" {
			for _, tool := range b.Runtime {
				operations = append(operations, tool.Name)
			}
		}
		for _, name := range operations {
			def, ok := byName[name]
			if !ok {
				return nil, core.E(core.Invalid, "selected provider operation is unavailable")
			}
			def.Name = b.Alias + "_" + name
			def.Tenants = []string{s.Tenant}
			bindings = append(bindings, def)
		}
	}
	if len(bindings) > 128 {
		return nil, core.E(core.Invalid, "too many selected tools")
	}
	bound, err := NewWithTransports(bindings, r.transports)
	if err != nil {
		return nil, core.E(core.Invalid, "invalid tool binding: "+err.Error())
	}
	return bound, nil
}

func descriptors(r *Registry, s core.Session) []core.Tool {
	result := make([]core.Tool, 0, len(r.tools))
	for _, name := range s.Config.Tools {
		result = append(result, r.tools[name].binding.Tool)
	}
	for _, b := range s.Config.Bindings {
		ops := b.Operations
		if b.Provider == "" {
			for _, t := range b.Runtime {
				ops = append(ops, t.Name)
			}
		}
		for _, op := range ops {
			result = append(result, r.tools[b.Alias+"_"+op].binding.Tool)
		}
	}
	return result
}

func sameDescriptors(a, b []core.Tool) bool {
	var av, bv any
	ab, _ := json.Marshal(a)
	bb, _ := json.Marshal(b)
	_ = json.Unmarshal(ab, &av)
	_ = json.Unmarshal(bb, &bv)
	return reflect.DeepEqual(av, bv)
}

func (r *Registry) Bind(ctx context.Context, s *core.Session) error {
	bound, err := r.resolve(ctx, *s)
	if err != nil {
		return err
	}
	s.Descriptors = descriptors(bound, *s)
	return nil
}

func (r *Registry) Describe(ctx context.Context, s core.Session) ([]core.Tool, error) {
	bound, err := r.resolve(ctx, s)
	if err != nil {
		var unavailable *Unavailable
		if errors.As(err, &unavailable) && s.Descriptors != nil {
			return s.Descriptors, nil
		}
		return nil, err
	}
	current := descriptors(bound, s)
	if !sameDescriptors(current, s.Descriptors) {
		return nil, core.E(core.Conflict, "saved tool descriptors differ from installed bindings")
	}
	return current, nil
}

func (r *Registry) Invoke(ctx context.Context, s core.Session, scope core.ToolScope, call core.Call, progress func(json.RawMessage)) (core.Result, error) {
	bound, err := r.resolve(ctx, s)
	if err != nil {
		var unavailable *Unavailable
		if core.ErrorCode(err) == core.Forbidden || errors.As(err, &unavailable) {
			return core.ToolError(err.Error(), false), nil
		}
		return core.Result{}, err
	}
	if !sameDescriptors(descriptors(bound, s), s.Descriptors) {
		return core.Result{}, core.E(core.Conflict, "saved tool descriptors differ from installed bindings")
	}
	return bound.Call(ctx, scope, call, progress), nil
}
