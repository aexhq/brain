// A separate local tool service. Brain has no approval state or UI.
package main

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/tools"
)

type approval struct {
	Call     string `json:"call_id"`
	Session  string `json:"session_id"`
	Question string `json:"question"`
	answer   chan bool
}
type service struct {
	key, operator, tenant, brainURL string
	mu                              sync.Mutex
	pending                         map[string]*approval
	client                          *http.Client
}

func bindings(url, tenant string) []tools.Binding {
	return []tools.Binding{
		{Tool: core.Tool{Name: "echo", Description: "Echo text through an external tool service.", InputSchema: json.RawMessage(`{"type":"object","properties":{"text":{"type":"string","maxLength":4096}},"required":["text"],"additionalProperties":false}`)}, URL: url + "/tools/echo", TokenEnv: "BRAIN_TOOLS_KEY", Tenants: []string{tenant}},
		{Tool: core.Tool{Name: "inspect_session", Description: "Read the first page of events from your own Brain session.", InputSchema: json.RawMessage(`{"type":"object","properties":{},"additionalProperties":false}`)}, URL: url + "/tools/inspect", TokenEnv: "BRAIN_TOOLS_KEY", Tenants: []string{tenant}, InspectSession: true},
		{Tool: core.Tool{Name: "ask_approval", Description: "Ask the local operator a yes/no question. Wait silently for their answer. This does not itself authorize another service to act.", InputSchema: json.RawMessage(`{"type":"object","properties":{"question":{"type":"string","minLength":1,"maxLength":4096}},"required":["question"],"additionalProperties":false}`)}, URL: url + "/tools/approval", TokenEnv: "BRAIN_TOOLS_KEY", Tenants: []string{tenant}, TimeoutSeconds: 300},
	}
}

func (s *service) handler() http.Handler {
	mux := http.NewServeMux()
	auth := func(key string, next http.HandlerFunc) http.HandlerFunc {
		return func(w http.ResponseWriter, r *http.Request) {
			if subtle.ConstantTimeCompare([]byte(r.Header.Get("Authorization")), []byte("Bearer "+key)) != 1 {
				http.Error(w, "unauthorized", 401)
				return
			}
			next(w, r)
		}
	}
	mux.HandleFunc("POST /tools/{tool}", auth(s.key, s.call))
	mux.HandleFunc("GET /approvals", auth(s.operator, func(w http.ResponseWriter, r *http.Request) {
		s.mu.Lock()
		pending := make([]*approval, 0, len(s.pending))
		for _, a := range s.pending {
			pending = append(pending, a)
		}
		s.mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"approvals": pending})
	}))
	mux.HandleFunc("POST /approvals/{id}", auth(s.operator, func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Approved *bool `json:"approved"`
		}
		if err := decode(w, r, &input); err != nil || input.Approved == nil {
			http.Error(w, "approved must be true or false", 400)
			return
		}
		s.mu.Lock()
		a := s.pending[r.PathValue("id")]
		if a == nil {
			s.mu.Unlock()
			http.Error(w, "approval not found", 404)
			return
		}
		select {
		case a.answer <- *input.Approved:
			delete(s.pending, a.Call)
			s.mu.Unlock()
			w.WriteHeader(204)
		default:
			s.mu.Unlock()
			http.Error(w, "already answered", 409)
		}
	}))
	return mux
}

func decode(w http.ResponseWriter, r *http.Request, value any) error {
	d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 16<<10))
	d.DisallowUnknownFields()
	if err := d.Decode(value); err != nil {
		return err
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing JSON")
	}
	return nil
}
func result(w http.ResponseWriter, r core.Result) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(r)
}

func (s *service) call(w http.ResponseWriter, r *http.Request) {
	var req tools.Request
	if err := decode(w, r, &req); err != nil {
		http.Error(w, "invalid tool request", 400)
		return
	}
	if req.Tenant != s.tenant || !core.ValidID(req.Session) || !core.ValidID(req.Call) {
		http.Error(w, "invalid scope", 403)
		return
	}
	switch r.PathValue("tool") {
	case "echo":
		var input struct {
			Text string `json:"text"`
		}
		if err := json.Unmarshal(req.Arguments, &input); err != nil || len(input.Text) > 4096 {
			http.Error(w, "invalid text", 400)
			return
		}
		result(w, core.Result{Content: []core.Content{{Type: "text", Text: input.Text}}})
	case "inspect":
		if req.BrainURL != s.brainURL || req.InspectionToken == "" {
			result(w, core.ToolError("session inspection was not delegated", false))
			return
		}
		target := s.brainURL + "/sessions/" + req.Session + "/events?limit=20"
		request, err := http.NewRequestWithContext(r.Context(), "GET", target, nil)
		if err != nil {
			result(w, core.ToolError("invalid Brain address", false))
			return
		}
		request.Header.Set("Authorization", "Bearer "+req.InspectionToken)
		response, err := s.client.Do(request)
		if err != nil {
			result(w, core.ToolError("session inspection failed", false))
			return
		}
		defer response.Body.Close()
		b, err := io.ReadAll(io.LimitReader(response.Body, (256<<10)+1))
		if err != nil || len(b) > 256<<10 || response.StatusCode != 200 {
			result(w, core.ToolError("session inspection failed or exceeded its output limit", false))
			return
		}
		result(w, core.Result{Content: []core.Content{{Type: "text", Text: string(b)}}})
	case "approval":
		var input struct {
			Question string `json:"question"`
		}
		if err := json.Unmarshal(req.Arguments, &input); err != nil || strings.TrimSpace(input.Question) == "" || len(input.Question) > 4096 {
			http.Error(w, "invalid question", 400)
			return
		}
		a := &approval{Call: req.Call, Session: req.Session, Question: input.Question, answer: make(chan bool, 1)}
		s.mu.Lock()
		if len(s.pending) >= 32 || s.pending[req.Call] != nil {
			s.mu.Unlock()
			result(w, core.ToolError("approval queue is full or call already exists", false))
			return
		}
		s.pending[req.Call] = a
		s.mu.Unlock()
		defer func() { s.mu.Lock(); delete(s.pending, req.Call); s.mu.Unlock() }()
		w.Header().Set("Content-Type", "text/event-stream")
		progress, _ := json.Marshal(map[string]string{"call_id": a.Call, "status": "waiting_for_operator"})
		if _, err := fmt.Fprintf(w, "event: progress\ndata: %s\n\n", progress); err != nil {
			return
		}
		if err := http.NewResponseController(w).Flush(); err != nil {
			return
		}
		var outcome core.Result
		timer := time.NewTimer(5 * time.Minute)
		defer timer.Stop()
		select {
		case approved := <-a.answer:
			if approved {
				outcome = core.Result{Content: []core.Content{{Type: "text", Text: "Operator answered yes."}}}
			} else {
				outcome = core.ToolError("Operator answered no.", false)
			}
		case <-timer.C:
			outcome = core.ToolError("Operator did not answer before the deadline.", false)
		case <-r.Context().Done():
			return
		}
		b, _ := json.Marshal(outcome)
		_, _ = fmt.Fprintf(w, "event: result\ndata: %s\n\n", b)
	default:
		http.NotFound(w, r)
	}
}

func main() {
	listen := flag.String("listen", "127.0.0.1:8081", "local tool service address")
	brain := flag.String("brain", "http://127.0.0.1:8080", "trusted Brain API address")
	tenant := flag.String("tenant", "local", "allowed tenant")
	printConfig := flag.Bool("print-config", false, "print a local Brain configuration with these tool bindings")
	flag.Parse()
	if *printConfig {
		cfg := map[string]any{"listen": "127.0.0.1:8080", "public_url": *brain, "data_dir": ".brain", "tokens": []any{map[string]any{"token_env": "BRAIN_API_KEY", "tenant": *tenant, "manage": true}}, "harnesses": map[string]any{"pi": map[string]any{"command": []string{"node", "runners/pi/dist/main.js"}}}, "providers": map[string]any{"openai": map[string]string{"api_key_env": "OPENAI_API_KEY"}}, "tools": bindings("http://"+*listen, *tenant), "persistence": map[string]any{"provider": "filesystem", "directory": ".brain-archive"}}
		enc := json.NewEncoder(os.Stdout)
		enc.SetIndent("", "  ")
		if err := enc.Encode(cfg); err != nil {
			slog.Error("encode configuration", "error", err)
			os.Exit(1)
		}
		return
	}
	key, operator := os.Getenv("BRAIN_TOOLS_KEY"), os.Getenv("BRAIN_APPROVAL_KEY")
	if len(key) < 24 || len(operator) < 24 || key == operator {
		slog.Error("BRAIN_TOOLS_KEY and BRAIN_APPROVAL_KEY need distinct tokens of at least 24 bytes")
		os.Exit(1)
	}
	s := &service{key: key, operator: operator, tenant: *tenant, brainURL: strings.TrimRight(*brain, "/"), pending: map[string]*approval{}, client: &http.Client{Timeout: 10 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return errors.New("redirect disabled") }}}
	server := &http.Server{Addr: *listen, Handler: s.handler(), ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 10 * time.Second, IdleTimeout: 60 * time.Second}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go func() { <-ctx.Done(); _ = server.Close() }()
	slog.Info("local tool service listening", "address", *listen)
	if err := server.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		slog.Error("tool service stopped", "error", err)
		os.Exit(1)
	}
}
