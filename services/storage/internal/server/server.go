package server

import (
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"strings"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/storage/internal/adapters"
	"github.com/aexhq/brain/services/storage/internal/sessions/metadata"
	"github.com/aexhq/brain/services/storage/internal/transcripts"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
)

type Server struct {
	Store  *metadata.Store
	Token  string
	Logger *slog.Logger
}

func (s *Server) Handler() (http.Handler, error) {
	if s.Store == nil || len(s.Token) < 32 {
		return nil, errors.New("storage and a service token of at least 32 bytes are required")
	}
	if s.Logger == nil {
		s.Logger = slog.Default()
	}
	m := http.NewServeMux()
	m.HandleFunc("GET /health/live", func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(200) })
	m.HandleFunc("POST /rpc", s.rpc)
	return m, nil
}
func (s *Server) rpc(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	if subtle.ConstantTimeCompare([]byte(strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")), []byte(s.Token)) != 1 {
		w.WriteHeader(401)
		_ = json.NewEncoder(w).Encode(api.Response{Error: &core.Error{Code: core.Forbidden, Message: "service authentication required"}})
		return
	}
	ctx := otel.GetTextMapPropagator().Extract(r.Context(), propagation.HeaderCarrier(r.Header))
	ctx, span := otel.Tracer("github.com/aexhq/brain/storage").Start(ctx, "storage.request")
	defer span.End()
	var in api.Request
	d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 2<<20))
	d.DisallowUnknownFields()
	if err := d.Decode(&in); err != nil || d.Decode(new(any)) != io.EOF {
		s.fail(w, core.E(core.Invalid, "invalid storage request"))
		return
	}
	var out api.Response
	var err error
	if in.Identity.Collection != "" && in.Operation != "object.put" && in.Operation != "object.get" && in.Operation != "object.commit" {
		s.fail(w, core.E(core.Invalid, "package collection only accepts object operations"))
		return
	}
	switch in.Operation {
	case "transcript.read":
		var head api.Record
		head, err = s.Store.Get(ctx, in.Identity)
		if err == nil && head.Deleted {
			err = core.E(core.NotFound, "session was deleted")
		}
		if err == nil {
			out.Events, out.Next, err = (transcripts.Reader{Get: s.Store.GetObject}).Read(ctx, in.Identity, head, in.After, in.Limit, in.Tail)
		}
	case "session.sync":
		v, e := s.Store.Status(ctx, in.Identity)
		out.Status, err = &v, e
	case "session.get":
		v, e := s.Store.Get(ctx, in.Identity)
		out.Record = &v
		err = e
	case "session.put":
		if in.Record == nil {
			err = core.E(core.Invalid, "record is required")
			break
		}
		v, e := s.Store.Put(ctx, in.Identity, *in.Record, in.ExpectedRevision)
		out.Record = &v
		err = e
	case "session.claim":
		v, e := s.Store.Claim(ctx, in.Identity)
		out.Record = &v
		err = e
	case "session.flush":
		v, e := s.Store.Flush(ctx, in.Identity)
		out.Record = &v
		err = e
	case "session.delete":
		err = s.Store.Erase(ctx, in.Identity)
	case "session.list":
		out.Records, out.Next, err = s.Store.List(ctx, in.Identity.Tenant, in.After, in.Limit)
	case "object.put":
		if in.Object == nil {
			err = core.E(core.Invalid, "object is required")
			break
		}
		out.Reference, err = s.Store.PutObject(ctx, in.Identity, *in.Object)
	case "object.get":
		v, e := s.Store.GetObject(ctx, in.Identity, in.Reference)
		out.Object = &v
		err = e
	case "object.commit":
		err = s.Store.CommitObject(ctx, in.Identity, in.Reference)
	default:
		err = core.E(core.Invalid, "unknown storage operation")
	}
	if err != nil {
		s.fail(w, err)
		return
	}
	_ = json.NewEncoder(w).Encode(out)
}
func (s *Server) fail(w http.ResponseWriter, err error) {
	var e *core.Error
	if !errors.As(err, &e) {
		switch {
		case errors.Is(err, adapters.ErrNotFound):
			e = &core.Error{Code: core.NotFound, Message: "storage record not found"}
		case errors.Is(err, adapters.ErrConflict):
			e = &core.Error{Code: core.Conflict, Message: "storage revision changed"}
		case errors.Is(err, adapters.ErrInvalid):
			e = &core.Error{Code: core.Invalid, Message: "invalid storage request"}
		default:
			s.Logger.Error("storage operation failed", "error", err)
			e = &core.Error{Code: core.Internal, Message: "storage operation failed"}
		}
	}
	status := 500
	switch e.Code {
	case core.NotFound:
		status = 404
	case core.Invalid:
		status = 400
	case core.Forbidden:
		status = 403
	case core.Conflict:
		status = 409
	case core.Unsupported:
		status = 422
	case core.Overloaded:
		status = 503
	}
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(api.Response{Error: e})
}
