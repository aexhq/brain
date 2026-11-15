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
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/propagation"
)

type Server struct {
	Store    *metadata.Store
	Token    string
	Logger   *slog.Logger
	appended metric.Int64Counter
}

func (s *Server) Handler() (http.Handler, error) {
	if s.Store == nil || len(s.Token) < 32 {
		return nil, errors.New("storage and a service token of at least 32 bytes are required")
	}
	if s.Logger == nil {
		s.Logger = slog.Default()
	}
	var err error
	s.appended, err = otel.Meter("github.com/aexhq/brain/services/storage").Int64Counter("storage.transcript.bytes", metric.WithUnit("By"))
	if err != nil {
		return nil, err
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
	d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 5<<20))
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
	case "transcript.append":
		v, e := s.append(ctx, in)
		out.Record, err = &v, e
	case "state.capture":
		v, e := s.capture(ctx, in)
		out.Record, err = &v, e
	case "state.read":
		v, e := s.owned(ctx, in.Identity)
		if e == nil {
			if v.Snapshot != "" {
				v, e = s.readSnapshot(ctx, in.Identity, v.Snapshot)
			} else {
				v.Native = ""
			}
			if e == nil {
				state, se := s.native(ctx, in.Identity, v)
				out.Reference, e = state.Content, se
			}
		}
		err = e
	case "session.save":
		v, e := s.save(ctx, in)
		out.Record, err = &v, e
	case "snapshot.publish":
		v, e := s.owned(ctx, in.Identity)
		// Capture may have completed while the coordinator failed before saving
		// its checkpoint. Publish only the last coherent native boundary.
		if e == nil {
			if v.Snapshot == "" {
				v.Native = ""
			} else {
				var saved api.Record
				saved, e = s.readSnapshot(ctx, in.Identity, v.Snapshot)
				v.Native = saved.Native
			}
		}
		if e == nil {
			v.Snapshot, e = s.snapshot(ctx, in.Identity, v)
		}
		if e == nil {
			v, e = s.Store.Put(ctx, in.Identity, v, v.Revision)
		}
		if e == nil {
			v, e = s.Store.Flush(ctx, in.Identity)
		}
		out.Record, err = &v, e
	case "snapshot.read", "snapshot.restore":
		current, e := s.owned(ctx, in.Identity)
		var v api.Record
		if e == nil {
			v, e = s.readSnapshot(ctx, in.Identity, in.Reference)
		}
		if e == nil {
			v.Credentials, v.Session.Credentials = current.Credentials, current.Credentials
			v.Session.Tenant = in.Identity.Tenant
			if in.Operation == "snapshot.restore" {
				if in.Record == nil || in.Record.Session.ID != v.Session.ID {
					e = core.E(core.Invalid, "restored session is required")
				} else {
					v.Session = in.Record.Session
					v.Session.Credentials = current.Credentials
					v, e = s.Store.Put(ctx, in.Identity, v, current.Revision)
				}
			}
		}
		out.Record, err = &v, e
	case "transcript.read":
		var head api.Record
		head, err = s.Store.Get(ctx, in.Identity)
		if err == nil && head.Deleted {
			err = core.E(core.NotFound, "session was deleted")
		}
		if err == nil && in.Until != nil {
			head, err = s.boundary(ctx, in.Identity, head, *in.Until)
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
