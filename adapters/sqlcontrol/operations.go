package sqlcontrol

import (
	"context"
	"database/sql"
	"strconv"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

const operationColumns = `id,session_id,run_id,step_key,kind,action,arguments_digest,status,recipient,revision,
arguments,output_limit,deadline_ms,generation,result_sequence,result_digest,result_body,usage,claimant,retry_of,error,intent_sequence,intent_digest,token_reserve`

func scanOperation(row scanner) (control.Operation, error) {
	var out control.Operation
	var kind, status, arguments, body, usage, retry, failure string
	var revision int64
	err := row.Scan(&out.View.ID, &out.View.SessionID, &out.View.RunID, &out.View.StepKey,
		&kind, &out.View.Action, &out.View.ArgumentsDigest, &status, &out.View.Recipient, &revision,
		&arguments, &out.OutputLimit, &out.Deadline, &out.Generation, &out.ResultSequence,
		&out.ResultDigest, &body, &usage, &out.Claimant, &retry, &failure, &out.IntentSequence, &out.IntentDigest, &out.TokenReserve)
	if err != nil {
		return out, notFound(err)
	}
	out.View.Kind = wire.EffectKind(kind)
	out.View.Status = wire.OperationStatus(status)
	out.View.Revision = strconv.FormatInt(revision, 10)
	out.Arguments = []byte(arguments)
	if failure != "" {
		out.View.Error = &failure
	}
	if body != "" {
		receipt, err := decode[wire.Receipt](body)
		if err != nil {
			return out, err
		}
		out.View.Result = receipt.Result
		out.View.ResultAvailable = true
	}
	if usage != "" {
		value, err := decode[wire.Usage](usage)
		if err != nil {
			return out, err
		}
		out.View.Usage = &value
	}
	if retry != "" {
		out.View.RetryOf = &retry
	}
	return out, nil
}

func (s *Store) operation(ctx context.Context, q queryer, session, id string) (control.Operation, error) {
	return scanOperation(q.QueryRowContext(ctx, s.q("SELECT "+operationColumns+" FROM operations WHERE session_id=? AND id=?"), session, id))
}

func (s *Store) Operation(ctx context.Context, session, id string) (control.Operation, error) {
	return s.operation(ctx, s.db, session, id)
}

func (s *Store) Operations(ctx context.Context, session string, limit int, after ...string) ([]control.Operation, error) {
	rows, err := s.pageRows(ctx, operationColumns, "operations", "session_id=? AND id>?", "id", s.operationBytes(), limit, session, control.After(after))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []control.Operation{}
	for rows.Next() {
		op, err := scanOperation(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, op)
	}
	return out, rows.Err()
}

func (s *Store) Prepare(ctx context.Context, owner control.Owner, request control.Prepare) (out control.Operation, err error) {
	if request.OutputLimit < 1 || request.OutputLimit > 196608 {
		return out, kernel.Fail(wire.ErrorCodeInvalid, "operation output exceeds the admitted inline bound")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		session, e := s.guard(ctx, tx, request.Session, owner, true)
		if e != nil {
			return e
		}
		hash := kernel.Digest(request.Arguments)
		out, e = s.operation(ctx, tx, request.Session, request.ID)
		if e == nil {
			if out.View.ArgumentsDigest != hash || out.View.Action != request.Action || out.View.StepKey != request.StepKey {
				return kernel.Fail(wire.ErrorCodeConflict, "harness step diverged from its durable operation")
			}
			return nil
		}
		if kernel.Response(e).Code != wire.ErrorCodeNotFound {
			return e
		}
		var active int
		e = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM runs WHERE id=? AND session_id=? AND status IN ('queued','active','waiting')"), request.Run, request.Session).Scan(&active)
		if e != nil || active != 1 {
			return kernel.Fail(wire.ErrorCodeConflict, "run no longer admits operations")
		}
		now, e := s.now(ctx, tx)
		if e != nil {
			return e
		}
		if session.View.Config.Limits.DeadlineMs != nil && uint64(now) >= *session.View.Config.Limits.DeadlineMs {
			return kernel.Fail(wire.ErrorCodeForbidden, "session deadline reached")
		}
		var intent int
		if e = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM allocations WHERE session_id=? AND sequence=? AND status='written' AND digest=?"), request.Session, request.IntentSequence, request.IntentDigest).Scan(&intent); e != nil {
			return e
		}
		if intent != 1 {
			return kernel.Fail(wire.ErrorCodeConflict, "dispatch intent has not durably reached the transcript")
		}
		// Reserve payload plus conservative SQLite/index/WAL overhead before send.
		reserve := kernel.OutputReservation(request.OutputLimit)
		request.TokenReserve = 0
		if request.Kind == wire.EffectKindModel {
			request.TokenReserve, e = kernel.ModelTokenReservation(session.View.Config, request.Arguments)
			if e != nil {
				return e
			}
		}
		if e = s.reserveBudgets(ctx, tx, request.Session, reserve, request.TokenReserve); e != nil {
			return e
		}
		deadline := now + request.Timeout
		if session.View.Config.Limits.DeadlineMs != nil {
			deadline = min(deadline, int64(*session.View.Config.Limits.DeadlineMs))
		}
		_, e = tx.ExecContext(ctx, s.q(`INSERT INTO operations(id,session_id,run_id,step_key,kind,action,arguments,
arguments_digest,recipient,output_limit,deadline_ms,generation,approval,retry_of,intent_sequence,intent_digest,token_reserve)
VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`), request.ID, request.Session, request.Run, request.StepKey,
			request.Kind, request.Action, string(request.Arguments), hash, request.Recipient,
			request.OutputLimit, deadline, request.Generation, boolInt(request.Approval), request.RetryOf, request.IntentSequence, request.IntentDigest, request.TokenReserve)
		if e != nil {
			return e
		}
		if _, e = s.accept(ctx, tx, kernel.Principal{Tenant: session.Tenant, Subject: "kernel"}, request.Session, wire.Input{
			RequestID: "prepared:" + request.ID, Data: wire.InputData{Kind: "journal", Name: kernel.Ptr("operation.prepared"), Data: kernel.JSON(map[string]string{"id": request.ID, "recipient": request.Recipient})},
		}); e != nil {
			return e
		}
		out, e = s.operation(ctx, tx, request.Session, request.ID)
		return e
	})
	return
}

func boolInt(value bool) int {
	if value {
		return 1
	}
	return 0
}

func (s *Store) ClaimDispatch(ctx context.Context, session string, owner control.Owner, id, claimant, requestKey string) (out control.Operation, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.guard(ctx, tx, session, owner, true)
		if e != nil {
			return e
		}
		now, e := s.now(ctx, tx)
		if e != nil {
			return e
		}
		if current.View.Config.Limits.DeadlineMs != nil && uint64(now) >= *current.View.Config.Limits.DeadlineMs {
			return kernel.Fail(wire.ErrorCodeForbidden, "session deadline reached before dispatch")
		}
		out, err = s.operation(ctx, tx, session, id)
		if err != nil {
			return err
		}
		if out.View.Status != wire.OperationStatusPrepared {
			return kernel.Fail(wire.ErrorCodeConflict, "operation was already claimed; notification replay is not execution permission")
		}
		var approval, live int
		if err := tx.QueryRowContext(ctx, s.q("SELECT approval FROM operations WHERE id=?"), id).Scan(&approval); err != nil {
			return err
		}
		if approval != 0 {
			if err := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM interactions WHERE session_id=? AND subject=? AND subject_digest=? AND status='answered' AND expires_ms>"+s.clock()), session, id, out.View.ArgumentsDigest).Scan(&live); err != nil {
				return err
			}
			if live != 1 {
				return kernel.Fail(wire.ErrorCodeForbidden, "operation has no current approval for these exact arguments")
			}
		}
		if out.View.Recipient != "" && out.View.Recipient != "model" && out.View.Recipient != "internal" {
			err := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM connections WHERE session_id=? AND id=? AND status='active' AND generation=?"),
				session, out.View.Recipient, out.Generation).Scan(&live)
			if err != nil {
				return err
			}
			if live != 1 || (claimant != owner.Node && claimant != out.View.Recipient) {
				return kernel.Fail(wire.ErrorCodeForbidden, "connection is revoked, replaced or not the invocation recipient")
			}
		}
		e = requireOne(tx.ExecContext(ctx, s.q(`UPDATE operations SET status='dispatched',claimant=?,claim_key=?,revision=revision+1
WHERE id=? AND session_id=? AND status='prepared' AND deadline_ms>`+s.clock()+`
AND (?=1 OR ?>`+s.clock()+`)
AND EXISTS(SELECT 1 FROM runs r WHERE r.id=operations.run_id AND r.status IN ('queued','active','waiting'))`),
			claimant, requestKey, id, session, boolInt(current.View.Config.Limits.DeadlineMs == nil), kernel.Value(current.View.Config.Limits.DeadlineMs)))
		if e != nil {
			return e
		}
		out, e = s.operation(ctx, tx, session, id)
		return e
	})
	// The caller may send exactly once only when this commit returns nil.
	return
}

func (s *Store) RecordReceipt(ctx context.Context, session, id string, receipt wire.Receipt, sequence int64, digest string) (out control.Operation, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.session(ctx, tx, session, true)
		if e != nil {
			return e
		}
		if current.View.Lifecycle == wire.LifecycleDeleted {
			return kernel.Fail(wire.ErrorCodeConflict, "deleted session cannot accept new receipt bodies")
		}
		out, e = s.operation(ctx, tx, session, id)
		if e != nil {
			return e
		}
		body := string(kernel.JSON(receipt))
		hash := kernel.Digest(kernel.JSON(struct {
			Outcome wire.Outcome
			Result  any
			Usage   *wire.Usage
			Error   *string
		}{receipt.Outcome, receipt.Result, receipt.Usage, receipt.Error}))
		var oldHash, oldKey string
		if err := tx.QueryRowContext(ctx, s.q("SELECT receipt_hash,receipt_key FROM operations WHERE id=?"), id).Scan(&oldHash, &oldKey); err != nil {
			return err
		}
		if oldKey != "" && oldKey == receipt.RequestID && oldHash != hash {
			return kernel.Fail(wire.ErrorCodeConflict, "receipt request key has different evidence")
		}
		if out.View.Status == wire.OperationStatusUnknown && receipt.Outcome == wire.OutcomeUnknown && oldHash == hash {
			return nil
		}
		if out.View.Status == wire.OperationStatusCompleted || out.View.Status == wire.OperationStatusFailed {
			if oldHash != hash {
				return kernel.Fail(wire.ErrorCodeConflict, "conflicting settled receipt")
			}
			return nil
		}
		if out.View.Status != wire.OperationStatusDispatched && out.View.Status != wire.OperationStatusUnknown {
			return kernel.Fail(wire.ErrorCodeConflict, "receipt has no committed dispatch claim")
		}
		if receipt.Outcome != wire.OutcomeCompleted && receipt.Outcome != wire.OutcomeFailed && receipt.Outcome != wire.OutcomeUnknown {
			return kernel.Fail(wire.ErrorCodeInvalid, "invalid receipt outcome")
		}
		var unavailable bool
		receipt, unavailable, e = kernel.BoundedReceipt(receipt, out.OutputLimit)
		if e != nil {
			return e
		}
		body = string(kernel.JSON(receipt))
		if unavailable {
			body = ""
			sequence = 0
			digest = ""
		}
		if receipt.Outcome == wire.OutcomeCompleted && receipt.Result == nil && receipt.Error != nil {
			body = ""
		}
		if sequence < 0 || (sequence == 0 && digest != "") {
			return kernel.Fail(wire.ErrorCodeInvalid, "receipt result position is invalid")
		}
		if sequence > 0 {
			var written int
			if e = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM allocations WHERE session_id=? AND sequence=? AND status='written' AND digest=?"),
				session, sequence, digest).Scan(&written); e != nil {
				return e
			}
			if written != 1 {
				return kernel.Fail(wire.ErrorCodeConflict, "receipt result has no matching written evidence")
			}
		}
		usage := ""
		if receipt.Usage != nil {
			usage = string(kernel.JSON(receipt.Usage))
		}
		e = requireOne(tx.ExecContext(ctx, s.q(`UPDATE operations SET status=?,revision=revision+1,
result_sequence=?,result_digest=?,result_body=?,usage=?,error=?,receipt_hash=?,receipt_key=?
WHERE id=? AND session_id=? AND status IN ('dispatched','unknown')`), kernel.OutcomeStatus(receipt.Outcome),
			sequence, digest, body, usage, kernel.Value(receipt.Error), hash, receipt.RequestID, id, session))
		if e != nil {
			return e
		}
		if e = s.settlePackage(ctx, tx, current, out, receipt); e != nil {
			return e
		}
		if receipt.Outcome != wire.OutcomeUnknown {
			if e = s.settleBudgets(ctx, tx, out, receipt, len(body), false, sequence > 0); e != nil {
				return e
			}
		}
		if _, e = tx.ExecContext(ctx, s.q("UPDATE sessions SET ready=1 WHERE id=?"), session); e != nil {
			return e
		}
		out, e = s.operation(ctx, tx, session, id)
		return e
	})
	return
}

func (s *Store) BindResult(ctx context.Context, session string, owner control.Owner, id string, sequence int64, digest string) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, false); err != nil {
			return err
		}
		operation, err := s.operation(ctx, tx, session, id)
		if err != nil {
			return err
		}
		if operation.ResultSequence == sequence && sequence > 0 && operation.ResultDigest == digest {
			return nil
		}
		if err = requireOne(tx.ExecContext(ctx, s.q(`UPDATE operations SET result_sequence=?,result_digest=? WHERE id=? AND session_id=? AND result_sequence=0
AND status IN ('completed','failed','unknown') AND EXISTS(SELECT 1 FROM allocations a WHERE a.session_id=operations.session_id
AND a.sequence=? AND a.status='written' AND a.digest=?)`), sequence, digest, id, session, sequence, digest)); err != nil {
			return err
		}
		if operation.View.Status == wire.OperationStatusCompleted || operation.View.Status == wire.OperationStatusFailed {
			return s.releaseOutputBudgets(ctx, tx, operation)
		}
		return nil
	})
}

func (s *Store) expiration() string {
	return `o.status IN ('prepared','dispatched') AND (o.deadline_ms<=` + s.clock() + `
OR (o.status='dispatched' AND EXISTS(SELECT 1 FROM nodes n WHERE n.id=o.claimant AND (n.lease<=` + s.clock() + ` OR n.draining<>0)))
OR (o.status='prepared' AND (s.lifecycle<>'open'
OR EXISTS(SELECT 1 FROM runs r WHERE r.id=o.run_id AND r.status IN ('completed','failed','cancelled','blocked'))
OR EXISTS(SELECT 1 FROM connections c WHERE c.id=o.recipient AND c.status<>'active')
OR EXISTS(SELECT 1 FROM interactions i WHERE i.session_id=o.session_id AND i.subject=o.id AND (i.status IN ('rejected','cancelled','expired') OR i.expires_ms<=` + s.clock() + `)))))`
}

func (s *Store) ExpireOperations(ctx context.Context, limit int) (count int, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		rows, e := tx.QueryContext(ctx, s.q(`SELECT o.session_id,o.id FROM operations o JOIN sessions s ON s.id=o.session_id
WHERE `+s.expiration()+` ORDER BY o.deadline_ms,o.id LIMIT ?`), limit)
		if e != nil {
			return e
		}
		type key struct{ session, id string }
		keys := []key{}
		for rows.Next() {
			var k key
			if e = rows.Scan(&k.session, &k.id); e != nil {
				rows.Close()
				return e
			}
			keys = append(keys, k)
		}
		e = rows.Err()
		rows.Close()
		if e != nil {
			return e
		}
		for _, k := range keys {
			_, e := s.session(ctx, tx, k.session, true)
			if e != nil {
				return e
			}
			op, e := s.operation(ctx, tx, k.session, k.id)
			if e != nil {
				return e
			}
			var expired int
			if e = tx.QueryRowContext(ctx, s.q(`SELECT COUNT(*) FROM operations o JOIN sessions s ON s.id=o.session_id
WHERE o.id=? AND `+s.expiration()), k.id).Scan(&expired); e != nil {
				return e
			}
			if expired == 0 {
				continue
			}
			status, reason := "unknown", "dispatch deadline passed; the original recipient may still supply authenticated evidence"
			body := ""
			if op.View.Status == wire.OperationStatusPrepared {
				status, reason = "failed", "operation was never dispatched before its deadline, cancellation, revocation or rejected approval"
				body = string(kernel.JSON(wire.Receipt{RequestID: "not-dispatched:" + op.View.ID, Outcome: wire.OutcomeFailed, Error: &reason}))
				if e = s.settleBudgets(ctx, tx, op, wire.Receipt{}, len(body), true, false); e != nil {
					return e
				}
			}
			if e = requireOne(tx.ExecContext(ctx, s.q(`UPDATE operations AS o SET status=?,result_body=?,error=?,revision=revision+1
WHERE o.id=? AND o.status=? AND EXISTS(SELECT 1 FROM sessions s WHERE s.id=o.session_id AND `+s.expiration()+`)`),
				status, body, reason, op.View.ID, op.View.Status)); e != nil {
				return e
			}
			if _, e = tx.ExecContext(ctx, s.q("UPDATE sessions SET ready=1 WHERE id=?"), k.session); e != nil {
				return e
			}
			count++
		}
		due, e := tx.QueryContext(ctx, s.q("SELECT id,session_id FROM interactions WHERE status='pending' AND expires_ms<="+s.clock()+" ORDER BY expires_ms,id LIMIT ?"), limit)
		if e != nil {
			return e
		}
		interactions := []key{}
		for due.Next() {
			var item key
			if e = due.Scan(&item.id, &item.session); e != nil {
				due.Close()
				return e
			}
			interactions = append(interactions, item)
		}
		e = due.Err()
		due.Close()
		if e != nil {
			return e
		}
		for _, item := range interactions {
			if _, e = tx.ExecContext(ctx, s.q("UPDATE sessions SET ready=1 WHERE id=?"), item.session); e != nil {
				return e
			}
			if _, e = tx.ExecContext(ctx, s.q("UPDATE interactions SET status='expired',revision=revision+1 WHERE id=? AND status='pending' AND expires_ms<="+s.clock()), item.id); e != nil {
				return e
			}
		}
		return nil
	})
	return
}
