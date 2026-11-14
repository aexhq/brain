// Package hosted binds uploaded modules to the same canonical call as remote
// and browser tools. Customer code executes only through Sandbox.
package hosted

import (
	"archive/tar"
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
	sandbox "github.com/aexhq/brain/services/sandbox/api"
	storage "github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/tools/api"
	"github.com/aexhq/brain/services/tools/registry"
)

type Service struct {
	Storage *storage.Client
	Sandbox *sandbox.Client
}

func (s *Service) Upload(ctx context.Context, tenant string, input api.Upload) (api.Package, error) {
	if input.Module == "" || len(input.Module) > 1<<20 {
		return api.Package{}, core.E(core.Invalid, "module must contain at most 1 MiB of bundled JavaScript")
	}
	r, e := tools.New([]tools.Binding{{Tool: input.Tool, Tenants: []string{tenant}, Handler: func(context.Context, tools.Request, func(json.RawMessage)) core.Result { return core.Result{} }}})
	if e != nil {
		return api.Package{}, core.E(core.Invalid, e.Error())
	}
	r.Close()
	b, e := json.Marshal(input)
	if e != nil {
		return api.Package{}, e
	}
	i := storage.Identity{Tenant: tenant, Session: "hosted-tools", Collection: "packages"}
	ref, _, e := s.Storage.Upload(ctx, i, bytes.NewReader(b), 2<<20)
	if e != nil {
		return api.Package{}, e
	}
	_, e = s.Storage.Do(ctx, storage.Request{Operation: "object.commit", Identity: i, Reference: ref})
	return api.Package{Reference: ref, Tool: input.Tool}, e
}
func (s *Service) load(ctx context.Context, tenant, ref string) (api.Upload, error) {
	var v api.Upload
	if !core.ValidObjectReference(ref) {
		return v, core.E(core.Invalid, "invalid package reference")
	}
	var b bytes.Buffer
	_, e := s.Storage.Download(ctx, storage.Identity{Tenant: tenant, Session: "hosted-tools", Collection: "packages"}, ref, &b, 2<<20)
	if e != nil {
		return v, e
	}
	e = json.Unmarshal(b.Bytes(), &v)
	return v, e
}
func (s *Service) Provider() tools.Provider {
	return tools.Provider{
		Parameters: json.RawMessage(`{"type":"object","properties":{"package":{"type":"string","pattern":"^[a-f0-9]{64}$"}},"required":["package"],"additionalProperties":false}`),
		Authorize:  func(context.Context, tools.BindingContext) error { return nil },
		Bind: func(ctx context.Context, b tools.BindingContext) ([]tools.Binding, error) {
			var p struct {
				Package string `json:"package"`
			}
			if e := json.Unmarshal(b.Parameters, &p); e != nil {
				return nil, e
			}
			v, e := s.load(ctx, b.Tenant, p.Package)
			if e != nil {
				return nil, e
			}
			return []tools.Binding{{Tool: v.Tool, Handler: func(ctx context.Context, r tools.Request, progress func(json.RawMessage)) core.Result {
				return s.invoke(ctx, v.Module, r, progress)
			}}}, nil
		},
	}
}
func (s *Service) invoke(ctx context.Context, module string, r tools.Request, progress func(json.RawMessage)) core.Result {
	env, e := s.Sandbox.Create(ctx, sandbox.Create{Tenant: r.Tenant, Session: r.Session, Profile: "tool"})
	if e != nil {
		return core.ToolError("hosted tool capacity is unavailable", false)
	}
	defer func() {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		_ = s.Sandbox.Remove(ctx, env.ID)
	}()
	var archive bytes.Buffer
	tw := tar.NewWriter(&archive)
	if e = tw.WriteHeader(&tar.Header{Name: "tool.mjs", Mode: 0600, Size: int64(len(module))}); e != nil {
		return core.ToolError("hosted package failed", false)
	}
	_, e = tw.Write([]byte(module))
	e = errors.Join(e, tw.Close())
	if e != nil {
		return core.ToolError("hosted package failed", false)
	}
	if e = s.Sandbox.Import(ctx, env.ID, &archive); e != nil {
		return core.ToolError("hosted package import failed", false)
	}
	stream, e := s.Sandbox.Process(ctx, env.ID, sandbox.Process{Command: []string{"node", "/app/tools/hosted/runner.mjs"}})
	if e != nil {
		return core.ToolError("hosted process could not start", false)
	}
	defer stream.Close()
	if e = json.NewEncoder(stream).Encode(r); e != nil {
		return core.ToolError("hosted tool connection failed", true)
	}
	scan := bufio.NewScanner(stream)
	scan.Buffer(make([]byte, 4096), 2<<20)
	for scan.Scan() {
		var f api.Frame
		if e = json.Unmarshal(scan.Bytes(), &f); e != nil {
			return core.ToolError("hosted tool emitted invalid output", true)
		}
		if len(f.Progress) > 0 {
			progress(f.Progress)
		}
		if f.Result != nil {
			return *f.Result
		}
	}
	return core.ToolError("hosted tool exited without a result", true)
}
