package server

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"time"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/sandbox/api"
	"github.com/aexhq/brain/services/sandbox/internal/environments"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/propagation"
	"golang.org/x/net/websocket"
)

type Server struct {
	Manager         *environments.Manager
	Token           string
	MaxArchiveBytes int64
}

func (s *Server) Handler() (http.Handler, error) {
	if s.Manager == nil || len(s.Token) < 32 || s.MaxArchiveBytes < 1 {
		return nil, errors.New("sandbox manager, service token and archive limit are required")
	}
	m := http.NewServeMux()
	m.HandleFunc("GET /health/live", func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(200) })
	m.HandleFunc("POST /environments", s.auth(s.create))
	m.HandleFunc("DELETE /environments/{id}", s.auth(s.remove))
	m.HandleFunc("PUT /environments/{id}/files", s.auth(s.importFiles))
	m.HandleFunc("GET /environments/{id}/files", s.auth(s.exportFiles))
	m.HandleFunc("GET /environments/{id}/process", s.auth(s.process))
	return m, nil
}
func (s *Server) auth(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if subtle.ConstantTimeCompare([]byte(r.Header.Get("Authorization")), []byte("Bearer "+s.Token)) != 1 {
			fail(w, 401, "service authentication required")
			return
		}
		ctx := otel.GetTextMapPropagator().Extract(r.Context(), propagation.HeaderCarrier(r.Header))
		ctx, span := otel.Tracer("github.com/aexhq/brain/sandbox").Start(ctx, "sandbox.request")
		defer span.End()
		next(w, r.WithContext(ctx))
	}
}
func fail(w http.ResponseWriter, status int, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(api.Failure{Error: message})
}
func failure(w http.ResponseWriter, err error) {
	var e *core.Error
	if errors.As(err, &e) {
		status := 400
		switch e.Code {
		case core.NotFound:
			status = 404
		case core.Conflict:
			status = 409
		case core.Overloaded:
			status = 503
		}
		fail(w, status, e.Message)
		return
	}
	slog.Error("sandbox operation failed", "error", err)
	fail(w, 500, "sandbox operation failed")
}
func (s *Server) create(w http.ResponseWriter, r *http.Request) {
	var in api.Create
	d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 8192))
	d.DisallowUnknownFields()
	if e := d.Decode(&in); e != nil || d.Decode(new(any)) != io.EOF {
		fail(w, 400, "invalid environment request")
		return
	}
	v, e := s.Manager.Create(r.Context(), in)
	if e != nil {
		failure(w, e)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(v)
}
func (s *Server) remove(w http.ResponseWriter, r *http.Request) {
	if e := s.Manager.Remove(r.Context(), r.PathValue("id")); e != nil {
		failure(w, e)
		return
	}
	w.WriteHeader(204)
}
func (s *Server) importFiles(w http.ResponseWriter, r *http.Request) {
	if e := s.Manager.Import(r.Context(), r.PathValue("id"), http.MaxBytesReader(w, r.Body, s.MaxArchiveBytes), s.MaxArchiveBytes); e != nil {
		failure(w, e)
		return
	}
	w.WriteHeader(204)
}
func (s *Server) exportFiles(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/x-tar")
	if e := s.Manager.Export(r.Context(), r.PathValue("id"), w, s.MaxArchiveBytes); e != nil {
		slog.Error("sandbox export failed", "error", e)
		panic(http.ErrAbortHandler)
	}
}

type processIO struct {
	io.ReadWriter
	ready  chan struct{}
	cancel context.CancelFunc
}

func (p *processIO) Read(b []byte) (int, error) {
	n, e := p.ReadWriter.Read(b)
	if e != nil {
		p.cancel()
	}
	return n, e
}
func (p *processIO) Write(b []byte) (int, error) {
	<-p.ready
	n, e := p.ReadWriter.Write(b)
	if e != nil {
		p.cancel()
	}
	return n, e
}
func (s *Server) process(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	wsServer := websocket.Server{Handshake: func(c *websocket.Config, r *http.Request) error { return nil }, Handler: func(ws *websocket.Conn) {
		defer ws.Close()
		ws.MaxPayloadBytes = 4 << 20
		ctx, cancel := context.WithCancel(r.Context())
		defer cancel()
		defer func() {
			ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
			defer cancel()
			if e := s.Manager.Remove(ctx, id); e != nil {
				slog.Error("sandbox cleanup failed", "error", e)
			}
		}()
		var input api.Process
		_ = ws.SetReadDeadline(time.Now().Add(10 * time.Second))
		if e := websocket.JSON.Receive(ws, &input); e != nil {
			return
		}
		_ = ws.SetReadDeadline(time.Time{})
		stream := &processIO{ReadWriter: ws, ready: make(chan struct{}), cancel: cancel}
		_ = s.Manager.Process(ctx, id, input.Command, stream, stream, func(e error) {
			v := api.Failure{}
			if e != nil {
				v.Error = "process could not start"
			}
			if e = websocket.JSON.Send(ws, v); e != nil {
				cancel()
			}
			close(stream.ready)
		})
	}}
	wsServer.ServeHTTP(w, r)
}
