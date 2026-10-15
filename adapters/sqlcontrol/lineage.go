package sqlcontrol

import (
	"context"
	"database/sql"
	"errors"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Store) lockLineage(ctx context.Context, tx *sql.Tx, root string, exclusive bool) error {
	query := "SELECT root_id FROM tree_authority WHERE root_id=?"
	if s.postgres {
		if exclusive {
			query += " FOR UPDATE"
		} else {
			query += " FOR SHARE"
		}
	}
	var identifier string
	return tx.QueryRowContext(ctx, s.q(query), root).Scan(&identifier)
}

func (s *Store) liveLineage(ctx context.Context, tx *sql.Tx, session control.Session, dispatch bool) error {
	if err := s.lockLineage(ctx, tx, session.Budget, false); err != nil {
		return err
	}
	var denied int
	query := `SELECT COUNT(*) FROM budget_members m JOIN budgets b ON b.id=m.budget_id WHERE m.session_id=?
AND ((m.budget_id<>m.session_id AND m.generation<>b.cancel_generation) OR (?=1 AND b.paused<>0))`
	if err := tx.QueryRowContext(ctx, s.q(query), session.View.ID, boolInt(dispatch)).Scan(&denied); err != nil {
		return err
	}
	if denied != 0 {
		return kernel.Fail(wire.ErrorCodeConflict, "an enclosing session cancelled or paused this delegated authority")
	}
	return nil
}

func (s *Store) pauseLineage(ctx context.Context, tx *sql.Tx, session control.Session, paused bool) error {
	if err := s.lockLineage(ctx, tx, session.Budget, true); err != nil {
		return err
	}
	return requireOne(tx.ExecContext(ctx, s.q("UPDATE budgets SET paused=? WHERE id=?"), boolInt(paused), session.View.ID))
}

func (s *Store) cancelLineage(ctx context.Context, tx *sql.Tx, session control.Session) error {
	if err := s.lockLineage(ctx, tx, session.Budget, true); err != nil {
		return err
	}
	var generation int64
	if err := tx.QueryRowContext(ctx, s.q(`UPDATE budgets SET cancel_generation=cancel_generation+1
WHERE id=? AND cancel_generation<9223372036854775807 RETURNING cancel_generation`), session.View.ID).Scan(&generation); err != nil {
		return err
	}
	_, err := tx.ExecContext(ctx, s.q(`INSERT INTO control_cascades(session_id,generation) VALUES(?,?)
ON CONFLICT(session_id) DO UPDATE SET generation=excluded.generation,after_id='',status='pending'`), session.View.ID, generation)
	return err
}

func (s *Store) PropagateControls(ctx context.Context, limit int) (closed []string, err error) {
	closed = []string{}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		var parent, after string
		var generation int64
		e := tx.QueryRowContext(ctx, "SELECT session_id,generation,after_id FROM control_cascades WHERE status='pending' ORDER BY session_id LIMIT 1").
			Scan(&parent, &generation, &after)
		if errors.Is(e, sql.ErrNoRows) {
			return nil
		}
		if e != nil {
			return e
		}
		rows, e := tx.QueryContext(ctx, s.q(`SELECT session_id FROM budget_members
WHERE budget_id=? AND session_id<>? AND generation<? AND session_id>? ORDER BY session_id LIMIT ?`),
			parent, parent, generation, after, max(1, min(limit, 4)))
		if e != nil {
			return e
		}
		candidates := []string{}
		for rows.Next() {
			var identifier string
			if e = rows.Scan(&identifier); e != nil {
				rows.Close()
				return e
			}
			candidates = append(candidates, identifier)
		}
		e = rows.Err()
		rows.Close()
		if e != nil {
			return e
		}
		for _, identifier := range candidates {
			current, e := s.session(ctx, tx, identifier, true)
			if e != nil {
				return e
			}
			if current.View.Lifecycle == wire.LifecycleDeleted {
				continue
			}
			for _, query := range []string{
				"UPDATE sessions SET lifecycle='closed',ready=1,revision=revision+1 WHERE id=? AND lifecycle<>'closed'",
				"UPDATE runs SET status='cancelled' WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked')",
				"UPDATE commands SET status='cancelled' WHERE session_id=? AND status='accepted' AND kind IN ('message','action','event')",
				"UPDATE interactions SET status='cancelled',revision=revision+1 WHERE session_id=? AND status='pending'",
				"UPDATE schedules SET status='cancelled' WHERE session_id=? AND status='active'",
				wakeParents,
			} {
				if _, e = tx.ExecContext(ctx, s.q(query), identifier); e != nil {
					return e
				}
			}
			closed = append(closed, identifier)
		}
		next, status := after, "done"
		if len(candidates) != 0 {
			next, status = candidates[len(candidates)-1], "pending"
		}
		return requireOne(tx.ExecContext(ctx, s.q(`UPDATE control_cascades SET after_id=?,status=?
WHERE session_id=? AND generation=? AND after_id=? AND status='pending'`), next, status, parent, generation, after))
	})
	if err != nil {
		closed = nil
	}
	return
}
