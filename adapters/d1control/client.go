// Package d1control calls authenticated, finite D1 domain operations. It never
// transmits SQL or treats a lost HTTP response as execution permission.
package d1control

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

type Store struct {
	origin string
	key    []byte
	client *http.Client
}

func Open(ctx context.Context, origin string, key []byte, development bool) (*Store, error) {
	u, err := url.Parse(origin)
	if err != nil || u.Host == "" || u.User != nil || u.Path != "" || u.RawQuery != "" || u.Fragment != "" ||
		(u.Scheme != "https" && !(development && u.Scheme == "http")) || len(key) != 32 {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "D1 needs an HTTPS Worker origin and a 32-byte service signing key")
	}
	transport := &http.Transport{DisableKeepAlives: true, MaxConnsPerHost: 16, ResponseHeaderTimeout: 20 * time.Second}
	transport.Protocols = new(http.Protocols)
	transport.Protocols.SetHTTP1(true)
	store := &Store{origin: strings.TrimRight(origin, "/"), key: bytes.Clone(key), client: &http.Client{
		Transport: transport, Timeout: 30 * time.Second,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
	}}
	if err = store.call(ctx, "Initialize", nil, nil); err != nil {
		_ = store.Close()
		return nil, err
	}
	return store, nil
}

func (s *Store) Close() error {
	s.client.CloseIdleConnections()
	return nil
}

var readOnly = map[string]bool{
	"Clock": true, "Session": true, "List": true, "Command": true, "Pending": true, "NextInput": true,
	"Allocations": true, "LocateRecord": true, "Runs": true, "Operation": true, "Operations": true,
	"Interactions": true, "Connections": true, "Documents": true, "Files": true, "Schedules": true,
	"Participant": true, "Coverage": true, "Report": true, "Budget": true, "Package": true, "Children": true,
	"RunnableRuns": true, "PendingResults": true, "Interaction": true, "ActiveConnection": true, "ChildState": true, "File": true,
	"Nodes": true, "Transfer": true, "PendingTransfers": true, "TransferRuns": true,
}

func (s *Store) call(ctx context.Context, method string, arguments []any, output any) error {
	values := make([]string, 0, len(arguments))
	for _, argument := range arguments {
		body, err := json.Marshal(argument)
		if err != nil {
			return kernel.Fail(wire.ErrorCodeInvalid, "control argument is not valid JSON")
		}
		values = append(values, string(body))
	}
	body := kernel.JSON(struct {
		Arguments []string           `json:"arguments"`
		Authority *control.Authority `json:"authority,omitempty"`
	}{values, control.RequestAuthority(ctx)})
	if len(body) > 2<<20 {
		return kernel.Fail(wire.ErrorCodeInvalid, "D1 control request exceeds two MiB")
	}
	path := "/control/v1/" + method
	stamp := strconv.FormatInt(time.Now().UnixMilli(), 10)
	mac := hmac.New(sha256.New, s.key)
	_, _ = mac.Write([]byte("brain-d1/1\x00" + path + "\x00" + stamp + "\x00"))
	_, _ = mac.Write(body)
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, s.origin+path, bytes.NewReader(body))
	if err != nil {
		return err
	}
	request.GetBody = nil
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Brain-Request-Time", stamp)
	request.Header.Set("X-Brain-Control-Signature", hex.EncodeToString(mac.Sum(nil)))
	uncertain := func() error {
		code := wire.ErrorCodeUncertain
		if readOnly[method] {
			code = wire.ErrorCodeUnavailable
		}
		return kernel.Fail(code, "D1 control response is unavailable; no dispatch permission was returned")
	}
	response, err := s.client.Do(request)
	if err != nil {
		return uncertain()
	}
	defer response.Body.Close()
	limit := int64(4 << 20)
	if response.StatusCode != http.StatusOK {
		limit = 65536
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, limit+1))
	if err != nil || int64(len(data)) > limit {
		return uncertain()
	}
	if response.StatusCode != http.StatusOK {
		var failure wire.ErrorResponse
		if json.Unmarshal(data, &failure) != nil || failure.Code == "" {
			return uncertain()
		}
		return &kernel.Fault{Response: failure}
	}
	var envelope struct {
		Value json.RawMessage `json:"value"`
	}
	if json.Unmarshal(data, &envelope) != nil || envelope.Value == nil {
		return uncertain()
	}
	if output != nil && json.Unmarshal(envelope.Value, output) != nil {
		return uncertain()
	}
	return nil
}

func tuple(values []json.RawMessage, output ...any) error {
	if len(values) != len(output) {
		return kernel.Fail(wire.ErrorCodeCorrupt, "D1 control tuple does not match its operation")
	}
	for i := range output {
		if json.Unmarshal(values[i], output[i]) != nil {
			return kernel.Fail(wire.ErrorCodeCorrupt, "D1 control tuple is invalid")
		}
	}
	return nil
}

func (s *Store) Clock(ctx context.Context) (value int64, err error) {
	err = s.call(ctx, "Clock", nil, &value)
	return
}
func (s *Store) RegisterNode(ctx context.Context, node string, lease int64) error {
	return s.call(ctx, "RegisterNode", []any{node, lease}, nil)
}
func (s *Store) ConfigureNode(ctx context.Context, node, endpoint, custody string, capabilities []string) error {
	return s.call(ctx, "ConfigureNode", []any{node, endpoint, custody, capabilities}, nil)
}
func (s *Store) RenewNode(ctx context.Context, node string, lease int64) error {
	return s.call(ctx, "RenewNode", []any{node, lease}, nil)
}
func (s *Store) RetireNode(ctx context.Context, node string) error {
	return s.call(ctx, "RetireNode", []any{node}, nil)
}
func (s *Store) Create(ctx context.Context, tenant string, request wire.SessionCreate, parent, budget string, requirements []string) (value control.Session, err error) {
	err = s.call(ctx, "Create", []any{tenant, request, parent, budget, requirements}, &value)
	return
}
func (s *Store) Session(ctx context.Context, tenant, id string) (value control.Session, err error) {
	err = s.call(ctx, "Session", []any{tenant, id}, &value)
	return
}
func (s *Store) List(ctx context.Context, tenant, after string, limit int) (values []control.Session, err error) {
	err = s.call(ctx, "List", []any{tenant, after, limit}, &values)
	return
}
func (s *Store) ClaimOwner(ctx context.Context, session, node string) (value control.Owner, err error) {
	err = s.call(ctx, "ClaimOwner", []any{session, node}, &value)
	return
}
func (s *Store) Accept(ctx context.Context, principal kernel.Principal, session string, input wire.Input) (value wire.CommandReceipt, err error) {
	err = s.call(ctx, "Accept", []any{principal, session, input}, &value)
	return
}
func (s *Store) Command(ctx context.Context, session, id string) (value wire.CommandReceipt, err error) {
	err = s.call(ctx, "Command", []any{session, id}, &value)
	return
}
func (s *Store) Pending(ctx context.Context, node string, limit int) (values []string, err error) {
	err = s.call(ctx, "Pending", []any{node, limit}, &values)
	return
}
func (s *Store) FinishStep(ctx context.Context, session string, owner control.Owner) error {
	return s.call(ctx, "FinishStep", []any{session, owner}, nil)
}
func (s *Store) NextInput(ctx context.Context, session string, owner control.Owner) (value wire.Input, id, source string, err error) {
	var values []json.RawMessage
	if err = s.call(ctx, "NextInput", []any{session, owner}, &values); err == nil {
		err = tuple(values, &value, &id, &source)
	}
	return
}
func (s *Store) ApplyInput(ctx context.Context, session string, apply control.ApplyInput) error {
	return s.call(ctx, "ApplyInput", []any{session, apply}, nil)
}
func (s *Store) Allocate(ctx context.Context, session string, owner control.Owner) (value int64, err error) {
	err = s.call(ctx, "Allocate", []any{session, owner}, &value)
	return
}
func (s *Store) Written(ctx context.Context, session string, owner control.Owner, sequence int64, digest string, bytes int64) error {
	return s.call(ctx, "Written", []any{session, owner, sequence, digest, bytes}, nil)
}
func (s *Store) SealUnused(ctx context.Context, session string, owner control.Owner, sequence int64) error {
	return s.call(ctx, "SealUnused", []any{session, owner, sequence}, nil)
}
func (s *Store) Allocations(ctx context.Context, session string, after int64, limit int) (values []control.Allocation, high int64, err error) {
	var response []json.RawMessage
	if err = s.call(ctx, "Allocations", []any{session, after, limit}, &response); err == nil {
		err = tuple(response, &values, &high)
	}
	return
}
func (s *Store) LocateRecord(ctx context.Context, session string, sequence int64) (value control.Allocation, err error) {
	err = s.call(ctx, "LocateRecord", []any{session, sequence}, &value)
	return
}
func (s *Store) Runs(ctx context.Context, session string, limit int, after ...string) (values []control.Run, err error) {
	err = s.call(ctx, "Runs", []any{session, limit, control.After(after)}, &values)
	return
}
func (s *Store) SaveDecision(ctx context.Context, session string, owner control.Owner, run string, step uint64, decision wire.Decision, process ...string) error {
	if len(process) != 0 && process[0] != "" {
		return s.call(ctx, "SaveHarnessDecision", []any{session, owner, run, step, decision, process[0]}, nil)
	}
	return s.call(ctx, "SaveDecision", []any{session, owner, run, step, decision}, nil)
}
func (s *Store) BeginDecision(ctx context.Context, session string, owner control.Owner, run string, step uint64, digest, process string, repeatable bool) error {
	return s.call(ctx, "BeginDecision", []any{session, owner, run, step, digest, process, repeatable}, nil)
}
func (s *Store) Children(ctx context.Context, session, run string, step uint64) (value wire.ChildResults, err error) {
	err = s.call(ctx, "Children", []any{session, run, step}, &value)
	return
}
func (s *Store) CompleteRun(ctx context.Context, session string, owner control.Owner, run string, step uint64, record wire.Record) error {
	return s.call(ctx, "CompleteRun", []any{session, owner, run, step, record}, nil)
}
func (s *Store) WaitDispatch(ctx context.Context, session string, owner control.Owner, run string, step uint64, operation string) error {
	return s.call(ctx, "WaitDispatch", []any{session, owner, run, step, operation}, nil)
}
func (s *Store) AdvanceRun(ctx context.Context, session string, owner control.Owner, run string, step uint64, status wire.RunStatus, wait *wire.Wait, failure *wire.ErrorResponse) error {
	return s.call(ctx, "AdvanceRun", []any{session, owner, run, step, status, wait, failure}, nil)
}
func (s *Store) Prepare(ctx context.Context, owner control.Owner, request control.Prepare) (value control.Operation, err error) {
	err = s.call(ctx, "Prepare", []any{owner, request}, &value)
	return
}
func (s *Store) Operation(ctx context.Context, session, id string) (value control.Operation, err error) {
	err = s.call(ctx, "Operation", []any{session, id}, &value)
	return
}
func (s *Store) Operations(ctx context.Context, session string, limit int, after ...string) (values []control.Operation, err error) {
	err = s.call(ctx, "Operations", []any{session, limit, control.After(after)}, &values)
	return
}
func (s *Store) ClaimDispatch(ctx context.Context, session string, owner control.Owner, id, claimant, request string) (value control.Operation, err error) {
	err = s.call(ctx, "ClaimDispatch", []any{session, owner, id, claimant, request}, &value)
	return
}
func (s *Store) RecordReceipt(ctx context.Context, session, id string, receipt wire.Receipt, sequence int64, digest string) (value control.Operation, err error) {
	err = s.call(ctx, "RecordReceipt", []any{session, id, receipt, sequence, digest}, &value)
	return
}
func (s *Store) BindResult(ctx context.Context, session string, owner control.Owner, id string, sequence int64, digest string) error {
	return s.call(ctx, "BindResult", []any{session, owner, id, sequence, digest}, nil)
}
func (s *Store) ExpireOperations(ctx context.Context, limit int) (value int, err error) {
	err = s.call(ctx, "ExpireOperations", []any{limit}, &value)
	return
}
func (s *Store) Interact(ctx context.Context, session string, request wire.InteractionCreate) (value wire.Interaction, err error) {
	err = s.call(ctx, "Interact", []any{session, request}, &value)
	return
}
func (s *Store) Interactions(ctx context.Context, session string, limit int, after ...string) (values []wire.Interaction, err error) {
	err = s.call(ctx, "Interactions", []any{session, limit, control.After(after)}, &values)
	return
}
func (s *Store) Answer(ctx context.Context, session, id string, answer wire.InteractionAnswer) (value wire.Interaction, err error) {
	err = s.call(ctx, "Answer", []any{session, id, answer}, &value)
	return
}
func (s *Store) Attach(ctx context.Context, session string, request wire.ConnectionAttach) (value wire.Connection, err error) {
	err = s.call(ctx, "Attach", []any{session, request}, &value)
	return
}
func (s *Store) Connections(ctx context.Context, session string, limit int, after ...string) (values []wire.Connection, err error) {
	err = s.call(ctx, "Connections", []any{session, limit, control.After(after)}, &values)
	return
}
func (s *Store) RevokeConnection(ctx context.Context, session, id string) (value wire.Connection, err error) {
	err = s.call(ctx, "RevokeConnection", []any{session, id}, &value)
	return
}
func (s *Store) Documents(ctx context.Context, session, scope string, limit int, after ...string) (values []wire.Document, err error) {
	err = s.call(ctx, "Documents", []any{session, scope, limit, control.After(after)}, &values)
	return
}
func (s *Store) PutDocument(ctx context.Context, session, scope, key string, request wire.DocumentUpdate) (value wire.Document, err error) {
	err = s.call(ctx, "PutDocument", []any{session, scope, key, request}, &value)
	return
}
func (s *Store) DeleteDocument(ctx context.Context, session, scope, key string) (value wire.Document, err error) {
	err = s.call(ctx, "DeleteDocument", []any{session, scope, key}, &value)
	return
}
func (s *Store) PutFile(ctx context.Context, session string, file wire.FileInfo) error {
	return s.call(ctx, "PutFile", []any{session, file}, nil)
}
func (s *Store) Files(ctx context.Context, session string, limit int, after ...string) (values []wire.FileInfo, err error) {
	err = s.call(ctx, "Files", []any{session, limit, control.After(after)}, &values)
	return
}
func (s *Store) DeleteFile(ctx context.Context, session, name string) (value wire.FileInfo, err error) {
	err = s.call(ctx, "DeleteFile", []any{session, name}, &value)
	return
}
func (s *Store) Schedule(ctx context.Context, session string, request wire.ScheduleCreate) (value wire.Schedule, err error) {
	err = s.call(ctx, "Schedule", []any{session, request}, &value)
	return
}
func (s *Store) Schedules(ctx context.Context, session string, limit int, after ...string) (values []wire.Schedule, err error) {
	err = s.call(ctx, "Schedules", []any{session, limit, control.After(after)}, &values)
	return
}
func (s *Store) CancelSchedule(ctx context.Context, session, id string) (value wire.Schedule, err error) {
	err = s.call(ctx, "CancelSchedule", []any{session, id}, &value)
	return
}
func (s *Store) FireTimers(ctx context.Context, limit int) (value int, err error) {
	err = s.call(ctx, "FireTimers", []any{limit}, &value)
	return
}
func (s *Store) Grant(ctx context.Context, session string, request wire.ParticipantGrant) (value control.Participant, err error) {
	err = s.call(ctx, "Grant", []any{session, request}, &value)
	return
}
func (s *Store) Participant(ctx context.Context, id string) (value control.Participant, err error) {
	err = s.call(ctx, "Participant", []any{id}, &value)
	return
}
func (s *Store) RevokeParticipant(ctx context.Context, session, id string) error {
	return s.call(ctx, "RevokeParticipant", []any{session, id}, nil)
}
func (s *Store) Handoff(ctx context.Context, session string, request wire.Handoff) (value control.Session, err error) {
	err = s.call(ctx, "Handoff", []any{session, request}, &value)
	return
}
func (s *Store) Delete(ctx context.Context, session string) (value control.Session, err error) {
	err = s.call(ctx, "Delete", []any{session}, &value)
	return
}
func (s *Store) PublishCoverage(ctx context.Context, owner control.Owner, coverage control.Coverage) error {
	return s.call(ctx, "PublishCoverage", []any{owner, coverage}, nil)
}
func (s *Store) Coverage(ctx context.Context, session string, after int64, limit int) (values []control.Coverage, err error) {
	err = s.call(ctx, "Coverage", []any{session, after, limit}, &values)
	return
}
func (s *Store) Report(ctx context.Context) (value wire.StorageReport, err error) {
	err = s.call(ctx, "Report", nil, &value)
	return
}
func (s *Store) Budget(ctx context.Context, session string) (value wire.SessionBudget, err error) {
	err = s.call(ctx, "Budget", []any{session}, &value)
	return
}
func (s *Store) PropagateControls(ctx context.Context, limit int) (value []string, err error) {
	err = s.call(ctx, "PropagateControls", []any{limit}, &value)
	return
}

func (s *Store) Delegate(ctx context.Context, parent string, owner control.Owner, run string, step, index uint64, request wire.SessionCreate, requirements []string) (value control.Session, err error) {
	err = s.call(ctx, "Delegate", []any{parent, owner, run, step, index, request, requirements}, &value)
	return
}
func (s *Store) PutPackage(ctx context.Context, tenant string, pkg wire.Package) error {
	return s.call(ctx, "PutPackage", []any{tenant, pkg}, nil)
}
func (s *Store) Package(ctx context.Context, tenant, id string) (value wire.Package, err error) {
	err = s.call(ctx, "Package", []any{tenant, id}, &value)
	return
}

var _ control.Store = (*Store)(nil)
