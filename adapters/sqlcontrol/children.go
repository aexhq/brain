package sqlcontrol

import (
	"context"
	"database/sql"
	"strconv"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

const wakeParents = `WITH changed(id) AS (VALUES(?))
UPDATE sessions SET ready=1 WHERE id IN (
SELECT c.session_id FROM run_children c JOIN changed ON c.child_id=changed.id
UNION SELECT r.session_id FROM runs r JOIN changed ON r.wait_id=changed.id WHERE r.status='waiting' AND r.wait_kind='child')`

const childrenReady = `r.wait_kind='children' AND NOT EXISTS(
SELECT 1 FROM run_children links JOIN commands input ON input.id=links.command_id
LEFT JOIN runs child ON child.id=input.run_id
WHERE links.session_id=r.session_id AND links.run_id=r.id AND links.step=r.step-1
AND (input.status='accepted' OR child.status IN ('queued','active','waiting','paused')))`

func (s *Store) Children(ctx context.Context, session, run string, step uint64) (out wire.ChildResults, err error) {
	out = wire.ChildResults{SessionID: session, RunID: run, Step: step, Ready: true, Children: []wire.ChildResult{}}
	err = s.db.QueryRowContext(ctx, s.q("SELECT expected FROM child_groups WHERE session_id=? AND run_id=? AND step=?"),
		session, run, step).Scan(&out.Expected)
	if err != nil {
		return out, notFound(err)
	}
	rows, err := s.db.QueryContext(ctx, s.q(`SELECT links.ordinal,links.child_id,links.command_id,input.status,input.run_id,
COALESCE(child.status,''),COALESCE(child.error,input.error),COALESCE(child.completion_sequence,0),COALESCE(child.completion_digest,'')
FROM run_children links JOIN commands input ON input.id=links.command_id
LEFT JOIN runs child ON child.id=input.run_id AND child.session_id=links.child_id
WHERE links.session_id=? AND links.run_id=? AND links.step=? ORDER BY links.ordinal LIMIT 9`), session, run, step)
	if err != nil {
		return out, err
	}
	defer rows.Close()
	for rows.Next() {
		var value wire.ChildResult
		var input, status, failure, digest string
		var sequence int64
		if err = rows.Scan(&value.Index, &value.SessionID, &value.CommandID, &input, &value.RunID, &status, &failure, &sequence, &digest); err != nil {
			return out, err
		}
		if status == "" {
			switch input {
			case "accepted":
				status = "queued"
			case "failed", "cancelled":
				status = input
			default:
				return out, kernel.Fail(wire.ErrorCodeCorrupt, "delegated input has no matching run")
			}
		}
		value.Status = wire.RunStatus(status)
		value.Availability = wire.ChildResultAvailabilityPending
		if kernel.Terminal(value.Status) {
			value.Availability = wire.ChildResultAvailabilityNone
		}
		if value.Status == wire.RunStatusCompleted {
			value.Availability = wire.ChildResultAvailabilityUnavailable
			if sequence > 0 && len(digest) == 64 {
				value.Availability = wire.ChildResultAvailabilityReferenced
				value.Completion = &wire.RecordReference{SessionID: value.SessionID, Sequence: strconv.FormatInt(sequence, 10), Digest: digest}
			}
		}
		if failure != "" {
			f, err := decode[wire.ErrorResponse](failure)
			if err != nil {
				return out, err
			}
			value.Error = &f
		}
		if value.Availability == wire.ChildResultAvailabilityPending {
			out.Ready = false
		}
		out.Children = append(out.Children, value)
	}
	if err = rows.Err(); err != nil {
		return out, err
	}
	if len(out.Children) > kernel.MaxForkChildren || uint64(len(out.Children)) > out.Expected {
		return out, kernel.Fail(wire.ErrorCodeCorrupt, "delegation exceeds its bounded decision")
	}
	out.Ready = out.Ready && uint64(len(out.Children)) == out.Expected
	return out, nil
}

func (s *Store) CompleteRun(ctx context.Context, session string, owner control.Owner, run string, step uint64, record wire.Record) error {
	reference, err := kernel.CompletionReference(session, run, record)
	if err != nil {
		return err
	}
	sequence, _ := kernel.Sequence(reference.Sequence)
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.guard(ctx, tx, session, owner, true); err != nil {
			return err
		}
		var body, status, digest string
		var savedStep uint64
		var savedSequence int64
		if err := tx.QueryRowContext(ctx, s.q("SELECT decision,status,step,completion_sequence,completion_digest FROM runs WHERE session_id=? AND id=?"),
			session, run).Scan(&body, &status, &savedStep, &savedSequence, &digest); err != nil {
			return notFound(err)
		}
		if status == "completed" && savedSequence == sequence && digest == reference.Digest {
			return nil
		}
		if status != "active" || savedStep != step || body == "" || step >= 4294967295 {
			return kernel.Fail(wire.ErrorCodeConflict, "run no longer holds this completion decision")
		}
		decision, err := decode[wire.Decision](body)
		if err != nil {
			return err
		}
		if err = kernel.ValidateCompletion(decision, record); err != nil {
			return err
		}
		err = requireOne(tx.ExecContext(ctx, s.q(`UPDATE runs SET status='completed',step=step+1,checkpoint=?,decision='',
wait_kind='',wait_id='',error='',completion_sequence=?,completion_digest=?,updated_ms=`+s.clock()+`
WHERE id=? AND session_id=? AND status='active' AND step=? AND EXISTS(
SELECT 1 FROM allocations WHERE session_id=? AND sequence=? AND digest=? AND status='written')`),
			string(decision.Checkpoint), sequence, reference.Digest, run, session, step, session, sequence, reference.Digest))
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, s.q(wakeParents), session)
		return err
	})
}
