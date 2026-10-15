package sqlcontrol

import (
	"context"
	"database/sql"
	"errors"
	"strconv"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

const commandColumns = "id,session_id,status,accepted_ms,run_id,sequence,error"

func scanCommand(row scanner) (wire.CommandReceipt, error) {
	var out wire.CommandReceipt
	var run, failure string
	var sequence int64
	err := row.Scan(&out.ID, &out.SessionID, &out.Status, &out.AcceptedMs, &run, &sequence, &failure)
	if err != nil {
		return out, notFound(err)
	}
	if run != "" {
		out.RunID = &run
	}
	if sequence > 0 && out.Status == "applied" {
		out.Sequence = kernel.Ptr(strconv.FormatInt(sequence, 10))
	}
	if failure != "" {
		e, err := decode[wire.ErrorResponse](failure)
		if err != nil {
			return out, err
		}
		out.Error = &e
	}
	return out, nil
}

func (s *Store) accept(ctx context.Context, tx *sql.Tx, p kernel.Principal, session string, request wire.Input) (wire.CommandReceipt, error) {
	id := kernel.ID("input", session, request.RequestID)
	digest := kernel.Digest(kernel.JSON(request.Data))
	var old string
	err := tx.QueryRowContext(ctx, s.q("SELECT digest FROM commands WHERE id=? AND session_id=?"), id, session).Scan(&old)
	if err == nil {
		if digest != old {
			return wire.CommandReceipt{}, kernel.Fail(wire.ErrorCodeConflict, "input idempotency key has different content")
		}
		return scanCommand(tx.QueryRowContext(ctx, s.q("SELECT "+commandColumns+" FROM commands WHERE id=?"), id))
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return wire.CommandReceipt{}, err
	}
	current, err := s.session(ctx, tx, session, true)
	if err != nil {
		return wire.CommandReceipt{}, err
	}
	if current.Tenant != p.Tenant || current.View.Lifecycle != wire.LifecycleOpen {
		return wire.CommandReceipt{}, kernel.Fail(wire.ErrorCodeConflict, "session is not open")
	}
	if request.Data.Kind != "control" && request.Data.Kind != "journal" {
		if err = s.liveLineage(ctx, tx, current, false); err != nil {
			return wire.CommandReceipt{}, err
		}
	}
	if len(kernel.JSON(request.Data)) > int(current.View.Config.Limits.InputBytes) {
		return wire.CommandReceipt{}, kernel.Fail(wire.ErrorCodeInvalid, "input exceeds admitted byte limit")
	}
	var count int64
	err = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM commands WHERE session_id=? AND status='accepted'"), session).Scan(&count)
	if err != nil {
		return wire.CommandReceipt{}, err
	}
	if count >= int64(current.View.Config.Limits.PendingInputs) {
		return wire.CommandReceipt{}, kernel.Fail(wire.ErrorCodeOverloaded, "pending input limit reached")
	}
	_, err = tx.ExecContext(ctx, s.q("INSERT INTO commands(id,session_id,request_key,digest,payload,source,kind,accepted_ms) VALUES(?,?,?,?,?,?,?,"+s.clock()+")"),
		id, session, request.RequestID, digest, string(kernel.JSON(request.Data)), p.Subject, request.Data.Kind)
	if err != nil {
		return wire.CommandReceipt{}, err
	}
	if request.Data.Kind == "control" {
		switch kernel.Value(request.Data.Control) {
		case wire.ControlKindPause:
			_, err = tx.ExecContext(ctx, s.q("UPDATE sessions SET paused=1,revision=revision+1 WHERE id=? AND paused=0"), session)
			if err == nil {
				err = s.pauseLineage(ctx, tx, current, true)
			}
		case wire.ControlKindResume:
			_, err = tx.ExecContext(ctx, s.q("UPDATE sessions SET paused=0,revision=revision+1 WHERE id=? AND paused=1"), session)
			if err == nil {
				err = s.pauseLineage(ctx, tx, current, false)
			}
			if err == nil {
				_, err = tx.ExecContext(ctx, s.q("UPDATE runs SET status=CASE WHEN decision='' AND wait_kind<>'' THEN 'waiting' ELSE 'active' END,error='',updated_ms="+s.clock()+" WHERE session_id=? AND status='blocked'"), session)
			}
		case wire.ControlKindCancel, wire.ControlKindClose:
			if err = s.cancelLineage(ctx, tx, current); err != nil {
				return wire.CommandReceipt{}, err
			}
			_, err = tx.ExecContext(ctx, s.q("UPDATE runs SET status='cancelled',updated_ms="+s.clock()+" WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked')"), session)
			if err == nil {
				_, err = tx.ExecContext(ctx, s.q("UPDATE commands SET status='cancelled' WHERE session_id=? AND status='accepted' AND kind IN ('message','action','event')"), session)
			}
			if err == nil {
				_, err = tx.ExecContext(ctx, s.q("UPDATE interactions SET status='cancelled',revision=revision+1 WHERE session_id=? AND status='pending'"), session)
			}
			if err == nil {
				_, err = tx.ExecContext(ctx, s.q(wakeParents), session)
			}
			if err == nil && kernel.Value(request.Data.Control) == wire.ControlKindClose {
				_, err = tx.ExecContext(ctx, s.q("UPDATE sessions SET lifecycle='closed',revision=revision+1 WHERE id=? AND lifecycle='open'"), session)
				if err == nil {
					_, err = tx.ExecContext(ctx, s.q("UPDATE schedules SET status='cancelled' WHERE session_id=? AND status='active'"), session)
				}
			}
		default:
			return wire.CommandReceipt{}, kernel.Fail(wire.ErrorCodeInvalid, "unknown control")
		}
		if err != nil {
			return wire.CommandReceipt{}, err
		}
	}
	if _, err = tx.ExecContext(ctx, s.q("UPDATE sessions SET ready=1,ready_ms=CASE WHEN ready=0 THEN "+s.clock()+" ELSE ready_ms END WHERE id=?"), session); err != nil {
		return wire.CommandReceipt{}, err
	}
	return scanCommand(tx.QueryRowContext(ctx, s.q("SELECT "+commandColumns+" FROM commands WHERE id=?"), id))
}

func (s *Store) Accept(ctx context.Context, p kernel.Principal, session string, request wire.Input) (out wire.CommandReceipt, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		var e error
		out, e = s.accept(ctx, tx, p, session, request)
		return e
	})
	return
}

func (s *Store) Command(ctx context.Context, session, id string) (wire.CommandReceipt, error) {
	return scanCommand(s.db.QueryRowContext(ctx, s.q("SELECT "+commandColumns+" FROM commands WHERE session_id=? AND id=?"), session, id))
}

func (s *Store) NextInput(ctx context.Context, session string, owner control.Owner) (input wire.Input, id, source string, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, false); err != nil {
			return err
		}
		if err := s.unfrozen(ctx, tx, session, false); err != nil {
			return err
		}
		var payload string
		err := tx.QueryRowContext(ctx, s.q("SELECT id,request_key,payload,source FROM commands WHERE session_id=? AND status='accepted' ORDER BY accepted_ms,id LIMIT 1"), session).
			Scan(&id, &input.RequestID, &payload, &source)
		if err != nil {
			return notFound(err)
		}
		input.Data, err = decode[wire.InputData](payload)
		return err
	})
	return
}

func (s *Store) ApplyInput(ctx context.Context, session string, apply control.ApplyInput) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		current, err := s.guard(ctx, tx, session, apply.Owner, false)
		if err != nil {
			return err
		}
		var payload, status string
		err = tx.QueryRowContext(ctx, s.q("SELECT payload,status FROM commands WHERE id=? AND session_id=?"), apply.Command, session).Scan(&payload, &status)
		if err != nil || status != "accepted" {
			return err
		}
		input, err := decode[wire.InputData](payload)
		if err != nil {
			return err
		}
		if apply.Disposition == wire.DispositionState {
			if err = s.documentQuota(ctx, tx, session, "state", apply.StateKey, input.Data); err != nil {
				code := kernel.Response(err).Code
				if code != wire.ErrorCodeInvalid && code != wire.ErrorCodeOverloaded {
					return err
				}
				if err = requireOne(tx.ExecContext(ctx, s.q("UPDATE commands SET status='failed',sequence=?,error=? WHERE id=? AND status='accepted'"),
					apply.Sequence, string(kernel.JSON(kernel.Response(err))), apply.Command)); err != nil {
					return err
				}
				_, err = tx.ExecContext(ctx, s.q(wakeParents), session)
				return err
			}
			_, err = tx.ExecContext(ctx, s.q(`INSERT INTO documents(session_id,scope,key,revision,value,request_key,request_hash)
VALUES(?,'state',?,1,?,?,?) ON CONFLICT(session_id,scope,key) DO UPDATE SET revision=documents.revision+1,value=excluded.value,request_key=excluded.request_key,request_hash=excluded.request_hash`),
				session, apply.StateKey, string(input.Data), apply.Command, kernel.Digest(input.Data))
			if err != nil {
				return err
			}
		}
		if apply.Disposition == wire.DispositionInterrupt {
			if err = s.cancelLineage(ctx, tx, current); err != nil {
				return err
			}
			if _, err := tx.ExecContext(ctx, s.q("UPDATE runs SET status='cancelled' WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked')"), session); err != nil {
				return err
			}
			if _, err := tx.ExecContext(ctx, s.q(wakeParents), session); err != nil {
				return err
			}
		}
		runID := ""
		if apply.Run {
			runID = kernel.ID("run", session, apply.Command)
			_, err = tx.ExecContext(ctx, s.q("INSERT INTO runs(id,session_id,command_id,status,updated_ms) VALUES(?,?,?,'queued',"+s.clock()+") ON CONFLICT DO NOTHING"), runID, session, apply.Command)
			if err != nil {
				return err
			}
		}
		if apply.Disposition == wire.DispositionSteer || apply.Disposition == wire.DispositionWake {
			_, err = tx.ExecContext(ctx, s.q("UPDATE runs SET status='active',wait_kind='',wait_id='' WHERE session_id=? AND status='waiting' AND wait_kind='input'"), session)
			if err != nil {
				return err
			}
		}
		if current.View.Lifecycle == wire.LifecycleDeleted {
			return kernel.Fail(wire.ErrorCodeConflict, "session is deleted")
		}
		return requireOne(tx.ExecContext(ctx, s.q("UPDATE commands SET status='applied',sequence=?,run_id=? WHERE id=? AND status='accepted'"),
			apply.Sequence, runID, apply.Command))
	})
}

const runColumns = "r.id,r.session_id,r.command_id,r.status,r.step,r.checkpoint,r.decision,r.wait_kind,r.wait_id,r.error,c.payload,c.sequence,r.completion_sequence,r.completion_digest,r.harness_process"

func scanRun(row scanner) (control.Run, error) {
	var out control.Run
	var status, checkpoint, decision, waitKind, waitID, failure, payload string
	var completionSequence int64
	var completionDigest string
	err := row.Scan(&out.View.ID, &out.View.SessionID, &out.View.CommandID, &status, &out.View.Step,
		&checkpoint, &decision, &waitKind, &waitID, &failure, &payload, &out.InputSequence, &completionSequence, &completionDigest, &out.HarnessProcess)
	if err != nil {
		return out, notFound(err)
	}
	out.View.Status = wire.RunStatus(status)
	out.Checkpoint = []byte(checkpoint)
	out.Input, err = decode[wire.InputData](payload)
	if err != nil {
		return out, err
	}
	if decision != "" {
		d, err := decode[wire.Decision](decision)
		if err != nil {
			return out, err
		}
		out.Decision = &d
	}
	if waitKind != "" {
		out.View.Wait = &wire.Wait{Kind: waitKind, ID: waitID}
	}
	if failure != "" {
		f, err := decode[wire.ErrorResponse](failure)
		if err != nil {
			return out, err
		}
		out.View.Error = &f
	}
	if completionSequence > 0 {
		out.View.Completion = &wire.RecordReference{SessionID: out.View.SessionID, Sequence: strconv.FormatInt(completionSequence, 10), Digest: completionDigest}
	}
	return out, nil
}

func (s *Store) Runs(ctx context.Context, session string, limit int, after ...string) ([]control.Run, error) {
	rows, err := s.pageRows(ctx, runColumns, "runs r JOIN commands c ON c.id=r.command_id", "r.session_id=? AND r.id>?", "r.id", s.runBytes(), limit, session, control.After(after))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []control.Run{}
	for rows.Next() {
		run, err := scanRun(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, run)
	}
	return out, rows.Err()
}

func (s *Store) SaveDecision(ctx context.Context, session string, owner control.Owner, run string, step uint64, decision wire.Decision, process ...string) error {
	identity := control.After(process)
	if identity != "" && !kernel.Identifier(identity) {
		return kernel.Fail(wire.ErrorCodeInvalid, "harness process identity is invalid")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, true); err != nil {
			return err
		}
		body := string(kernel.JSON(decision))
		if len(body) > 262144 {
			return kernel.Fail(wire.ErrorCodeInvalid, "harness decision exceeds the durable checkpoint bound")
		}
		if err := requireOne(tx.ExecContext(ctx, s.q("UPDATE runs SET decision=?,status='active',harness_process=CASE WHEN ?='' THEN harness_process ELSE ? END,updated_ms="+s.clock()+" WHERE id=? AND session_id=? AND step=? AND status IN ('queued','active','waiting') AND (decision='' OR decision=?) AND (?='' OR (harness_process=? AND decision_started_step=?))"),
			body, identity, identity, run, session, step, body, identity, identity, step)); err != nil {
			return err
		}
		if decision.Kind == "delegate" || decision.Kind == "fork" {
			children, err := kernel.Delegations(decision)
			if err != nil {
				return err
			}
			_, err = tx.ExecContext(ctx, s.q("INSERT INTO child_groups VALUES(?,?,?,?) ON CONFLICT DO NOTHING"), session, run, step, len(children))
			return err
		}
		return nil
	})
}

func (s *Store) WaitDispatch(ctx context.Context, session string, owner control.Owner, run string, step uint64, operation string) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, false); err != nil {
			return err
		}
		return requireOne(tx.ExecContext(ctx, s.q(`UPDATE runs SET status='waiting',wait_kind='dispatch',wait_id=?,updated_ms=`+s.clock()+`
WHERE id=? AND session_id=? AND step=? AND decision<>'' AND status IN ('queued','active','waiting')
AND EXISTS(SELECT 1 FROM operations o WHERE o.id=? AND o.session_id=runs.session_id AND o.run_id=runs.id AND o.step_key=CAST(runs.step AS TEXT))`),
			operation, run, session, step, operation))
	})
}

func (s *Store) AdvanceRun(ctx context.Context, session string, owner control.Owner, run string, step uint64, status wire.RunStatus, wait *wire.Wait, failure *wire.ErrorResponse) error {
	if status == wire.RunStatusCompleted {
		return kernel.Fail(wire.ErrorCodeInvalid, "completion requires written matching evidence")
	}
	if step >= 4294967295 {
		return kernel.Fail(wire.ErrorCodeOverloaded, "run decision step limit reached")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, false); err != nil {
			return err
		}
		if status == wire.RunStatusBlocked {
			message := ""
			if failure != nil {
				message = string(kernel.JSON(failure))
			}
			if err := requireOne(tx.ExecContext(ctx, s.q("UPDATE runs SET status='blocked',error=?,updated_ms="+s.clock()+" WHERE id=? AND session_id=? AND step=? AND status IN ('queued','active','waiting','paused')"), message, run, session, step)); err != nil {
				return err
			}
			_, err := tx.ExecContext(ctx, s.q(wakeParents), session)
			return err
		}
		var decision, checkpoint string
		if err := tx.QueryRowContext(ctx, s.q("SELECT decision,checkpoint FROM runs WHERE id=? AND session_id=?"), run, session).Scan(&decision, &checkpoint); err != nil {
			return notFound(err)
		}
		if decision != "" {
			d, err := decode[wire.Decision](decision)
			if err != nil {
				return err
			}
			checkpoint = string(d.Checkpoint)
		}
		if wait != nil && wait.Kind == "children" {
			d, err := decode[wire.Decision](decision)
			if err != nil {
				return err
			}
			children, err := kernel.Delegations(d)
			if err != nil || wait.ID != strconv.FormatUint(step, 10) {
				return kernel.Fail(wire.ErrorCodeConflict, "child wait differs from its saved decision")
			}
			var count int
			if err = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM run_children WHERE session_id=? AND run_id=? AND step=?"),
				session, run, step).Scan(&count); err != nil {
				return err
			}
			if count != len(children) {
				return kernel.Fail(wire.ErrorCodeConflict, "not every saved child has been admitted")
			}
		}
		kind, id, message := "", "", ""
		if wait != nil {
			kind, id = wait.Kind, wait.ID
		}
		if failure != nil {
			message = string(kernel.JSON(failure))
		}
		if err := requireOne(tx.ExecContext(ctx, s.q("UPDATE runs SET status=?,step=step+1,checkpoint=?,decision='',wait_kind=?,wait_id=?,error=?,updated_ms="+s.clock()+" WHERE id=? AND session_id=? AND step=? AND status NOT IN ('completed','failed','cancelled','blocked')"),
			status, checkpoint, kind, id, message, run, session, step)); err != nil {
			return err
		}
		if kernel.Terminal(status) {
			_, err := tx.ExecContext(ctx, s.q(wakeParents), session)
			return err
		}
		return nil
	})
}
