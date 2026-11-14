package api

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/tools/registry"
	"golang.org/x/net/websocket"
	"io"
	"net/http"
	"strings"
	"time"
)

func (s *ExecutorServer) registerExecutor(w http.ResponseWriter, r *http.Request, g core.Grant) {
	if s.Bridge == nil {
		s.fail(w, r, core.E(core.Unsupported, "runtime executors are not configured"))
		return
	}
	if !g.Manage || g.ReadOnly || g.Executor != "" {
		s.fail(w, r, core.E(core.Forbidden, "executor registration requires management"))
		return
	}
	var input tools.ExecutorRegistration
	if err := s.decode(w, r, &input); err != nil {
		s.badInput(w, r, err)
		return
	}
	var saved *core.Session
	if input.Session != "" {
		session, err := s.Sessions.Get(r.Context(), g, input.Session)
		if err != nil {
			s.fail(w, r, err)
			return
		}
		saved = &session
	} else if g.Session != "" {
		s.fail(w, r, core.E(core.Forbidden, "session-scoped credentials can only rebind their own session"))
		return
	}
	registration, err := s.Bridge.Register(g.Tenant, input, saved)
	if err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 201, registration)
}

func (s *ExecutorServer) executorScope(w http.ResponseWriter, r *http.Request, g core.Grant) bool {
	if s.Bridge == nil || g.Executor == "" || g.Executor != r.PathValue("id") {
		s.fail(w, r, core.E(core.Forbidden, "executor credential required"))
		return false
	}
	return true
}
func (s *ExecutorServer) executorReply(w http.ResponseWriter, r *http.Request, g core.Grant) {
	if !s.executorScope(w, r, g) {
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, 2<<20)
	var reply tools.Reply
	d := json.NewDecoder(r.Body)
	d.DisallowUnknownFields()
	if err := d.Decode(&reply); err != nil {
		s.badInput(w, r, err)
		return
	}
	if d.Decode(new(any)) != io.EOF {
		s.badInput(w, r, core.E(core.Invalid, "trailing reply data"))
		return
	}
	if err := s.Bridge.Reply(g.Executor, reply); err != nil {
		s.fail(w, r, err)
		return
	}
	write(w, 200, core.Status{Status: "accepted"})
}
func (s *ExecutorServer) unregisterExecutor(w http.ResponseWriter, r *http.Request, g core.Grant) {
	if !s.executorScope(w, r, g) {
		return
	}
	s.Bridge.Unregister(g.Executor)
	write(w, 200, core.Status{Status: "closed"})
}

func (s *ExecutorServer) executorCommands(w http.ResponseWriter, r *http.Request, g core.Grant) {
	if !s.executorScope(w, r, g) {
		return
	}
	c, err := s.Bridge.Attach(g.Executor)
	if err != nil {
		s.fail(w, r, err)
		return
	}
	defer s.Bridge.Detach(g.Executor, c)
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-store")
	rc := http.NewResponseController(w)
	send := func(command tools.Command) error {
		data, err := json.Marshal(command)
		if err != nil {
			return err
		}
		if err := rc.SetWriteDeadline(time.Now().Add(10 * time.Second)); err != nil {
			return err
		}
		if _, err = fmt.Fprintf(w, "data: %s\n\n", data); err != nil {
			return err
		}
		return rc.Flush()
	}
	if send(tools.Command{Type: "ready", Connection: c.ID}) != nil {
		return
	}
	ticker := time.NewTicker(15 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case command := <-c.Commands:
			if send(command) != nil {
				return
			}
		case <-ticker.C:
			if send(tools.Command{Type: "ping", Connection: c.ID}) != nil {
				return
			}
		case <-c.Done:
			return
		case <-r.Context().Done():
			return
		}
	}
}

func (s *ExecutorServer) executorRPC(w http.ResponseWriter, r *http.Request, g core.Grant) {
	if !s.executorScope(w, r, g) {
		return
	}
	// Browser execution awaits the separately designed origin/authentication policy.
	if r.Header.Get("Origin") != "" {
		s.fail(w, r, core.E(core.Forbidden, "browser executor origins are not configured"))
		return
	}
	server := websocket.Server{Handshake: func(cfg *websocket.Config, r *http.Request) error {
		for _, p := range cfg.Protocol {
			if p == "brain-rpc" {
				cfg.Protocol = []string{"brain-rpc"}
				return nil
			}
		}
		return errors.New("brain-rpc subprotocol required")
	}, Handler: func(ws *websocket.Conn) {
		defer ws.Close()
		ws.MaxPayloadBytes = 2 << 20
		c, err := s.Bridge.Attach(g.Executor)
		if err != nil {
			return
		}
		defer s.Bridge.Detach(g.Executor, c)
		if websocket.JSON.Send(ws, tools.Command{Type: "ready", Connection: c.ID}) != nil {
			return
		}
		readDone := make(chan struct{})
		go func() {
			defer close(readDone)
			for {
				var data []byte
				if websocket.Message.Receive(ws, &data) != nil {
					return
				}
				var reply tools.Reply
				decoder := json.NewDecoder(bytes.NewReader(data))
				decoder.DisallowUnknownFields()
				if decoder.Decode(&reply) != nil || decoder.Decode(new(any)) != io.EOF {
					return
				}
				if s.Bridge.Reply(g.Executor, reply) != nil {
					return
				}
			}
		}()
		ticker := time.NewTicker(15 * time.Second)
		defer ticker.Stop()
		for {
			select {
			case command := <-c.Commands:
				if ws.SetWriteDeadline(time.Now().Add(10*time.Second)) != nil || websocket.JSON.Send(ws, command) != nil {
					return
				}
			case <-ticker.C:
				if ws.SetWriteDeadline(time.Now().Add(10*time.Second)) != nil || websocket.JSON.Send(ws, tools.Command{Type: "ping", Connection: c.ID}) != nil {
					return
				}
			case <-readDone:
				return
			case <-c.Done:
				return
			case <-r.Context().Done():
				return
			}
		}
	}}
	server.ServeHTTP(w, r)
}

func executorToken(r *http.Request) string {
	if strings.HasPrefix(r.Header.Get("Authorization"), "Bearer ") {
		return strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
	}
	if r.Header.Get("Upgrade") == "websocket" {
		for _, p := range strings.Split(r.Header.Get("Sec-WebSocket-Protocol"), ",") {
			p = strings.TrimSpace(p)
			if strings.HasPrefix(p, "brain-auth.") {
				return strings.TrimPrefix(p, "brain-auth.")
			}
		}
	}
	return ""
}

type SessionReader interface {
	Get(context.Context, core.Grant, string) (core.Session, error)
}
type ExecutorServer struct {
	Bridge   *tools.Bridge
	Sessions SessionReader
	Auth     *core.Auth
}
type scoped func(http.ResponseWriter, *http.Request, core.Grant)

func (s *ExecutorServer) Handler() http.Handler {
	m := http.NewServeMux()
	m.HandleFunc("POST /executors", s.authorize(s.registerExecutor))
	m.HandleFunc("GET /executors/{id}/commands", s.authorize(s.executorCommands))
	m.HandleFunc("POST /executors/{id}/responses", s.authorize(s.executorReply))
	m.HandleFunc("GET /executors/{id}/rpc", s.authorize(s.executorRPC))
	m.HandleFunc("POST /executors/{id}/close", s.authorize(s.unregisterExecutor))
	return m
}
func (s *ExecutorServer) authorize(next scoped) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Request-ID", core.ID())
		w.Header().Set("X-Content-Type-Options", "nosniff")
		g, ok := s.Auth.Authenticate(r)
		if !ok && s.Bridge != nil {
			id, tenant, valid := s.Bridge.Authenticate(executorToken(r))
			if valid {
				g = core.Grant{Tenant: tenant, Executor: id}
				ok = true
			}
		}
		if !ok {
			write(w, 401, map[string]any{"error": core.Error{Code: core.Forbidden, Message: "authentication required"}})
			return
		}
		if g.ReadOnly && r.Method != http.MethodGet {
			s.fail(w, r, core.E(core.Forbidden, "credential is read-only"))
			return
		}
		next(w, r, g)
	}
}
func (s *ExecutorServer) decode(w http.ResponseWriter, r *http.Request, v any) error {
	d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 2<<20))
	d.DisallowUnknownFields()
	if e := d.Decode(v); e != nil {
		return e
	}
	if d.Decode(new(any)) != io.EOF {
		return core.E(core.Invalid, "trailing request data")
	}
	return nil
}
func (s *ExecutorServer) badInput(w http.ResponseWriter, r *http.Request, e error) {
	s.fail(w, r, core.E(core.Invalid, "invalid request JSON"))
}
func write(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}
func (s *ExecutorServer) fail(w http.ResponseWriter, r *http.Request, err error) {
	code := core.ErrorCode(err)
	status := 500
	message := "executor operation failed"
	var e *core.Error
	if errors.As(err, &e) {
		message = e.Message
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
	}
	write(w, status, map[string]any{"error": core.Error{Code: code, Message: message}, "request_id": w.Header().Get("X-Request-ID")})
}
