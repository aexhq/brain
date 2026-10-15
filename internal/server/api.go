package server

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"io"
	"mime"
	"net/http"
	"path"
	"strconv"
	"strings"
	"time"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aexhq/brain/internal/packages"
	"github.com/aexhq/brain/internal/validation"
)

func boundedBody(w http.ResponseWriter, r *http.Request, limit int64) ([]byte, error) {
	body, err := io.ReadAll(http.MaxBytesReader(w, r.Body, limit))
	if err != nil {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "request body exceeds its byte bound or is unreadable")
	}
	return body, nil
}

func (s *Server) validateConfig(ctx context.Context, tenant string, config wire.SessionConfig) error {
	if err := kernel.ValidateConfig(config); err != nil {
		return err
	}
	if err := s.Harness.Validate(config); err != nil {
		return err
	}
	if err := s.Effects.Validate(ctx, tenant, config); err != nil {
		return err
	}
	for _, a := range config.Actions {
		if _, err := validation.Schema(a.InputSchema); err != nil {
			return err
		}
		if len(a.OutputSchema) > 0 {
			if _, err := validation.Schema(a.OutputSchema); err != nil {
				return err
			}
		}
	}
	for _, event := range config.Events {
		if _, err := validation.Schema(event.Schema); err != nil {
			return err
		}
	}
	return nil
}

func (s *Server) create(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	if s.draining.Load() || !s.healthy.Load() {
		return kernel.Fail(wire.ErrorCodeOverloaded, "server admission is drained")
	}
	request, err := readBody[wire.SessionCreate](s, w, r, "SessionCreate")
	if err != nil {
		return err
	}
	if !kernel.Identifier(request.RequestID) {
		return kernel.Fail(wire.ErrorCodeInvalid, "request_id must be an identifier")
	}
	if err = s.validateConfig(r.Context(), p.Tenant, request.Config); err != nil {
		return err
	}
	if request.Message != nil && len(*request.Message) > int(request.Config.Limits.InputBytes) {
		return kernel.Fail(wire.ErrorCodeInvalid, "initial message exceeds input limit")
	}
	session, err := s.Control.Create(r.Context(), p.Tenant, request, "", "", s.requirements(p.Tenant, request.Config))
	if err != nil {
		return err
	}
	return writeJSON(w, session.View)
}

func (s *Server) list(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	sessions, err := s.Control.List(r.Context(), p.Tenant, r.URL.Query().Get("after"), limit)
	if err != nil {
		return err
	}
	page := wire.SessionPage{Sessions: []wire.SessionView{}}
	for _, session := range sessions {
		page.Sessions = append(page.Sessions, session.View)
	}
	if len(sessions) != 0 {
		page.Next = kernel.Ptr(sessions[len(sessions)-1].View.ID)
	}
	return writeJSON(w, page)
}
func (s *Server) session(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	session, err := s.Control.Session(r.Context(), p.Tenant, r.PathValue("session"))
	if err != nil {
		return err
	}
	return writeJSON(w, session.View)
}
func (s *Server) delete(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	if err := s.audit(r.Context(), p, r.PathValue("session"), "session.deletion_requested", map[string]any{"source": p.Subject}); err != nil {
		return err
	}
	session, err := s.Control.Delete(r.Context(), r.PathValue("session"))
	if err != nil {
		return err
	}
	s.cancelSession(session.View.ID)
	return writeJSON(w, session.View)
}

func (s *Server) validateInput(config wire.SessionConfig, input wire.InputData, source string) error {
	switch input.Kind {
	case "message":
		if len(kernel.Value(input.Text)) == 0 {
			return kernel.Fail(wire.ErrorCodeInvalid, "message cannot be empty")
		}
	case "action":
		if input.RetryOf != nil {
			return kernel.Fail(wire.ErrorCodeInvalid, "use the explicit operation retry endpoint")
		}
		action, err := kernel.Action(config, kernel.Value(input.Name))
		if err != nil {
			return err
		}
		return validation.Validate(action.InputSchema, input.Arguments)
	case "event":
		route, err := kernel.Route(config, kernel.Value(input.Name), kernel.Value(input.Version), source)
		if err != nil {
			return err
		}
		if route.Disposition == wire.DispositionState && len(input.Data) > 65536 {
			return kernel.Fail(wire.ErrorCodeInvalid, "state event exceeds the document byte bound")
		}
		return validation.Validate(route.Schema, input.Data)
	case "control":
		if input.Control == nil {
			return kernel.Fail(wire.ErrorCodeInvalid, "control is required")
		}
	default:
		return kernel.Fail(wire.ErrorCodeInvalid, "unknown input kind")
	}
	return nil
}

func (s *Server) input(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	input, err := readBody[wire.Input](s, w, r, "Input")
	if err != nil {
		return err
	}
	if !kernel.Identifier(input.RequestID) {
		return kernel.Fail(wire.ErrorCodeInvalid, "request_id must be an identifier")
	}
	if (s.draining.Load() || !s.healthy.Load()) && input.Data.Kind != "control" {
		return kernel.Fail(wire.ErrorCodeOverloaded, "server admission is drained")
	}
	session, err := s.Control.Session(r.Context(), p.Tenant, r.PathValue("session"))
	if err != nil {
		return err
	}
	if err = s.validateInput(session.View.Config, input.Data, p.Subject); err != nil {
		return err
	}
	if input.Data.Kind == "control" && !p.Allows(session.View.ID, "session:control") {
		return kernel.Fail(wire.ErrorCodeForbidden, "control permission is required")
	}
	if input.Data.Kind == "action" && !p.Allows(session.View.ID, "action:"+kernel.Value(input.Data.Name)) {
		return kernel.Fail(wire.ErrorCodeForbidden, "action permission is required")
	}
	receipt, err := s.Control.Accept(r.Context(), p, session.View.ID, input)
	if err != nil {
		return err
	}
	if input.Data.Kind == "control" && (kernel.Value(input.Data.Control) == wire.ControlKindCancel || kernel.Value(input.Data.Control) == wire.ControlKindClose) {
		s.cancelSession(session.View.ID)
	}
	return writeJSON(w, receipt)
}
func (s *Server) command(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	receipt, err := s.Control.Command(r.Context(), r.PathValue("session"), r.PathValue("command"))
	if err != nil {
		return err
	}
	return writeJSON(w, receipt)
}
func (s *Server) records(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	after := r.URL.Query().Get("after")
	if after == "" {
		after = "0"
	}
	sequence, err := kernel.Sequence(after)
	if err != nil {
		return err
	}
	page, err := s.History.Page(r.Context(), r.PathValue("session"), sequence, limit)
	if err != nil {
		return err
	}
	return writeJSON(w, page)
}

func (s *Server) events(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	after := r.Header.Get("Last-Event-ID")
	if after == "" {
		after = r.URL.Query().Get("after")
	}
	if after == "" {
		after = "0"
	}
	sequence, err := kernel.Sequence(after)
	if err != nil {
		return err
	}
	notifications, unsubscribe, err := s.History.Subscribe(r.PathValue("session"))
	if err != nil {
		return err
	}
	defer unsubscribe()
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("X-Accel-Buffering", "no")
	controller := http.NewResponseController(w)
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		if _, err = s.principal(r); err != nil {
			return nil
		}
		if _, err = s.Control.Session(r.Context(), p.Tenant, r.PathValue("session")); err != nil {
			return nil
		}
		page, err := s.History.Page(r.Context(), r.PathValue("session"), sequence, 64)
		if err != nil {
			return nil
		}
		records := map[string]wire.Record{}
		gaps := map[string]wire.Gap{}
		for _, record := range page.Records {
			records[record.Sequence] = record
		}
		for _, gap := range page.Gaps {
			gaps[gap.First] = gap
		}
		next, _ := kernel.Sequence(page.Next)
		advanced := false
		for current := sequence + 1; current <= next; current++ {
			id := strconv.FormatInt(current, 10)
			if gap, ok := gaps[id]; ok {
				if gap.Kind == wire.GapKindReserved {
					break
				}
				if err = controller.SetWriteDeadline(time.Now().Add(10 * time.Second)); err != nil {
					return nil
				}
				if _, err = io.WriteString(w, "id: "+id+"\nevent: gap\ndata: "+string(kernel.JSON(gap))+"\n\n"); err != nil {
					return nil
				}
			} else if record, ok := records[id]; ok {
				if err = controller.SetWriteDeadline(time.Now().Add(10 * time.Second)); err != nil {
					return nil
				}
				if _, err = io.WriteString(w, "id: "+id+"\nevent: record\ndata: "+string(kernel.JSON(record))+"\n\n"); err != nil {
					return nil
				}
			} else {
				break
			}
			sequence = current
			advanced = true
		}
		if err = controller.Flush(); err != nil {
			return nil
		}
		if page.More && advanced {
			continue
		}
		select {
		case <-r.Context().Done():
			return nil
		case <-notifications:
		case <-ticker.C:
			_ = controller.SetWriteDeadline(time.Now().Add(10 * time.Second))
			if _, err = io.WriteString(w, ": keep-alive\n\n"); err != nil {
				return nil
			}
			if err = controller.Flush(); err != nil {
				return nil
			}
		}
	}
}

func (s *Server) runs(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	runs, err := s.Control.Runs(r.Context(), r.PathValue("session"), limit, r.URL.Query().Get("after"))
	if err != nil {
		return err
	}
	out := []wire.Run{}
	for _, run := range runs {
		out = append(out, run.View)
	}
	return writeJSON(w, out)
}
func (s *Server) operations(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	operations, err := s.Control.Operations(r.Context(), r.PathValue("session"), limit, r.URL.Query().Get("after"))
	if err != nil {
		return err
	}
	out := []wire.Operation{}
	for _, operation := range operations {
		out = append(out, operation.View)
	}
	return writeJSON(w, out)
}
func (s *Server) operation(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	operation, err := s.Control.Operation(r.Context(), r.PathValue("session"), r.PathValue("operation"))
	if err != nil {
		return err
	}
	operation = s.available(r.Context(), operation)
	return writeJSON(w, operation.View)
}

func (s *Server) claim(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.Claim](s, w, r, "Claim")
	if err != nil {
		return err
	}
	session, err := s.Control.Session(r.Context(), p.Tenant, r.PathValue("session"))
	if err != nil {
		return err
	}
	operation, err := s.Control.Operation(r.Context(), session.View.ID, r.PathValue("operation"))
	if err != nil {
		return err
	}
	if operation.View.Kind != wire.EffectKindAction || operation.View.Recipient != request.ConnectionID || operation.Generation != request.Generation {
		return kernel.Fail(wire.ErrorCodeForbidden, "claim is not from the original invocation recipient")
	}
	action, err := kernel.Action(session.View.Config, operation.View.Action)
	if err != nil {
		return err
	}
	application := false
	for _, template := range session.View.Config.Connections {
		if template.Name == action.Connection && template.Kind == wire.ConnectionKindApplication {
			application = true
			break
		}
	}
	if !application {
		return kernel.Fail(wire.ErrorCodeForbidden, "only an application connection accepts public dispatch claims")
	}
	operation, err = s.Control.ClaimDispatch(r.Context(), session.View.ID, session.Owner, operation.View.ID, request.ConnectionID, request.RequestID)
	if err != nil {
		return err
	}
	return writeJSON(w, wire.Execution{Operation: operation.View, Arguments: operation.Arguments, ReceiptToken: s.receiptToken(operation), MaxOutputBytes: uint64(operation.OutputLimit), DeadlineMs: uint64(operation.Deadline)})
}

func (s *Server) receipt(w http.ResponseWriter, r *http.Request) {
	err := func() error {
		token := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
		if !strings.HasPrefix(token, "r_"+r.PathValue("operation")+".") {
			return kernel.Fail(wire.ErrorCodeUnauthorized, "receipt authority is invalid")
		}
		operation, err := s.Control.Operation(r.Context(), r.PathValue("session"), r.PathValue("operation"))
		if err != nil {
			if kernel.Response(err).Code == wire.ErrorCodeNotFound {
				return kernel.Fail(wire.ErrorCodeUnauthorized, "receipt authority is invalid")
			}
			return err
		}
		if !hmacEqual(token, s.receiptToken(operation)) {
			return kernel.Fail(wire.ErrorCodeUnauthorized, "receipt authority is invalid")
		}
		receipt, err := readBody[wire.Receipt](s, w, r, "Receipt")
		if err != nil {
			return err
		}
		if operation.View.Kind == wire.EffectKindAction && receipt.Outcome == wire.OutcomeCompleted {
			schema, e := s.Control.ReceiptOutputSchema(r.Context(), operation.View.SessionID, operation.View.ID)
			if e != nil {
				return e
			}
			if schema != "" && validation.Validate(json.RawMessage(schema), receipt.Result) != nil {
				receipt.Error = kernel.Ptr("completed action returned bytes outside its declared output schema")
			}
		}
		result, err := s.Control.RecordReceipt(r.Context(), operation.View.SessionID, operation.View.ID, receipt, 0, "")
		if err != nil {
			return err
		}
		return writeJSON(w, result.View)
	}()
	if err != nil {
		writeError(w, err)
	}
}

func hmacEqual(a, b string) bool { return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1 }

func (s *Server) retry(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.Retry](s, w, r, "Retry")
	if err != nil {
		return err
	}
	old, err := s.Control.Operation(r.Context(), r.PathValue("session"), r.PathValue("operation"))
	if err != nil {
		return err
	}
	if old.View.Status != wire.OperationStatusUnknown && old.View.Status != wire.OperationStatusFailed {
		return kernel.Fail(wire.ErrorCodeConflict, "only explicitly failed or uncertain operations can be retried")
	}
	if old.View.Status == wire.OperationStatusUnknown && !request.AcknowledgeUncertainty {
		return kernel.Fail(wire.ErrorCodeInvalid, "retry requires acknowledging the original uncertain effect")
	}
	if old.View.Kind != wire.EffectKindAction {
		return kernel.Fail(wire.ErrorCodeUnsupported, "start an explicit new run for a failed model or lifecycle attempt")
	}
	if !p.Allows(old.View.SessionID, "action:"+old.View.Action) {
		return kernel.Fail(wire.ErrorCodeForbidden, "retry requires the original action grant")
	}
	if len(old.Arguments) == 0 && old.IntentSequence > 0 {
		record, e := s.History.Get(r.Context(), old.View.SessionID, old.IntentSequence, old.IntentDigest)
		if e != nil {
			return e
		}
		var intent struct {
			Arguments json.RawMessage `json:"arguments"`
		}
		if json.Unmarshal(record.Data, &intent) != nil {
			return kernel.Fail(wire.ErrorCodeCorrupt, "invalid retained action intent")
		}
		if kernel.Digest(intent.Arguments) != old.View.ArgumentsDigest {
			return kernel.Fail(wire.ErrorCodeCorrupt, "retained action arguments disagree with control")
		}
		old.Arguments = intent.Arguments
	}
	if len(old.Arguments) == 0 {
		return kernel.Fail(wire.ErrorCodeUnavailable, "original arguments are unavailable; retry cannot fabricate them")
	}
	input := wire.Input{RequestID: "retry:" + request.RequestID, Data: wire.InputData{Kind: "action", Name: &old.View.Action, Arguments: old.Arguments, RetryOf: &old.View.ID}}
	receipt, err := s.Control.Accept(r.Context(), p, old.View.SessionID, input)
	if err != nil {
		return err
	}
	return writeJSON(w, receipt)
}

func (s *Server) interactions(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	out, err := s.Control.Interactions(r.Context(), r.PathValue("session"), limit, r.URL.Query().Get("after"))
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}
func (s *Server) interact(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.InteractionCreate](s, w, r, "InteractionCreate")
	if err != nil {
		return err
	}
	if err = kernel.ValidateInteraction(request); err != nil {
		return err
	}
	if _, err = validation.Schema(request.Schema); err != nil {
		return err
	}
	if err = s.audit(r.Context(), p, r.PathValue("session"), "interaction.requested", request); err != nil {
		return err
	}
	out, err := s.Control.Interact(r.Context(), r.PathValue("session"), request)
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}
func (s *Server) answer(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.InteractionAnswer](s, w, r, "InteractionAnswer")
	if err != nil {
		return err
	}
	if err = kernel.ValidateAnswer(request); err != nil {
		return err
	}
	interaction, err := s.Control.Interaction(r.Context(), r.PathValue("session"), r.PathValue("interaction"))
	if err != nil {
		return err
	}
	if request.Accept {
		if err = validation.Validate(interaction.Request.Schema, request.Answer); err != nil {
			return err
		}
	}
	if interaction.Request.Kind == wire.InteractionKindApproval && !p.Allows(r.PathValue("session"), "operation:approve") {
		return kernel.Fail(wire.ErrorCodeForbidden, "approval permission is required")
	}
	if err = s.audit(r.Context(), p, r.PathValue("session"), "interaction.answer_requested", map[string]any{"id": r.PathValue("interaction"), "answer": request, "source": p.Subject}); err != nil {
		return err
	}
	out, err := s.Control.Answer(r.Context(), r.PathValue("session"), r.PathValue("interaction"), request)
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}

func (s *Server) connections(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	out, err := s.Control.Connections(r.Context(), r.PathValue("session"), limit, r.URL.Query().Get("after"))
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}
func (s *Server) attach(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.ConnectionAttach](s, w, r, "ConnectionAttach")
	if err != nil {
		return err
	}
	if err = kernel.ValidateAttachment(request); err != nil {
		return err
	}
	if err = s.audit(r.Context(), p, r.PathValue("session"), "connection.attach_requested", request); err != nil {
		return err
	}
	out, err := s.Control.Attach(r.Context(), r.PathValue("session"), request)
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}
func (s *Server) revoke(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	if err := s.audit(r.Context(), p, r.PathValue("session"), "connection.revoke_requested", map[string]string{"id": r.PathValue("connection")}); err != nil {
		return err
	}
	out, err := s.Control.RevokeConnection(r.Context(), r.PathValue("session"), r.PathValue("connection"))
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}

func (s *Server) documents(scope string) handler {
	return func(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
		limit, err := queryLimit(r)
		if err != nil {
			return err
		}
		out, err := s.Control.Documents(r.Context(), r.PathValue("session"), scope, limit, r.URL.Query().Get("after"))
		if err != nil {
			return err
		}
		return writeJSON(w, out)
	}
}
func (s *Server) putDocument(scope string) handler {
	return func(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
		request, err := readBody[wire.DocumentUpdate](s, w, r, "DocumentUpdate")
		if err != nil {
			return err
		}
		if !kernel.Identifier(r.PathValue("key")) || len(request.Value) > 65536 {
			return kernel.Fail(wire.ErrorCodeInvalid, "invalid document key or value size")
		}
		if err = s.audit(r.Context(), p, r.PathValue("session"), scope+".update_requested", map[string]any{"key": r.PathValue("key"), "request": request}); err != nil {
			return err
		}
		out, err := s.Control.PutDocument(r.Context(), r.PathValue("session"), scope, r.PathValue("key"), request)
		if err != nil {
			return err
		}
		return writeJSON(w, out)
	}
}
func (s *Server) deleteDocument(scope string) handler {
	return func(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
		if err := s.audit(r.Context(), p, r.PathValue("session"), scope+".delete_requested", map[string]string{"key": r.PathValue("key")}); err != nil {
			return err
		}
		out, err := s.Control.DeleteDocument(r.Context(), r.PathValue("session"), scope, r.PathValue("key"))
		if err != nil {
			return err
		}
		return writeJSON(w, out)
	}
}
func (s *Server) files(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	out, err := s.Control.Files(r.Context(), r.PathValue("session"), limit, r.URL.Query().Get("after"))
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}
func (s *Server) putFile(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	name := r.PathValue("file")
	if name == "" || len(name) > 128 || path.Base(name) != name || strings.ContainsAny(name, "\\:") {
		return kernel.Fail(wire.ErrorCodeInvalid, "file name must be a bounded leaf name")
	}
	body, err := boundedBody(w, r, 16<<20)
	if err != nil {
		return err
	}
	digest := kernel.Digest(body)
	file := wire.FileInfo{Name: name, Digest: digest, Size: uint64(len(body)), MediaType: r.Header.Get("Content-Type"), Provenance: p.Subject}
	if file.MediaType == "" {
		file.MediaType = "application/octet-stream"
	}
	if _, _, err = mime.ParseMediaType(file.MediaType); err != nil || len(file.MediaType) > 256 {
		return kernel.Fail(wire.ErrorCodeInvalid, "file media type is invalid or too long")
	}
	if err = s.audit(r.Context(), p, r.PathValue("session"), "file.put_requested", file); err != nil {
		return err
	}
	if err = s.Blobs.Put(r.Context(), "blobs/"+r.PathValue("session")+"/"+digest, body); err != nil {
		return err
	}
	if err = s.Control.PutFile(r.Context(), r.PathValue("session"), file); err != nil {
		return err
	}
	return writeJSON(w, file)
}
func (s *Server) getFile(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	file, err := s.Control.File(r.Context(), r.PathValue("session"), r.PathValue("file"))
	if err != nil {
		return err
	}
	body, err := s.Blobs.Get(r.Context(), "blobs/"+r.PathValue("session")+"/"+file.Digest, int64(file.Size))
	if err != nil {
		return err
	}
	if uint64(len(body)) != file.Size || kernel.Digest(body) != file.Digest {
		return kernel.Fail(wire.ErrorCodeCorrupt, "artifact digest mismatch")
	}
	w.Header().Set("Content-Type", file.MediaType)
	w.Header().Set("ETag", `"`+file.Digest+`"`)
	_, err = w.Write(body)
	return err
}
func (s *Server) deleteFile(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	if err := s.audit(r.Context(), p, r.PathValue("session"), "file.delete_requested", map[string]string{"name": r.PathValue("file")}); err != nil {
		return err
	}
	out, err := s.Control.DeleteFile(r.Context(), r.PathValue("session"), r.PathValue("file"))
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}

func (s *Server) schedules(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	limit, err := queryLimit(r)
	if err != nil {
		return err
	}
	out, err := s.Control.Schedules(r.Context(), r.PathValue("session"), limit, r.URL.Query().Get("after"))
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}
func (s *Server) schedule(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.ScheduleCreate](s, w, r, "ScheduleCreate")
	if err != nil {
		return err
	}
	session, err := s.Control.Session(r.Context(), p.Tenant, r.PathValue("session"))
	if err != nil {
		return err
	}
	if err = s.validateInput(session.View.Config, request.Input, "timer"); err != nil {
		return err
	}
	if len(kernel.JSON(request.Input)) > int(session.View.Config.Limits.InputBytes) ||
		request.DueMs > 1<<53-1 || kernel.Value(request.IntervalMs) > 1<<53-1 {
		return kernel.Fail(wire.ErrorCodeInvalid, "scheduled input or time exceeds its admitted bound")
	}
	if request.Input.Kind == "action" && !p.Allows(session.View.ID, "action:"+kernel.Value(request.Input.Name)) {
		return kernel.Fail(wire.ErrorCodeForbidden, "scheduling requires the action grant")
	}
	if request.Input.Kind == "control" && !p.Allows(session.View.ID, "session:control") {
		return kernel.Fail(wire.ErrorCodeForbidden, "scheduling requires the control grant")
	}
	if err = s.audit(r.Context(), p, session.View.ID, "schedule.requested", request); err != nil {
		return err
	}
	out, err := s.Control.Schedule(r.Context(), session.View.ID, request)
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}
func (s *Server) unschedule(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	if err := s.audit(r.Context(), p, r.PathValue("session"), "schedule.cancel_requested", map[string]string{"id": r.PathValue("schedule")}); err != nil {
		return err
	}
	out, err := s.Control.CancelSchedule(r.Context(), r.PathValue("session"), r.PathValue("schedule"))
	if err != nil {
		return err
	}
	return writeJSON(w, out)
}

func (s *Server) delegate(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.Delegation](s, w, r, "Delegation")
	if err != nil {
		return err
	}
	parent, err := s.Control.Session(r.Context(), p.Tenant, r.PathValue("session"))
	if err != nil {
		return err
	}
	out, err := s.delegation(r.Context(), parent, request)
	if err != nil {
		return err
	}
	return writeJSON(w, out.View)
}
func (s *Server) handoff(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.Handoff](s, w, r, "Handoff")
	if err != nil {
		return err
	}
	out, err := s.Control.Handoff(r.Context(), r.PathValue("session"), request)
	if err != nil {
		return err
	}
	return writeJSON(w, out.View)
}
func (s *Server) identity(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	out := wire.IdentityView{Tenant: p.Tenant, Subject: p.Subject, Permissions: append([]string{}, p.Permissions...), Operator: p.Operator}
	if p.Session != "" {
		out.SessionID = kernel.Ptr(p.Session)
	}
	if p.GrantID != "" {
		out.ParticipantID = kernel.Ptr(p.GrantID)
	}
	if p.Issuer != "" {
		out.Issuer = kernel.Ptr(p.Issuer)
	}
	if p.Expires > 0 {
		out.ExpiresMs = kernel.Ptr(uint64(p.Expires))
	}
	return writeJSON(w, out)
}

func (s *Server) grant(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.ParticipantGrant](s, w, r, "ParticipantGrant")
	if err != nil {
		return err
	}
	if err = kernel.ValidateParticipant(request); err != nil {
		return err
	}
	if !p.Operator && (request.Subject != p.Subject || !kernel.PermissionSubset(request.Permissions, p.Permissions) ||
		p.Expires <= 0 || request.ExpiresMs > uint64(p.Expires)) {
		return kernel.Fail(wire.ErrorCodeForbidden, "delegated token cannot amplify permissions, lifetime or source identity")
	}
	if err = s.audit(r.Context(), p, r.PathValue("session"), "participant.grant_requested", map[string]any{"subject": request.Subject, "permissions": request.Permissions}); err != nil {
		return err
	}
	out, err := s.Control.Grant(r.Context(), r.PathValue("session"), request)
	if err != nil {
		return err
	}
	return writeJSON(w, wire.ParticipantToken{ID: out.ID, Token: "p_" + out.ID + "." + s.sign("participant", out.ID), ExpiresMs: uint64(out.Expires)})
}
func (s *Server) revokeParticipant(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	if err := s.Control.RevokeParticipant(r.Context(), r.PathValue("session"), r.PathValue("participant")); err != nil {
		return err
	}
	return writeJSON(w, "revoked")
}
func (s *Server) storage(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	report, err := s.Control.Report(r.Context())
	if err != nil {
		return err
	}
	cache, dirty, reserved, oldest, err := s.History.Stats(r.Context())
	if err != nil {
		return err
	}
	report.CacheBytes = uint64(cache)
	report.UnarchivedBytes = uint64(dirty)
	report.ArchiveQueueBytes = uint64(dirty)
	report.ReservedOutputBytes = uint64(reserved)
	report.OldestUnarchivedMs = uint64(max(oldest, 0))
	report.Accepting = s.healthy.Load() && !s.draining.Load()
	return writeJSON(w, report)
}

func (s *Server) packageAdmission(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	request, err := readBody[wire.PackageAdmission](s, w, r, "PackageAdmission")
	if err != nil {
		return err
	}
	session, err := s.Control.Session(r.Context(), p.Tenant, request.SessionID)
	if err != nil {
		return err
	}
	if !p.Allows(session.View.ID, "package:write") {
		return kernel.Fail(wire.ErrorCodeForbidden, "package grant is required")
	}
	if err = packages.Validate(request.Manifest); err != nil {
		return err
	}
	action, err := kernel.Action(session.View.Config, "build_package")
	if err != nil {
		return err
	}
	if action.Connection != request.Builder {
		return kernel.Fail(wire.ErrorCodeForbidden, "builder is not the admitted build_package placement")
	}
	digest := kernel.Digest(kernel.JSON(request.Manifest))
	id := kernel.ID("package", p.Tenant, digest)
	pkg, err := s.Control.Package(r.Context(), p.Tenant, id)
	if err != nil && kernel.Response(err).Code != wire.ErrorCodeNotFound {
		return err
	}
	if err != nil {
		pkg = wire.Package{ID: id, Digest: digest, Status: "admitted", Manifest: request.Manifest}
		if err = s.Control.PutPackage(r.Context(), p.Tenant, pkg); err != nil {
			return err
		}
	}
	input := wire.Input{RequestID: "package:" + id, Data: wire.InputData{Kind: "action", Name: kernel.Ptr("build_package"), Arguments: kernel.JSON(map[string]string{"package_id": id})}}
	if err = s.validateInput(session.View.Config, input.Data, p.Subject); err != nil {
		return err
	}
	if _, err = s.Control.Accept(r.Context(), p, session.View.ID, input); err != nil {
		return err
	}
	return writeJSON(w, pkg)
}
func (s *Server) packageGet(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	pkg, err := s.Control.Package(r.Context(), p.Tenant, r.PathValue("package"))
	if err != nil {
		return err
	}
	return writeJSON(w, pkg)
}
