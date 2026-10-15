package sqlcontrol

import (
	"context"
	"database/sql"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Store) ReportUsage(ctx context.Context, session, operation string, request wire.UsageSupplement, reporter control.UsageReporter) (out control.Operation, err error) {
	if !kernel.Identifier(request.RequestID) || !kernel.Identifier(reporter.Subject) {
		return out, kernel.Fail(wire.ErrorCodeInvalid, "usage evidence requires bounded original identifiers")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, err := s.session(ctx, tx, session, true)
		if err != nil {
			return err
		}
		out, err = s.operation(ctx, tx, session, operation)
		if err != nil {
			return err
		}
		switch reporter.Kind {
		case wire.UsageSourceOperator:
			if reporter.Tenant != current.Tenant {
				return kernel.Fail(wire.ErrorCodeNotFound, "session not found")
			}
		case wire.UsageSourceReceipt:
			if reporter.Subject != out.Claimant || out.Claimant == "" || reporter.Tenant != "" {
				return kernel.Fail(wire.ErrorCodeForbidden, "usage evidence does not belong to the original recipient")
			}
		default:
			return kernel.Fail(wire.ErrorCodeForbidden, "usage evidence requires an original recipient or operator attestation")
		}
		if out.View.Kind != wire.EffectKindModel || out.Claimant == "" ||
			(out.View.Status != wire.OperationStatusCompleted && out.View.Status != wire.OperationStatusFailed) {
			return kernel.Fail(wire.ErrorCodeConflict, "only a settled originally dispatched model accepts supplemental usage")
		}
		hash := kernel.Digest(kernel.JSON([]any{request, reporter.Kind, reporter.Subject, reporter.Tenant}))
		var prior string
		err = tx.QueryRowContext(ctx, s.q("SELECT request_hash FROM usage_evidence WHERE operation_id=? AND request_key=?"),
			operation, request.RequestID).Scan(&prior)
		if err != nil && err != sql.ErrNoRows {
			return err
		}
		if err == nil && prior != hash {
			return kernel.Fail(wire.ErrorCodeConflict, "usage request key has different evidence or attribution")
		}
		if current.View.Lifecycle == wire.LifecycleDeleted {
			if err := s.discardOperation(ctx, tx, &out); err != nil {
				return err
			}
		}
		if prior != "" {
			return nil
		}
		usage, difference, complete, err := kernel.RefineUsage(out.View.Usage, request.Counters)
		if err != nil {
			return err
		}
		var count int
		if err := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM usage_evidence WHERE operation_id=?"), operation).Scan(&count); err != nil {
			return err
		}
		if count >= 3 {
			return kernel.Fail(wire.ErrorCodeCorrupt, "usage evidence exceeds the three-counter refinement bound")
		}
		now, err := s.now(ctx, tx)
		if err != nil {
			return err
		}
		evidence := wire.UsageEvidence{ID: kernel.ID("usage", session, operation, request.RequestID),
			SessionID: session, OperationID: operation, RequestID: request.RequestID, Source: reporter.Kind,
			Subject: reporter.Subject, Counters: request.Counters, AcceptedMs: uint64(now)}
		if _, err := tx.ExecContext(ctx, s.q("INSERT INTO usage_evidence(id,session_id,operation_id,request_key,request_hash,body) VALUES(?,?,?,?,?,?)"),
			evidence.ID, session, operation, request.RequestID, hash, string(kernel.JSON(evidence))); err != nil {
			return err
		}
		if err := requireOne(tx.ExecContext(ctx, s.q(`UPDATE operations SET usage=?,revision=revision+1
WHERE session_id=? AND id=? AND status IN ('completed','failed')`), string(kernel.JSON(usage)), session, operation)); err != nil {
			return err
		}
		pools, err := s.budgetPools(ctx, tx, session, true)
		if err != nil {
			return err
		}
		reserve, incomplete := int64(0), 0
		if complete {
			reserve, incomplete = out.TokenReserve, 1
		}
		for _, pool := range pools {
			if err := requireOne(tx.ExecContext(ctx, s.q(`UPDATE budgets SET tokens=tokens+?,
reserved_tokens=reserved_tokens-?,incomplete_usages=incomplete_usages-?
WHERE id=? AND reserved_tokens>=? AND incomplete_usages>=?`),
				difference, reserve, incomplete, pool.id, reserve, incomplete)); err != nil {
				return err
			}
		}
		out, err = s.operation(ctx, tx, session, operation)
		return err
	})
	return
}

func (s *Store) UsageEvidence(ctx context.Context, session, operation string, limit int, after ...string) ([]wire.UsageEvidence, error) {
	predicate := "session_id=? AND id>?"
	arguments := []any{session, control.After(after)}
	if operation != "" {
		predicate += " AND operation_id=?"
		arguments = append(arguments, operation)
	}
	rows, err := s.pageRows(ctx, "body", "usage_evidence", predicate, "id", s.bytes("body"), limit, arguments...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	values := []wire.UsageEvidence{}
	for rows.Next() {
		var body string
		if err := rows.Scan(&body); err != nil {
			return nil, err
		}
		value, err := decode[wire.UsageEvidence](body)
		if err != nil {
			return nil, err
		}
		values = append(values, value)
	}
	return values, rows.Err()
}
