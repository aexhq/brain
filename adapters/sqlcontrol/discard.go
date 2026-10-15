package sqlcontrol

import (
	"context"
	"database/sql"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Store) discardOperation(ctx context.Context, tx *sql.Tx, operation *control.Operation) error {
	if operation.Discarded {
		return nil
	}
	receipt := kernel.DiscardReceiptBodies(wire.Receipt{Usage: operation.View.Usage})
	usage := ""
	if receipt.Usage != nil {
		usage = string(kernel.JSON(receipt.Usage))
	}
	if err := requireOne(tx.ExecContext(ctx, s.q(`UPDATE operations SET discarded=1,result_body='',usage=?
WHERE session_id=? AND id=? AND discarded=0`), usage, operation.View.SessionID, operation.View.ID)); err != nil {
		return err
	}
	if operation.ResultSequence == 0 && (operation.View.Status == wire.OperationStatusCompleted || operation.View.Status == wire.OperationStatusFailed) {
		if err := s.releaseOutputBudgets(ctx, tx, *operation); err != nil {
			return err
		}
	}
	operation.Discarded = true
	operation.View.Result = nil
	operation.View.ResultAvailable = false
	operation.View.Usage = receipt.Usage
	return nil
}

func (s *Store) DiscardDeletedResults(ctx context.Context, limit int) (count int, err error) {
	if limit < 1 {
		return 0, kernel.Fail(wire.ErrorCodeInvalid, "deleted-result batch must be positive")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		var session string
		err := tx.QueryRowContext(ctx, "SELECT id FROM sessions WHERE cleanup_ready=1 ORDER BY cleanup_ms,id LIMIT 1").Scan(&session)
		if err == sql.ErrNoRows {
			return nil
		}
		if err != nil {
			return err
		}
		current, err := s.session(ctx, tx, session, true)
		if err != nil {
			return err
		}
		if current.View.Lifecycle != wire.LifecycleDeleted {
			return kernel.Fail(wire.ErrorCodeCorrupt, "result disposal requires a deleted session")
		}
		rows, err := tx.QueryContext(ctx, s.q(`SELECT id,status,result_sequence,output_limit,usage FROM operations
WHERE session_id=? AND discarded=0 ORDER BY id LIMIT ?`), session, min(limit, 32))
		if err != nil {
			return err
		}
		operations := []control.Operation{}
		for rows.Next() {
			var operation control.Operation
			var usage string
			operation.View.SessionID = session
			if err = rows.Scan(&operation.View.ID, &operation.View.Status, &operation.ResultSequence, &operation.OutputLimit, &usage); err != nil {
				rows.Close()
				return err
			}
			if usage != "" {
				value, err := decode[wire.Usage](usage)
				if err != nil {
					rows.Close()
					return err
				}
				operation.View.Usage = &value
			}
			operations = append(operations, operation)
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return err
		}
		for i := range operations {
			if err = s.discardOperation(ctx, tx, &operations[i]); err != nil {
				return err
			}
			count++
		}
		_, err = tx.ExecContext(ctx, s.q(`UPDATE sessions SET cleanup_ready=CASE WHEN
EXISTS(SELECT 1 FROM operations WHERE session_id=? AND discarded=0) THEN 1 ELSE 0 END,cleanup_ms=`+s.clock()+` WHERE id=?`), session, session)
		return err
	})
	return
}
