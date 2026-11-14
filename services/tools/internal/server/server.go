package server

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"sync"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
	storage "github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/tools/api"
	"github.com/aexhq/brain/services/tools/internal/hosted"
	"github.com/aexhq/brain/services/tools/registry"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
)

type Server struct {
	Registry *tools.Registry
	Bridge   *tools.Bridge
	Hosted   *hosted.Service
	Storage  *storage.Client
	Auth     *core.Auth
	Token    string
}

func (s *Server) Handler() (http.Handler, error) {
	if s.Registry == nil || s.Bridge == nil || s.Auth == nil || s.Storage == nil || s.Hosted == nil || len(s.Token) < 32 {
		return nil, errors.New("tools dependencies and service authentication are required")
	}
	m := http.NewServeMux()
	m.HandleFunc("GET /health/live", func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(200) })
	m.HandleFunc("POST /rpc", s.rpc)
	m.HandleFunc("POST /tools/packages", s.upload)
	executors := (&api.ExecutorServer{Bridge: s.Bridge, Sessions: s, Auth: s.Auth}).Handler()
	m.Handle("/executors", executors)
	m.Handle("/executors/", executors)
	return m, nil
}
func (s *Server) Get(ctx context.Context, g core.Grant, id string) (core.Session, error) {
	r, e := s.Storage.Do(ctx, storage.Request{Operation: "session.get", Identity: storage.Identity{Tenant: g.Tenant, Session: id}})
	if e != nil {
		return core.Session{}, e
	}
	if r.Record == nil || r.Record.Deleted {
		return core.Session{}, core.E(core.NotFound, "session not found")
	}
	v := r.Record.Session
	v.Tenant = g.Tenant
	if !g.Allows(v) {
		return core.Session{}, core.E(core.NotFound, "session not found")
	}
	return v, nil
}
func (s *Server) upload(w http.ResponseWriter, r *http.Request) {
	fail := func(status int, code core.Code, message string) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_ = json.NewEncoder(w).Encode(map[string]any{"error": core.Error{Code: code, Message: message}})
	}
	g, ok := s.Auth.Authenticate(r)
	if !ok || !g.Manage || g.ReadOnly || g.Session != "" {
		fail(403, core.Forbidden, "management authentication required")
		return
	}
	var in api.Upload
	d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 2<<20))
	d.DisallowUnknownFields()
	if e := d.Decode(&in); e != nil || d.Decode(new(any)) != io.EOF {
		fail(400, core.Invalid, "invalid package upload")
		return
	}
	out, e := s.Hosted.Upload(r.Context(), g.Tenant, in)
	if e != nil {
		slog.Error("hosted package upload failed", "error", e)
		var typed *core.Error
		if errors.As(e, &typed) && typed.Code == core.Invalid {
			fail(400, typed.Code, typed.Message)
		} else {
			fail(500, core.Internal, "package upload failed")
		}
		return
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(201)
	_ = json.NewEncoder(w).Encode(out)
}
func (s *Server) rpc(w http.ResponseWriter, r *http.Request) {
	if subtle.ConstantTimeCompare([]byte(r.Header.Get("Authorization")), []byte("Bearer "+s.Token)) != 1 {
		http.Error(w, "service authentication required", 401)
		return
	}
	var in api.Request
	d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 2<<20))
	d.DisallowUnknownFields()
	if e := d.Decode(&in); e != nil || d.Decode(new(any)) != io.EOF {
		http.Error(w, "invalid tool request", 400)
		return
	}
	in.Session.Tenant = in.Tenant
	in.Session.Credentials = in.Credentials
	ctx := otel.GetTextMapPropagator().Extract(r.Context(), propagation.HeaderCarrier(r.Header))
	ctx, span := otel.Tracer("github.com/aexhq/brain/tools").Start(ctx, "tools.request")
	defer span.End()
	w.Header().Set("Content-Type", "application/x-ndjson")
	rc := http.NewResponseController(w)
	var mu sync.Mutex
	write := func(f api.Frame) {
		mu.Lock()
		defer mu.Unlock()
		_ = rc.SetWriteDeadline(time.Now().Add(15 * time.Second))
		if e := json.NewEncoder(w).Encode(f); e == nil {
			_ = rc.Flush()
		}
	}
	var out api.Frame
	var err error
	switch in.Operation {
	case "bind":
		err = s.Registry.Bind(ctx, &in.Session)
		out.Session = &in.Session
	case "describe":
		out.Tools, err = s.Registry.Describe(ctx, in.Session)
	case "catalog":
		out.Tools = s.Registry.Catalog(in.Tenant)
	case "close":
		s.Registry.CloseSession(in.Tenant, in.Session.ID)
	case "invoke":
		if in.Scope.Tenant != in.Tenant || in.Scope.Session != in.Session.ID {
			err = core.E(core.Invalid, "inconsistent tool scope")
			break
		}
		head, e := s.Storage.Do(ctx, storage.Request{Operation: "session.get", Identity: storage.Identity{Tenant: in.Tenant, Session: in.Session.ID}})
		if e != nil {
			err = e
			break
		}
		if head.Record == nil || head.Record.Deleted || in.Scope.Owner == "" || head.Record.Owner != in.Scope.Owner || head.Record.Epoch != in.Scope.Epoch {
			err = core.E(core.Conflict, "tool dispatch owner was fenced")
			break
		}
		result, e := s.Registry.Invoke(ctx, in.Session, in.Scope, in.Call, func(data json.RawMessage) { write(api.Frame{Progress: data}) })
		out.Result = &result
		err = e
	default:
		err = core.E(core.Invalid, "unknown tools operation")
	}
	if err != nil {
		var e *core.Error
		if errors.As(err, &e) {
			out.Error = e
		} else {
			slog.Error("tools operation failed", "error", err)
			out.Error = &core.Error{Code: core.Internal, Message: "tools operation failed"}
		}
	}
	out.Done = true
	write(out)
}
