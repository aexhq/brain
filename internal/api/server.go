// Package api exposes authenticated sessions and event streams over HTTP.
package api

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/config"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/session"
	"github.com/aexhq/brain/internal/telemetry"
	"github.com/aexhq/brain/internal/tools"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/propagation"
)

// Catalog is the only tool dependency of the HTTP API.
type Catalog interface{ Catalog(string) []core.Tool }

type Server struct {
	Bridge  *tools.Bridge
	Engine  *session.Engine
	Tools   Catalog
	Auth    *Auth
	Limits  config.Limits
	Metrics *telemetry.Metrics
	Logger  *slog.Logger
	subs    chan struct{}
}

func (s *Server) Handler() http.Handler {
	s.subs = make(chan struct{}, s.Limits.Subscribers)
	if s.Logger == nil {
		s.Logger = slog.Default()
	}
	m := http.NewServeMux()
	m.HandleFunc("GET /health/live", func(w http.ResponseWriter, r *http.Request) { write(w, 200, map[string]string{"status": "ok"}) })
	m.HandleFunc("GET /health/ready", func(w http.ResponseWriter, r *http.Request) {
		if !s.Engine.Ready() {
			write(w, 503, map[string]string{"status": "not_ready"})
			return
		}
		write(w, 200, map[string]string{"status": "ready"})
	})
	for _, route := range s.routes() {
		m.HandleFunc(route.Method+" "+route.Path, s.authorize(route.handler))
	}
	m.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) { s.fail(w, r, core.E(core.NotFound, "route not found")) })
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		id := core.ID()
		w.Header().Set("X-Request-ID", id)
		w.Header().Set("X-Content-Type-Options", "nosniff")
		ctx := otel.GetTextMapPropagator().Extract(r.Context(), propagation.HeaderCarrier(r.Header))
		r = r.WithContext(ctx)
		start := time.Now()
		m.ServeHTTP(w, r)
		if s.Metrics != nil {
			method := r.Method
			switch method {
			case "GET", "POST":
			default:
				method = "other"
			}
			attrs := metric.WithAttributes(attribute.String("route", r.Pattern), attribute.String("method", method))
			s.Metrics.Requests.Add(ctx, 1, attrs)
			if !strings.Contains(r.Header.Get("Accept"), "text/event-stream") {
				s.Metrics.Duration.Record(ctx, time.Since(start).Seconds(), attrs)
			}
		}
	})
}

type scoped func(http.ResponseWriter, *http.Request, core.Grant)

func (s *Server) authorize(next scoped) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		g, ok := s.Auth.Authenticate(r)
		if !ok && s.Bridge != nil {
			id, tenant, valid := s.Bridge.Authenticate(executorToken(r))
			if valid {
				g = core.Grant{Tenant: tenant, Executor: id}
				ok = true
			}
		}
		if g.Executor != "" && (!strings.HasPrefix(r.URL.Path, "/executors/") || r.PathValue("id") != g.Executor) {
			s.fail(w, r, core.E(core.Forbidden, "executor credential cannot access sessions"))
			return
		}
		if !ok {
			w.Header().Set("WWW-Authenticate", "Bearer")
			write(w, 401, ErrorResponse{Error: core.Error{Code: core.Forbidden, Message: "authentication required"}, RequestID: w.Header().Get("X-Request-ID")})
			return
		}
		if g.ReadOnly && r.Method != http.MethodGet {
			s.fail(w, r, core.E(core.Forbidden, "credential is read-only"))
			return
		}
		next(w, r, g)
	}
}

func (s *Server) decode(w http.ResponseWriter, r *http.Request, value any) error {
	r.Body = http.MaxBytesReader(w, r.Body, int64(s.Limits.InputBytes)+16<<10)
	d := json.NewDecoder(r.Body)
	d.DisallowUnknownFields()
	if err := d.Decode(value); err != nil {
		return err
	}
	if err := d.Decode(new(any)); err != io.EOF {
		return core.E(core.Invalid, "request must contain one JSON value")
	}
	return nil
}

func (s *Server) badInput(w http.ResponseWriter, r *http.Request, err error) {
	var limit *http.MaxBytesError
	if errors.As(err, &limit) {
		write(w, 413, ErrorResponse{Error: core.Error{Code: core.Invalid, Message: "request body limit exceeded"}, RequestID: w.Header().Get("X-Request-ID")})
		return
	}
	s.fail(w, r, core.E(core.Invalid, "invalid request JSON: "+err.Error()))
}

func public(session core.Session) core.Session {
	if session.Checkpoint != nil {
		cp := *session.Checkpoint
		cp.Files = nil
		cp.Config = core.Config{}
		session.Checkpoint = &cp
	}
	return session
}
func (s *Server) create(w http.ResponseWriter, r *http.Request, g core.Grant) {
	var input core.CreateSession
	if err := s.decode(w, r, &input); err != nil {
		s.badInput(w, r, err)
		return
	}
	created, err := s.Engine.Create(r.Context(), g, input)
	if err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 201, public(created))
}
func (s *Server) get(w http.ResponseWriter, r *http.Request, g core.Grant) {
	got, err := s.Engine.Get(r.Context(), g, r.PathValue("id"))
	if err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 200, public(got))
}
func limit(r *http.Request) (int, error) {
	v := r.URL.Query().Get("limit")
	if v == "" {
		return 100, nil
	}
	n, err := strconv.Atoi(v)
	if err != nil || n < 1 || n > 1000 {
		return 0, core.E(core.Invalid, "limit must be between 1 and 1000")
	}
	return n, nil
}
func (s *Server) list(w http.ResponseWriter, r *http.Request, g core.Grant) {
	n, err := limit(r)
	if err != nil {
		s.fail(w, r, err)
		return
	}
	sessions, err := s.Engine.List(r.Context(), g, r.URL.Query().Get("after"), n)
	if err != nil {
		s.fail(w, r, err)
		return
	}
	for i := range sessions {
		sessions[i] = public(sessions[i])
	}
	write(w, 200, SessionList{Sessions: sessions})
}
func (s *Server) send(w http.ResponseWriter, r *http.Request, g core.Grant) {
	var input core.Input
	if err := s.decode(w, r, &input); err != nil {
		s.badInput(w, r, err)
		return
	}
	if input.Type == "user.interrupt" {
		if input.Text != "" || input.Key != "" || input.Delivery != "" {
			s.fail(w, r, core.E(core.Invalid, "interrupt accepts no message fields"))
			return
		}
		if err := s.Engine.Interrupt(r.Context(), g, r.PathValue("id")); err != nil {
			s.fail(w, r, err)
			return
		}
		write(w, 202, core.Status{Status: "interrupt_requested"})
		return
	}
	event, err := s.Engine.Submit(r.Context(), g, r.PathValue("id"), input)
	if err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 202, event)
}
func (s *Server) resume(w http.ResponseWriter, r *http.Request, g core.Grant) {
	got, err := s.Engine.Resume(r.Context(), g, r.PathValue("id"))
	if err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 200, public(got))
}
func (s *Server) close(w http.ResponseWriter, r *http.Request, g core.Grant) {
	if err := s.Engine.CloseSession(r.Context(), g, r.PathValue("id")); err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 202, core.Status{Status: "close_requested"})
}

func (s *Server) flush(w http.ResponseWriter, r *http.Request, g core.Grant) {
	status, err := s.Engine.Flush(r.Context(), g, r.PathValue("id"))
	if err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 200, status)
}
func (s *Server) syncStatus(w http.ResponseWriter, r *http.Request, g core.Grant) {
	status, err := s.Engine.SyncStatus(r.Context(), g, r.PathValue("id"))
	if err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 200, status)
}
func (s *Server) restore(w http.ResponseWriter, r *http.Request, g core.Grant) {
	var input core.Restore
	if err := s.decode(w, r, &input); err != nil {
		s.badInput(w, r, err)
		return
	}
	got, err := s.Engine.Restore(r.Context(), g, r.PathValue("id"), input.Checkpoint)
	if err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 200, public(got))
}

func (s *Server) events(w http.ResponseWriter, r *http.Request, g core.Grant) {
	n, err := limit(r)
	if err != nil {
		s.fail(w, r, err)
		return
	}
	after := r.URL.Query().Get("after")
	if after == "" {
		after = r.Header.Get("Last-Event-ID")
	}
	if len(after) > 512 {
		s.fail(w, r, core.E(core.CursorUnavailable, "invalid event cursor"))
		return
	}
	log, release, err := s.Engine.Log(r.Context(), g, r.PathValue("id"))
	if err != nil {
		s.fail(w, r, err)
		return
	}
	defer release()
	if !strings.Contains(r.Header.Get("Accept"), "text/event-stream") {
		events, err := log.Read(after, n, -1)
		if err != nil {
			s.fail(w, r, err)
			return
		}
		next := after
		if len(events) > 0 {
			next = events[len(events)-1].Cursor
		}
		write(w, 200, EventList{Events: events, NextCursor: next})
		return
	}
	select {
	case s.subs <- struct{}{}:
		defer func() { <-s.subs }()
	default:
		s.fail(w, r, core.E(core.Overloaded, "subscriber limit reached"))
		return
	}
	sub, boundary := log.Subscribe(s.Limits.SubscriberEvents, s.Limits.SubscriberBytes)
	defer sub.Close()
	if _, err := log.Read(after, 1, boundary); err != nil {
		s.fail(w, r, err)
		return
	}
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("X-Accel-Buffering", "no")
	controller := http.NewResponseController(w)
	flush := func() error { return controller.Flush() }
	output := func(event core.Event) error {
		if err := controller.SetWriteDeadline(time.Now().Add(15 * time.Second)); err != nil && !errors.Is(err, http.ErrNotSupported) {
			return err
		}
		data, err := json.Marshal(event)
		if err != nil {
			return err
		}
		if event.Cursor != "" {
			if _, err := fmt.Fprintf(w, "id: %s\n", event.Cursor); err != nil {
				return err
			}
		}
		if _, err := fmt.Fprintf(w, "event: %s\ndata: %s\n\n", event.Type, data); err != nil {
			return err
		}
		return flush()
	}
	if err := flush(); err != nil {
		return
	}
	for {
		select {
		case <-sub.Done:
			return
		case <-r.Context().Done():
			return
		default:
		}
		batch, err := log.Read(after, 100, boundary)
		if err != nil {
			return
		}
		if len(batch) == 0 {
			break
		}
		for _, event := range batch {
			if err := output(event); err != nil {
				return
			}
			after = event.Cursor
		}
	}
	ticker := time.NewTicker(15 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case event := <-sub.Events:
			sub.Consumed(event)
			if err := output(event); err != nil {
				return
			}
		case <-sub.Done:
			return
		case <-r.Context().Done():
			return
		case <-ticker.C:
			if err := controller.SetWriteDeadline(time.Now().Add(15 * time.Second)); err != nil && !errors.Is(err, http.ErrNotSupported) {
				return
			}
			if _, err := io.WriteString(w, ": keepalive\n\n"); err != nil {
				return
			}
			if err := flush(); err != nil {
				return
			}
		}
	}
}

func write(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}
func (s *Server) fail(w http.ResponseWriter, r *http.Request, err error) {
	code := core.ErrorCode(err)
	if s.Metrics != nil {
		s.Metrics.Rejected.Add(r.Context(), 1, metric.WithAttributes(attribute.String("code", string(code))))
	}
	status := 500
	message := "internal server error"
	var typed *core.Error
	if errors.As(err, &typed) {
		message = typed.Message
	}
	switch code {
	case core.Invalid:
		status = 400
	case core.Forbidden:
		status = 403
	case core.NotFound:
		status = 404
	case core.Conflict:
		status = 409
	case core.Unsupported:
		status = 422
	case core.Overloaded:
		status = 429
		w.Header().Set("Retry-After", "1")
	case core.CursorUnavailable:
		status = 409
	}
	if status == 500 {
		s.Logger.Error("request failed", "request_id", w.Header().Get("X-Request-ID"), "error", err)
	}
	write(w, status, ErrorResponse{Error: core.Error{Code: code, Message: message}, RequestID: w.Header().Get("X-Request-ID")})
}
