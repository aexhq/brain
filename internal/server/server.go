package server

import (
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/execution"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/history"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aexhq/brain/internal/validation"
)

type Config struct {
	Node            string
	Token           string
	Key             []byte
	Workers         int
	Inflight        int
	Requests        int
	FileTransfers   int
	Streams         int
	Lease           time.Duration
	Development     bool
	Origins         []string
	InternalURL     string
	ArchiveInterval time.Duration
	Identity        execution.Identity
}

type Server struct {
	Config        Config
	Control       control.Store
	History       *history.Store
	Blobs         history.Archive
	Harness       execution.Harness
	Effects       execution.Effects
	Validator     *validation.Validator
	mux           *http.ServeMux
	draining      atomic.Bool
	healthy       atomic.Bool
	wg            sync.WaitGroup
	busy          sync.Map
	cancel        context.CancelFunc
	activeMu      sync.Mutex
	active        map[string]map[string]context.CancelFunc
	effectSlots   chan struct{}
	requestSlots  chan struct{}
	fileSlots     chan struct{}
	streamSlots   chan struct{}
	receiptSlots  chan struct{}
	routeMu       sync.Mutex
	routeFailures map[string]time.Time
}

func New(config Config, authority control.Store, records *history.Store, blobs history.Archive, harness execution.Harness, effects execution.Effects) (*Server, error) {
	if config.Inflight == 0 {
		config.Inflight = config.Workers * 4
	}
	if config.ArchiveInterval == 0 {
		config.ArchiveInterval = time.Second
	}
	if config.Requests == 0 {
		config.Requests = 128
	}
	if config.FileTransfers == 0 {
		config.FileTransfers = 4
	}
	if config.Streams == 0 {
		config.Streams = 1024
	}
	if config.Requests < 1 || config.Requests > 8192 || config.FileTransfers < 1 || config.FileTransfers > 128 ||
		config.Streams < 1 || config.Streams > 8192 {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid HTTP request, file-transfer or stream bound")
	}
	if config.ArchiveInterval < 100*time.Millisecond || config.ArchiveInterval > time.Minute {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "archive maintenance interval must be between 100 ms and one minute")
	}
	if config.Workers < 1 || config.Workers > 8192 || config.Lease < 3*time.Second || len(config.Key) < 32 {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid worker, lease or custody configuration")
	}
	v, err := validation.New()
	if err != nil {
		return nil, err
	}
	if config.Inflight < 1 || config.Inflight > 8192 {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid in-flight effect bound")
	}
	s := &Server{Config: config, Control: authority, History: records, Blobs: blobs, Harness: harness, Effects: effects, Validator: v, active: map[string]map[string]context.CancelFunc{}, effectSlots: make(chan struct{}, config.Inflight), routeFailures: map[string]time.Time{}}
	s.requestSlots = make(chan struct{}, config.Requests)
	s.fileSlots = make(chan struct{}, config.FileTransfers)
	s.streamSlots = make(chan struct{}, config.Streams)
	s.receiptSlots = make(chan struct{}, 32)
	if config.InternalURL != "" {
		if err = s.validateInternalURL(config.InternalURL); err != nil {
			return nil, err
		}
	}
	records.SetRemoteReader(s.remoteRecord)
	if err = s.routes(); err != nil {
		return nil, err
	}
	return s, nil
}

type principalKey struct{}
type handler func(http.ResponseWriter, *http.Request, kernel.Principal) error

func (s *Server) principal(r *http.Request) (kernel.Principal, error) {
	values := r.Header.Values("Authorization")
	fields := strings.Fields(r.Header.Get("Authorization"))
	if len(values) > 1 || (len(fields) != 0 && (len(fields) != 2 || !strings.EqualFold(fields[0], "Bearer") || len(fields[1]) > 16384)) {
		return kernel.Principal{}, kernel.Fail(wire.ErrorCodeUnauthorized, "one valid bearer authority is required")
	}
	token := ""
	if len(fields) == 2 {
		token = fields[1]
	}
	if s.Config.Token != "" && subtle.ConstantTimeCompare([]byte(token), []byte(s.Config.Token)) == 1 {
		tenant := r.Header.Get("X-Brain-Tenant")
		if tenant == "" {
			tenant = "local"
		}
		if !kernel.Identifier(tenant) {
			return kernel.Principal{}, kernel.Fail(wire.ErrorCodeInvalid, "invalid tenant identifier")
		}
		return kernel.Principal{Tenant: tenant, Subject: "operator", Operator: true}, nil
	}
	if s.Config.Token == "" && s.Config.Development && token == "" {
		return kernel.Principal{Tenant: "local", Subject: "local", Operator: true}, nil
	}
	if strings.HasPrefix(token, "p_") {
		id, signature, ok := strings.Cut(strings.TrimPrefix(token, "p_"), ".")
		if ok && s.verify("participant", id, signature) {
			grant, err := s.Control.Participant(r.Context(), id)
			if err == nil && !grant.Revoked {
				return kernel.Principal{Tenant: grant.Tenant, Subject: grant.Subject, Session: grant.Session, GrantID: grant.ID, Permissions: grant.Permissions, Expires: grant.Expires}, nil
			}
		}
	}
	if s.Config.Identity != nil && token != "" {
		return s.Config.Identity.Authenticate(r.Context(), token)
	}
	return kernel.Principal{}, kernel.Fail(wire.ErrorCodeUnauthorized, "valid bearer authority is required")
}

func (s *Server) sign(scope, id string) string {
	mac := hmac.New(sha256.New, s.Config.Key)
	_, _ = mac.Write([]byte(scope + "\x00" + id))
	return base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
}
func (s *Server) verify(scope, id, signature string) bool {
	return hmac.Equal([]byte(s.sign(scope, id)), []byte(signature))
}
func (s *Server) receiptToken(operation control.Operation) string {
	id := operation.View.SessionID + ":" + operation.View.ID + ":" + operation.View.Recipient
	return "r_" + operation.View.ID + "." + s.sign("receipt", id)
}

func (s *Server) wrap(permission string, fn handler) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("Cache-Control", "no-store")
		if origin := r.Header.Get("Origin"); origin != "" {
			admitted := false
			for _, allowed := range s.Config.Origins {
				if allowed == origin {
					admitted = true
				}
			}
			if !admitted {
				writeError(w, kernel.Fail(wire.ErrorCodeForbidden, "browser origin is not admitted"))
				return
			}
			w.Header().Set("Access-Control-Allow-Origin", origin)
			w.Header().Set("Vary", "Origin")
		}
		p, err := s.principal(r)
		if err == nil && permission != "" && !p.Allows(r.PathValue("session"), permission) {
			err = kernel.Fail(wire.ErrorCodeForbidden, "participant grant does not allow this operation")
		}
		if err == nil && strings.HasPrefix(r.URL.Path, "/operator/") && !p.Operator {
			err = kernel.Fail(wire.ErrorCodeForbidden, "operator authority is required")
		}
		if err == nil && r.PathValue("session") != "" {
			_, err = s.Control.Session(r.Context(), p.Tenant, r.PathValue("session"))
		}
		if err == nil {
			ctx := context.WithValue(r.Context(), principalKey{}, p)
			if p.GrantID != "" || p.Expires > 0 {
				ctx = control.WithAuthority(ctx, control.Authority{Participant: p.GrantID, Session: r.PathValue("session"), Tenant: p.Tenant,
					Subject: p.Subject, Permissions: p.Permissions, Expires: p.Expires})
			}
			err = fn(w, r.WithContext(ctx), p)
		}
		if err != nil {
			writeError(w, err)
		}
		slog.Debug("request", "route", r.Pattern, "duration_ms", time.Since(start).Milliseconds(), "code", kernel.Response(err).Code)
	}
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.Method == http.MethodOptions && r.Header.Get("Origin") != "" {
		for _, origin := range s.Config.Origins {
			if origin == r.Header.Get("Origin") {
				w.Header().Set("Access-Control-Allow-Origin", origin)
				w.Header().Set("Access-Control-Allow-Methods", "GET,POST,PUT,DELETE")
				w.Header().Set("Access-Control-Allow-Headers", "Authorization,Content-Type,Last-Event-ID,Idempotency-Key")
				w.Header().Set("Vary", "Origin")
				w.WriteHeader(http.StatusNoContent)
				return
			}
		}
		http.Error(w, "origin denied", http.StatusForbidden)
		return
	}
	release, ok := s.admitHTTP(w, r)
	if !ok {
		return
	}
	defer release()
	s.mux.ServeHTTP(w, r)
}

func writeJSON(w http.ResponseWriter, value any) error {
	w.Header().Set("Content-Type", "application/json")
	return json.NewEncoder(w).Encode(value)
}

func writeError(w http.ResponseWriter, err error) {
	response := kernel.Response(err)
	status := map[wire.ErrorCode]int{
		wire.ErrorCodeInvalid: 400, wire.ErrorCodeUnauthorized: 401, wire.ErrorCodeForbidden: 403,
		wire.ErrorCodeNotFound: 404, wire.ErrorCodeConflict: 409, wire.ErrorCodeOverloaded: 429,
		wire.ErrorCodeUnsupported: 422, wire.ErrorCodeUnavailable: 503, wire.ErrorCodeUncertain: 503,
		wire.ErrorCodeCorrupt: 500, wire.ErrorCodeInternal: 500,
	}[response.Code]
	if status == 0 {
		status = 500
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(response)
}

func readBody[T any](s *Server, w http.ResponseWriter, r *http.Request, name string) (T, error) {
	var out T
	body, err := boundedBody(w, r, 1048576)
	if err != nil {
		return out, err
	}
	if err = s.Validator.Wire(name, body); err != nil {
		return out, err
	}
	err = json.Unmarshal(body, &out)
	if err == nil {
		var envelope struct {
			RequestID *string `json:"request_id"`
		}
		if json.Unmarshal(body, &envelope) == nil && envelope.RequestID != nil && !kernel.Identifier(*envelope.RequestID) {
			return out, kernel.Fail(wire.ErrorCodeInvalid, "request_id must be a bounded identifier")
		}
	}
	return out, err
}

func queryLimit(r *http.Request) (int, error) {
	if len(r.URL.Query().Get("after")) > 128 {
		return 0, kernel.Fail(wire.ErrorCodeInvalid, "collection cursor exceeds its bound")
	}
	if r.URL.Query().Get("limit") == "" {
		return 64, nil
	}
	limit, err := strconv.Atoi(r.URL.Query().Get("limit"))
	if err != nil || limit < 1 || limit > 128 {
		return 0, kernel.Fail(wire.ErrorCodeInvalid, "limit must be between 1 and 128")
	}
	return limit, nil
}

func (s *Server) append(ctx context.Context, session control.Session, kind string, data any, run, operation string, reserved bool) (wire.Record, error) {
	// Probe with maximum-width coordinates before allocating a real position.
	record := wire.Record{SessionID: session.View.ID, Sequence: "9223372036854775807", TimeMs: 9007199254740991, Kind: kind, Data: kernel.JSON(data)}
	if run != "" {
		record.RunID = &run
	}
	if operation != "" {
		record.OperationID = &operation
	}
	reserve := ""
	if reserved {
		reserve = operation
	}
	if err := s.History.CheckAppend(ctx, record, reserve); err != nil {
		return wire.Record{}, err
	}
	sequence, err := s.Control.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		return wire.Record{}, err
	}
	now, err := s.Control.Clock(ctx)
	if err != nil {
		return wire.Record{}, err
	}
	record.Sequence, record.TimeMs = strconv.FormatInt(sequence, 10), uint64(now)
	digest, err := s.History.Append(ctx, record, reserve, session.Owner)
	if err != nil {
		code := kernel.Response(err).Code
		if code == wire.ErrorCodeInvalid || code == wire.ErrorCodeOverloaded {
			_ = s.Control.SealUnused(ctx, session.View.ID, session.Owner, sequence)
		}
		return record, err
	}
	if err = s.Control.Written(ctx, session.View.ID, session.Owner, sequence, digest, int64(len(kernel.JSON(record)))); err != nil {
		return record, err
	}
	return record, s.History.Confirm(ctx, record)
}

func (s *Server) owned(ctx context.Context, tenant, id string) (control.Session, error) {
	session, err := s.Control.Session(ctx, tenant, id)
	if err != nil {
		return session, err
	}
	if err = s.Harness.Validate(session.View.Config); err != nil {
		return session, err
	}
	if err = s.Effects.Validate(ctx, session.Tenant, session.View.Config); err != nil {
		return session, err
	}
	session.Owner, err = s.Control.ClaimOwner(ctx, id, s.Config.Node)
	return session, err
}

func (s *Server) archiveCapability() string {
	if archive, ok := s.Blobs.(interface{ Identity() string }); ok {
		return kernel.Capability("archive", archive.Identity())
	}
	return kernel.Capability("archive", s.Config.Node)
}

func (s *Server) requirements(tenant string, config wire.SessionConfig) []string {
	return append(s.Effects.Requirements(tenant, config),
		kernel.HarnessCapability(config.Harness, config.Model.Provider), s.archiveCapability())
}

func (s *Server) audit(ctx context.Context, p kernel.Principal, sessionID, kind string, data any) error {
	_, err := s.Control.Accept(ctx, p, sessionID, wire.Input{RequestID: "audit:" + rand.Text(), Data: wire.InputData{Kind: "journal", Name: &kind, Data: kernel.JSON(data)}})
	return err
}

func (s *Server) Start(parent context.Context) error {
	ctx, cancel := context.WithCancel(parent)
	s.cancel = cancel
	if err := s.Control.RegisterNode(ctx, s.Config.Node, s.Config.Lease.Milliseconds()); err != nil {
		cancel()
		return err
	}
	capabilities := append(s.Harness.Capabilities(), s.Effects.Capabilities()...)
	capabilities = append(capabilities, s.archiveCapability())
	if err := s.Control.ConfigureNode(ctx, s.Config.Node, s.Config.InternalURL, kernel.Digest(s.Config.Key), capabilities); err != nil {
		_ = s.Control.RetireNode(ctx, s.Config.Node)
		cancel()
		return err
	}
	s.healthy.Store(true)
	s.wg.Add(1)
	go s.coordinate(ctx)
	return nil
}

func (s *Server) Stop(ctx context.Context) error {
	s.draining.Store(true)
	if s.cancel != nil {
		s.cancel()
	}
	done := make(chan struct{})
	go func() { s.wg.Wait(); close(done) }()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-done:
	}
	s.healthy.Store(false)
	return errors.Join(s.Control.RetireNode(ctx, s.Config.Node), s.Harness.Close())
}

func (s *Server) routes() error {
	s.mux = http.NewServeMux()
	registered := map[string]bool{}
	add := func(pattern, permission string, fn handler) {
		s.mux.HandleFunc(pattern, s.wrap(permission, fn))
		registered[pattern] = true
	}
	add("GET /v2/identity", "", s.identity)
	add("POST /v2/sessions", "session:create", s.create)
	add("GET /v2/sessions", "session:list", s.list)
	add("GET /v2/sessions/{session}", "session:read", s.session)
	add("DELETE /v2/sessions/{session}", "session:delete", s.delete)
	add("POST /v2/sessions/{session}/inputs", "input:write", s.input)
	add("GET /v2/sessions/{session}/commands/{command}", "session:read", s.command)
	add("GET /v2/sessions/{session}/records", "session:read", s.records)
	s.mux.HandleFunc("GET /internal/v2/records/{session}/{sequence}", s.internalRecord)
	registered["GET /internal/v2/records/{session}/{sequence}"] = true
	add("GET /v2/sessions/{session}/events", "session:read", s.events)
	add("GET /v2/sessions/{session}/runs", "session:read", s.runs)
	add("GET /v2/sessions/{session}/runs/{run}/children", "session:read", s.children)
	add("GET /v2/sessions/{session}/runs/{run}/children/{child}/result", "session:read", s.childResult)
	add("GET /v2/sessions/{session}/budget", "session:read", s.budget)
	add("GET /v2/sessions/{session}/operations", "session:read", s.operations)
	add("GET /v2/sessions/{session}/operations/{operation}", "session:read", s.operation)
	add("POST /v2/sessions/{session}/operations/{operation}/claim", "operation:claim", s.claim)
	s.mux.HandleFunc("POST /v2/sessions/{session}/operations/{operation}/receipt", s.receipt)
	registered["POST /v2/sessions/{session}/operations/{operation}/receipt"] = true
	add("POST /v2/sessions/{session}/operations/{operation}/retry", "operation:retry", s.retry)
	add("GET /v2/sessions/{session}/interactions", "session:read", s.interactions)
	add("POST /v2/sessions/{session}/interactions", "interaction:create", s.interact)
	add("POST /v2/sessions/{session}/interactions/{interaction}", "interaction:answer", s.answer)
	add("GET /v2/sessions/{session}/connections", "session:read", s.connections)
	add("POST /v2/sessions/{session}/connections", "connection:attach", s.attach)
	add("DELETE /v2/sessions/{session}/connections/{connection}", "connection:revoke", s.revoke)
	for _, scope := range []string{"state", "memory"} {
		add("GET /v2/sessions/{session}/"+scope, "session:read", s.documents(scope))
		add("PUT /v2/sessions/{session}/"+scope+"/{key}", scope+":write", s.putDocument(scope))
		add("DELETE /v2/sessions/{session}/"+scope+"/{key}", scope+":write", s.deleteDocument(scope))
	}
	add("GET /v2/sessions/{session}/files", "session:read", s.files)
	add("PUT /v2/sessions/{session}/files/{file}", "file:write", s.putFile)
	add("GET /v2/sessions/{session}/files/{file}", "file:read", s.getFile)
	add("DELETE /v2/sessions/{session}/files/{file}", "file:write", s.deleteFile)
	add("GET /v2/sessions/{session}/schedules", "session:read", s.schedules)
	add("POST /v2/sessions/{session}/schedules", "schedule:write", s.schedule)
	add("DELETE /v2/sessions/{session}/schedules/{schedule}", "schedule:write", s.unschedule)
	add("POST /v2/sessions/{session}/webhooks", "webhook:write", s.input)
	add("POST /v2/sessions/{session}/delegations", "session:delegate", s.delegate)
	add("POST /v2/sessions/{session}/handoff", "session:handoff", s.handoff)
	add("POST /v2/sessions/{session}/participants", "participant:grant", s.grant)
	add("DELETE /v2/sessions/{session}/participants/{participant}", "participant:revoke", s.revokeParticipant)
	add("GET /v2/sessions/{session}/export", "session:export", s.export)
	add("GET /v2/catalog", "catalog:read", func(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
		return writeJSON(w, s.Effects.Catalog())
	})
	add("POST /v2/packages", "package:write", s.packageAdmission)
	add("GET /v2/packages/{package}", "package:read", s.packageGet)
	s.mux.HandleFunc("GET /health/live", func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(http.StatusNoContent) })
	s.mux.HandleFunc("GET /health/ready", func(w http.ResponseWriter, r *http.Request) {
		if !s.healthy.Load() || s.draining.Load() {
			w.WriteHeader(http.StatusServiceUnavailable)
		} else {
			w.WriteHeader(http.StatusNoContent)
		}
	})
	registered["GET /health/live"] = true
	registered["GET /health/ready"] = true
	add("POST /operator/drain", "operator", func(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
		s.draining.Store(true)
		return s.storage(w, r, p)
	})
	add("GET /operator/storage", "operator", s.storage)
	add("GET /operator/nodes", "operator", s.nodes)
	add("POST /operator/sessions/{session}/transfers", "operator", s.transferCreate)
	add("GET /operator/sessions/{session}/transfers/{transfer}", "operator", s.transferRead)
	add("DELETE /operator/sessions/{session}/transfers/{transfer}", "operator", s.transferCancel)
	for route := range wire.Routes {
		if !registered[route] {
			return fmt.Errorf("canonical route has no Go handler: %s", route)
		}
	}
	for route := range registered {
		if _, ok := wire.Routes[route]; !ok {
			return fmt.Errorf("Go route is not in the canonical contract: %s", route)
		}
	}
	return nil
}
