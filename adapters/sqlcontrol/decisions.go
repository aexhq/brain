package sqlcontrol

import (
	"context"
	"database/sql"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Store) BeginDecision(ctx context.Context, session string, owner control.Owner, run string, step uint64, digest, process string, repeatable bool) error {
	if len(digest) != 64 || !kernel.Identifier(process) || step > 4294967295 {
		return kernel.Fail(wire.ErrorCodeInvalid, "invalid harness decision boundary")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, true); err != nil {
			return err
		}
		var prior int64
		var allowed int
		var hash string
		err := tx.QueryRowContext(ctx, s.q(`SELECT decision_started_step,decision_input_hash,decision_repeatable FROM runs
WHERE session_id=? AND id=? AND step=? AND decision='' AND status IN ('queued','active','waiting')`), session, run, step).
			Scan(&prior, &hash, &allowed)
		if err == sql.ErrNoRows {
			return kernel.Fail(wire.ErrorCodeConflict, "run no longer holds this decision boundary")
		}
		if err != nil {
			return err
		}
		if prior == int64(step) {
			if hash != digest {
				return kernel.Fail(wire.ErrorCodeCorrupt, "harness decision request diverged from its original boundary")
			}
			if allowed == 0 || !repeatable {
				return kernel.Fail(wire.ErrorCodeUnsupported, "harness decision was already admitted without repeatable continuation")
			}
		}
		_, err = tx.ExecContext(ctx, s.q(`UPDATE runs SET harness_process=?,decision_started_step=?,decision_input_hash=?,decision_repeatable=?
WHERE session_id=? AND id=? AND step=?`), process, step, digest, boolInt(repeatable), session, run, step)
		return err
	})
}
