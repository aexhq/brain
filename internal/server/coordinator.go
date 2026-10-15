package server

import (
	"context"
	"encoding/json"
	"log/slog"
	"strconv"
	"time"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/aexhq/brain/internal/validation"
)

func (s *Server) coordinate(ctx context.Context) {
	defer s.wg.Done()
	s.wg.Add(1)
	go s.coordinateTransfers(ctx)
	work := make(chan string, s.Config.Workers)
	for i := 0; i < s.Config.Workers; i++ {
		s.wg.Add(1)
		go func() {
			defer s.wg.Done()
			for {
				select {
				case <-ctx.Done():
					return
				case session := <-work:
					if err := s.process(ctx, session); err != nil && ctx.Err() == nil {
						code := kernel.Response(err).Code
						if code != wire.ErrorCodeConflict {
							slog.Warn("session_step_failed", "session", session, "code", code)
						}
					}
					s.busy.Delete(session)
				}
			}
		}()
	}
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		ticker := time.NewTicker(s.Config.Lease / 3)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				leaseCtx, cancel := context.WithTimeout(ctx, s.Config.Lease/4)
				err := s.Control.RenewNode(leaseCtx, s.Config.Node, s.Config.Lease.Milliseconds())
				cancel()
				if err != nil {
					s.healthy.Store(false)
					s.draining.Store(true)
					slog.Error("node_lease_lost")
					s.cancel()
					return
				}
			}
		}
	}()
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		ticker := time.NewTicker(s.Config.ArchiveInterval)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-ticker.C:
				maintenance, cancel := context.WithTimeout(ctx, 10*time.Second)
				if err := s.History.Maintain(maintenance); err != nil && ctx.Err() == nil {
					slog.Warn("archive_delayed", "code", kernel.Response(err).Code)
				}
				cancel()
			}
		}
	}()
	ticker := time.NewTicker(50 * time.Millisecond)
	defer ticker.Stop()
	timers := time.NewTicker(500 * time.Millisecond)
	defer timers.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-timers.C:
			closed, err := s.Control.PropagateControls(ctx, 4)
			if err != nil && ctx.Err() == nil {
				slog.Warn("control_propagation_delayed", "code", kernel.Response(err).Code)
			}
			for _, identifier := range closed {
				s.cancelSession(identifier)
			}
			if _, err := s.Control.FireTimers(ctx, 64); err != nil && ctx.Err() == nil {
				slog.Warn("timer_admission_delayed", "code", kernel.Response(err).Code)
			}
			if _, err := s.Control.ExpireOperations(ctx, 128); err != nil && ctx.Err() == nil {
				slog.Warn("operation_expiry_delayed", "code", kernel.Response(err).Code)
			}
		case <-ticker.C:
			if !s.healthy.Load() {
				continue
			}
			sessions, err := s.Control.Pending(ctx, s.Config.Node, s.Config.Workers*2)
			if err != nil {
				slog.Warn("control_unavailable", "code", kernel.Response(err).Code)
				continue
			}
			for _, session := range sessions {
				if _, loaded := s.busy.LoadOrStore(session, true); loaded {
					continue
				}
				select {
				case work <- session:
				default:
					s.busy.Delete(session)
				}
			}
		}
	}
}

func (s *Server) process(ctx context.Context, id string) error {
	session, err := s.owned(ctx, "", id)
	if err != nil {
		return err
	}
	defer func() {
		finish, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer cancel()
		if err := s.Control.FinishStep(finish, id, session.Owner); err != nil && ctx.Err() == nil {
			slog.Warn("readiness_reconciliation_delayed", "code", kernel.Response(err).Code)
		}
	}()
	if err = s.bindResults(ctx, session); err != nil {
		return err
	}
	for i := 0; i < 16; i++ {
		input, command, source, err := s.Control.NextInput(ctx, id, session.Owner)
		if kernel.Response(err).Code == wire.ErrorCodeNotFound {
			break
		}
		if err != nil {
			return err
		}
		kind := "input.accepted"
		var data any = map[string]any{"command_id": command, "input": input.Data, "source": source}
		if input.Data.Kind == "journal" {
			kind = kernel.Value(input.Data.Name)
			data = json.RawMessage(input.Data.Data)
		}
		record, err := s.append(ctx, session, kind, data, "", "", false)
		if err != nil {
			if kernel.Response(err).Code == wire.ErrorCodeOverloaded {
				break
			}
			return err
		}
		sequence, _ := kernel.Sequence(record.Sequence)
		apply := control.ApplyInput{Command: command, Owner: session.Owner, Sequence: sequence, Run: input.Data.Kind == "message" || input.Data.Kind == "action"}
		if input.Data.Kind == "event" {
			route, err := kernel.Route(session.View.Config, kernel.Value(input.Data.Name), kernel.Value(input.Data.Version), source)
			if err != nil {
				return err
			}
			apply.Disposition = route.Disposition
			apply.StateKey = route.Name
			apply.Run = route.Action != nil || route.Disposition == wire.DispositionWake
		}
		if err = s.Control.ApplyInput(ctx, id, apply); err != nil {
			return err
		}
		if apply.Disposition == wire.DispositionInterrupt {
			s.cancelSession(id)
		}
	}
	session, err = s.Control.Session(ctx, "", id)
	if err != nil {
		return err
	}
	if err = s.bindResults(ctx, session); err != nil {
		return err
	}
	if session.View.Paused || session.View.Lifecycle != wire.LifecycleOpen {
		return nil
	}
	runs, err := s.Control.RunnableRuns(ctx, id, 64)
	if err != nil {
		return err
	}
	for _, run := range runs {
		if kernel.Terminal(run.View.Status) {
			continue
		}
		if err = s.step(ctx, session, run); err != nil {
			code := kernel.Response(err).Code
			if code == wire.ErrorCodeConflict || code == wire.ErrorCodeOverloaded {
				return err
			}
			failure := kernel.Response(err)
			_, _ = s.append(ctx, session, "run.blocked", failure, run.View.ID, "", false)
			_ = s.Control.AdvanceRun(ctx, id, session.Owner, run.View.ID, run.View.Step, wire.RunStatusBlocked, nil, &failure)
			return err
		}
	}
	return nil
}

func (s *Server) available(ctx context.Context, operation control.Operation) control.Operation {
	if operation.View.ResultAvailable || operation.ResultSequence == 0 {
		return operation
	}
	record, err := s.History.Get(ctx, operation.View.SessionID, operation.ResultSequence, operation.ResultDigest)
	if err != nil {
		return operation
	}
	var receipt wire.Receipt
	if json.Unmarshal(record.Data, &receipt) == nil {
		operation.View.Result = receipt.Result
		operation.View.Usage = receipt.Usage
		operation.View.ResultAvailable = receipt.Error == nil || len(receipt.Result) > 0 || receipt.Outcome != wire.OutcomeCompleted
	}
	return operation
}

func (s *Server) step(ctx context.Context, session control.Session, run control.Run) error {
	if err := s.Harness.Resume(ctx, session.View.Config, run.View.ID, run.HarnessProcess); err != nil {
		return err
	}
	var outcome *wire.Operation
	var children []wire.ChildResult
	if run.View.Wait != nil {
		wait := run.View.Wait
		switch wait.Kind {
		case "dispatch":
			// An approval wait retains the original saved call, not its result.
			if run.Decision == nil || run.Decision.Kind != "call" {
				return kernel.Fail(wire.ErrorCodeCorrupt, "dispatch wait has no original saved effect decision")
			}
		case "operation":
			operation, err := s.Control.Operation(ctx, session.View.ID, wait.ID)
			if err != nil {
				return err
			}
			if operation.View.Status == wire.OperationStatusPrepared || operation.View.Status == wire.OperationStatusDispatched {
				return nil
			}
			if operation.View.Status == wire.OperationStatusUnknown {
				return kernel.Fail(wire.ErrorCodeUncertain, "operation outcome is uncertain; explicit reconciliation or a new attempt is required")
			}
			operation = s.available(ctx, operation)
			if !operation.View.ResultAvailable {
				return kernel.Fail(wire.ErrorCodeUnavailable, "known operation outcome has unavailable response bytes; it will not be sent again")
			}
			if operation.View.Status == wire.OperationStatusCompleted && operation.View.Error != nil {
				return kernel.Fail(wire.ErrorCodeInvalid, "completed operation has evidence that cannot satisfy its declared result contract")
			}
			if operation.ResultSequence == 0 {
				return nil
			}
			outcome = &operation.View
		case "interaction":
			interaction, err := s.Control.Interaction(ctx, session.View.ID, wait.ID)
			if err != nil {
				return err
			}
			now, err := s.Control.Clock(ctx)
			if err != nil {
				return err
			}
			if interaction.Status == wire.InteractionStatusPending {
				if interaction.Request.ExpiresMs <= uint64(now) {
					return kernel.Fail(wire.ErrorCodeConflict, "interaction expired")
				}
				return nil
			}
			if interaction.Status != wire.InteractionStatusAnswered {
				return kernel.Fail(wire.ErrorCodeForbidden, "interaction was rejected or cancelled")
			}
			outcome = &wire.Operation{ID: interaction.ID, SessionID: session.View.ID, RunID: run.View.ID, Status: wire.OperationStatusCompleted, ResultAvailable: true, Result: interaction.Answer}
		case "input":
			return nil
		case "child":
			return kernel.Fail(wire.ErrorCodeUnsupported, "child waits require a saved delegation decision")
		case "children":
			step, err := strconv.ParseUint(wait.ID, 10, 32)
			if err != nil {
				return err
			}
			results, err := s.childResults(ctx, session.View.ID, run.View.ID, step)
			if err != nil {
				return err
			}
			if !results.Ready {
				return nil
			}
			children = results.Children
		default:
			return kernel.Fail(wire.ErrorCodeUnsupported, "unsupported wait kind")
		}
	}
	decision := run.Decision
	if decision == nil {
		var d wire.Decision
		process := run.HarnessProcess
		direct := run.Input.Kind == "action"
		if run.Input.Kind == "event" {
			for _, route := range session.View.Config.Events {
				if route.Name == kernel.Value(run.Input.Name) && route.Version == kernel.Value(run.Input.Version) && route.Action != nil {
					direct = true
					run.Input = wire.InputData{Kind: "action", Name: route.Action, Arguments: run.Input.Data}
				}
			}
		}
		if direct {
			if outcome == nil {
				d = wire.Decision{Kind: "call", Effect: kernel.Ptr(wire.EffectKindAction), Action: run.Input.Name, Arguments: run.Input.Arguments, Checkpoint: kernel.JSON(map[string]bool{"direct": true})}
			} else {
				if outcome.Status != wire.OperationStatusCompleted {
					return kernel.Fail(wire.ErrorCodeUnavailable, "application action failed")
				}
				d = wire.Decision{Kind: "complete", Result: outcome.Result, Checkpoint: run.Checkpoint}
			}
		} else {
			contextRecords, err := s.History.Context(ctx, session.View.ID, run.InputSequence)
			if err != nil {
				return err
			}
			request := wire.HarnessRequest{Protocol: "brain-decision/1", SessionID: session.View.ID, RunID: run.View.ID, Step: run.View.Step,
				CommandID: run.View.CommandID, Config: session.View.Config, Input: run.Input, Context: contextRecords, Checkpoint: run.Checkpoint, Outcome: outcome, Children: children}
			d, process, err = s.Harness.Decide(ctx, request, process, func(identity string, repeatable bool) error {
				return s.Control.BeginDecision(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step,
					kernel.Digest(kernel.JSON(request)), identity, repeatable)
			})
			if err != nil {
				return err
			}
		}
		if err := s.Control.SaveDecision(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, d, process); err != nil {
			return err
		}
		decision = &d
	}
	switch decision.Kind {
	case "call":
		return s.call(ctx, session, run, *decision)
	case "complete":
		record, err := s.append(ctx, session, "run.completed", map[string]any{"result": json.RawMessage(decision.Result)}, run.View.ID, "", false)
		if err != nil {
			return err
		}
		return s.Control.CompleteRun(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, record)
	case "emit":
		name := kernel.Value(decision.Name)
		source := "harness:" + session.View.Config.Harness.Name
		route, err := kernel.Route(session.View.Config, name, kernel.Value(decision.Version), source)
		if err != nil {
			return err
		}
		if err = validation.Validate(route.Schema, decision.Data); err != nil {
			return err
		}
		if _, err := s.append(ctx, session, "event.emitted", map[string]any{"name": name, "version": route.Version, "data": json.RawMessage(decision.Data), "source": source}, run.View.ID, "", false); err != nil {
			return err
		}
		return s.Control.AdvanceRun(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, wire.RunStatusActive, nil, nil)
	case "wait":
		return s.Control.AdvanceRun(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, wire.RunStatusWaiting, decision.Wait, nil)
	case "interact":
		if decision.Interaction == nil {
			return kernel.Fail(wire.ErrorCodeInvalid, "interaction decision has no request")
		}
		request := *decision.Interaction
		request.RunID = run.View.ID
		if _, err := validation.Schema(request.Schema); err != nil {
			return err
		}
		interaction, err := s.Control.Interact(ctx, session.View.ID, request)
		if err != nil {
			return err
		}
		if _, err = s.append(ctx, session, "interaction.requested", interaction, run.View.ID, "", false); err != nil {
			return err
		}
		return s.Control.AdvanceRun(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, wire.RunStatusWaiting, &wire.Wait{Kind: "interaction", ID: interaction.ID}, nil)
	case "delegate", "fork":
		requests, err := kernel.Delegations(*decision)
		if err != nil {
			return err
		}
		prepared := make([]wire.SessionCreate, len(requests))
		requirements := make([][]string, len(requests))
		for index, request := range requests {
			prepared[index], requirements[index], err = s.prepareDelegation(ctx, session, request)
			if err != nil {
				return err
			}
		}
		for index, request := range prepared {
			if _, err = s.Control.Delegate(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, uint64(index), request, requirements[index]); err != nil {
				return err
			}
		}
		return s.Control.AdvanceRun(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, wire.RunStatusWaiting,
			&wire.Wait{Kind: "children", ID: strconv.FormatUint(run.View.Step, 10)}, nil)
	default:
		return kernel.Fail(wire.ErrorCodeInvalid, "unsupported harness decision")
	}
}

func (s *Server) call(ctx context.Context, session control.Session, run control.Run, decision wire.Decision) error {
	step := strconv.FormatUint(run.View.Step, 10)
	id := kernel.ID("operation", session.View.ID, run.View.ID, step)
	operation, err := s.Control.Operation(ctx, session.View.ID, id)
	var template wire.ConnectionTemplate
	if kernel.Response(err).Code == wire.ErrorCodeNotFound {
		request := control.Prepare{ID: id, Session: session.View.ID, Run: run.View.ID, StepKey: step,
			Kind: kernel.Value(decision.Effect), Action: kernel.Value(decision.Action), Arguments: decision.Arguments,
			OutputLimit: int64(min(session.View.Config.Limits.OutputBytes, 196608)), Timeout: 300000, Recipient: "model"}
		if request.Kind == wire.EffectKindAction {
			action, err := kernel.Action(session.View.Config, request.Action)
			if err != nil {
				return err
			}
			if err = validation.Validate(action.InputSchema, request.Arguments); err != nil {
				return err
			}
			request.OutputLimit = int64(min(action.MaxOutputBytes, 196608))
			request.Timeout = int64(action.TimeoutMs)
			request.Approval = action.Approval
			connection, err := s.Control.ActiveConnection(ctx, session.View.ID, action.Connection)
			if err != nil {
				if kernel.Response(err).Code == wire.ErrorCodeNotFound {
					return kernel.Fail(wire.ErrorCodeUnavailable, "action connection is not attached")
				}
				return err
			}
			for _, candidate := range session.View.Config.Connections {
				if candidate.Name == action.Connection {
					template = candidate
				}
			}
			request.Recipient = connection.ID
			request.Generation = connection.Generation
			request.RetryOf = kernel.Value(run.Input.RetryOf)
		} else if request.Kind != wire.EffectKindModel {
			return kernel.Fail(wire.ErrorCodeUnsupported, "harness effect kind is not an admitted model or action")
		}
		reserve := kernel.OutputReservation(request.OutputLimit)
		if request.Kind == wire.EffectKindModel {
			request.TokenReserve, err = kernel.ModelTokenReservation(session.View.Config, request.Arguments)
			if err != nil {
				return err
			}
		}
		budget, err := s.Control.Budget(ctx, session.View.ID)
		if err != nil {
			return err
		}
		if err = kernel.AdmitBudget(budget, reserve, request.TokenReserve); err != nil {
			return err
		}
		if err = s.History.Reserve(ctx, session.View.ID, id, reserve); err != nil {
			return err
		}
		intent, err := s.append(ctx, session, "operation.intent", map[string]any{"id": id, "kind": request.Kind, "action": request.Action, "arguments": json.RawMessage(request.Arguments), "digest": kernel.Digest(request.Arguments)}, run.View.ID, id, false)
		if err != nil {
			return err
		}
		request.IntentSequence, _ = kernel.Sequence(intent.Sequence)
		request.IntentDigest = kernel.Digest(kernel.JSON(intent))
		operation, err = s.Control.Prepare(ctx, session.Owner, request)
		if err != nil {
			if kernel.Response(err).Code != wire.ErrorCodeUncertain {
				_ = s.History.Release(ctx, session.View.ID, id)
			}
			return err
		}
		if request.Approval {
			interaction, err := s.Control.Interact(ctx, session.View.ID, wire.InteractionCreate{
				RequestID: "approval:" + id, Kind: wire.InteractionKindApproval, RunID: run.View.ID, Subject: id, SubjectDigest: operation.View.ArgumentsDigest,
				Prompt: "Approve " + request.Action + " with the recorded arguments?", Schema: kernel.JSON(map[string]string{"type": "boolean"}), ExpiresMs: uint64(operation.Deadline),
			})
			if err != nil {
				return err
			}
			if _, err = s.append(ctx, session, "interaction.requested", interaction, run.View.ID, id, false); err != nil {
				return err
			}
		}
	} else if err != nil {
		return err
	}
	if operation.View.Kind == wire.EffectKindAction && template.Name == "" {
		action, err := kernel.Action(session.View.Config, operation.View.Action)
		if err != nil {
			return err
		}
		for _, candidate := range session.View.Config.Connections {
			if candidate.Name == action.Connection {
				template = candidate
			}
		}
	}
	if operation.View.Status == wire.OperationStatusPrepared {
		if template.Kind == wire.ConnectionKindApplication {
			return s.Control.AdvanceRun(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, wire.RunStatusWaiting, &wire.Wait{Kind: "operation", ID: id}, nil)
		}
		select {
		case s.effectSlots <- struct{}{}:
		default:
			return kernel.Fail(wire.ErrorCodeOverloaded, "node in-flight effect capacity is exhausted")
		}
		operation, err = s.Control.ClaimDispatch(ctx, session.View.ID, session.Owner, id, s.Config.Node, "dispatch:"+id)
		if err != nil {
			<-s.effectSlots
			if kernel.Response(err).Code == wire.ErrorCodeForbidden {
				return s.Control.WaitDispatch(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, id)
			}
			return err
		}
		executionCtx, cancel := context.WithDeadline(ctx, time.UnixMilli(operation.Deadline))
		s.activeMu.Lock()
		if s.active[session.View.ID] == nil {
			s.active[session.View.ID] = map[string]context.CancelFunc{}
		}
		s.active[session.View.ID][id] = cancel
		s.activeMu.Unlock()
		s.wg.Add(1)
		go s.execute(executionCtx, cancel, session, operation, template)
	} else if operation.View.Status == wire.OperationStatusDispatched {
		// A persisted claim is not a recoverable permission to send. Even when the
		// predecessor never reached the socket, only explicit retry can try again.
		if operation.Claimant != s.Config.Node {
			_, err = s.Control.RecordReceipt(ctx, session.View.ID, id, wire.Receipt{RequestID: "uncertain:" + id, Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("dispatch owner was lost; original recipient may still complete")}, 0, "")
			if err != nil {
				return err
			}
		}
	}
	return s.Control.AdvanceRun(ctx, session.View.ID, session.Owner, run.View.ID, run.View.Step, wire.RunStatusWaiting, &wire.Wait{Kind: "operation", ID: id}, nil)
}

func (s *Server) execute(ctx context.Context, cancel context.CancelFunc, session control.Session, operation control.Operation, template wire.ConnectionTemplate) {
	defer s.wg.Done()
	defer func() { <-s.effectSlots }()
	defer cancel()
	id := operation.View.ID
	defer func() {
		s.activeMu.Lock()
		defer s.activeMu.Unlock()
		delete(s.active[session.View.ID], id)
		if len(s.active[session.View.ID]) == 0 {
			delete(s.active, session.View.ID)
		}
	}()
	var receipt wire.Receipt
	var err error
	if operation.View.Kind == wire.EffectKindModel {
		var request wire.ModelRequest
		if err = json.Unmarshal(operation.Arguments, &request); err == nil {
			receipt, err = s.Effects.Model(ctx, session.Tenant, session.View.Config.Model, request, operation.OutputLimit)
		}
	} else {
		execution := wire.Execution{Operation: operation.View, Arguments: operation.Arguments, ReceiptToken: s.receiptToken(operation), MaxOutputBytes: uint64(operation.OutputLimit), DeadlineMs: uint64(operation.Deadline)}
		receipt, err = s.Effects.Action(ctx, session.Tenant, template, execution)
		if err == nil && receipt.Outcome == wire.OutcomeCompleted {
			if action, e := kernel.Action(session.View.Config, operation.View.Action); e == nil && len(action.OutputSchema) > 0 {
				if validation.Validate(action.OutputSchema, receipt.Result) != nil {
					receipt.Error = kernel.Ptr("completed action returned bytes outside its declared output schema")
				}
			}
		}
	}
	if err != nil {
		receipt = wire.Receipt{Outcome: wire.OutcomeUnknown, Error: kernel.Ptr("dispatch returned no authoritative outcome")}
	}
	receipt.RequestID = "result:" + id
	persist, persistCancel := context.WithTimeout(context.WithoutCancel(ctx), 10*time.Second)
	defer persistCancel()
	if _, err = s.Control.RecordReceipt(persist, session.View.ID, id, receipt, 0, ""); err != nil {
		slog.Error("receipt_persistence_uncertain", "operation", id, "code", kernel.Response(err).Code)
	}
}

func (s *Server) cancelSession(session string) {
	s.activeMu.Lock()
	defer s.activeMu.Unlock()
	for _, cancel := range s.active[session] {
		cancel()
	}
}

func (s *Server) delegation(ctx context.Context, parent control.Session, request wire.Delegation) (control.Session, error) {
	create, requirements, err := s.prepareDelegation(ctx, parent, request)
	if err != nil {
		return control.Session{}, err
	}
	return s.Control.Create(ctx, parent.Tenant, create, parent.View.ID, parent.Budget, requirements)
}

func (s *Server) prepareDelegation(ctx context.Context, parent control.Session, request wire.Delegation) (wire.SessionCreate, []string, error) {
	if err := kernel.ValidateDelegation(parent.View.Config, request.Config); err != nil {
		return wire.SessionCreate{}, nil, err
	}
	if len(request.Context) > 64 {
		return wire.SessionCreate{}, nil, kernel.Fail(wire.ErrorCodeInvalid, "delegated context is limited to 64 record references")
	}
	if err := s.validateConfig(ctx, parent.Tenant, request.Config); err != nil {
		return wire.SessionCreate{}, nil, err
	}
	for _, reference := range request.Context {
		sequence, err := kernel.Sequence(reference)
		if err != nil || sequence == 0 {
			if err == nil {
				err = kernel.Fail(wire.ErrorCodeInvalid, "delegated context positions start at one")
			}
			return wire.SessionCreate{}, nil, err
		}
		page, err := s.History.Page(ctx, parent.View.ID, sequence-1, 1)
		if err != nil {
			return wire.SessionCreate{}, nil, err
		}
		if len(page.Gaps) != 0 || len(page.Records) != 1 {
			return wire.SessionCreate{}, nil, kernel.Fail(wire.ErrorCodeUnavailable, "delegated context is unavailable")
		}
		request.Message += "\nUntrusted shared observation:\n" + string(kernel.JSON(page.Records[0]))
		if len(request.Message) > int(request.Config.Limits.InputBytes) {
			return wire.SessionCreate{}, nil, kernel.Fail(wire.ErrorCodeInvalid, "delegated context exceeds child input limit")
		}
	}
	create := wire.SessionCreate{RequestID: kernel.ID("child", parent.View.ID, request.RequestID), Config: request.Config, Message: &request.Message}
	requirements := s.requirements(parent.Tenant, request.Config)
	return create, requirements, nil
}
