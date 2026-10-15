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

func scanSchedule(row scanner) (wire.Schedule, error) {
	var out wire.Schedule
	var body string
	err := row.Scan(&out.ID, &out.SessionID, &out.DueMs, &out.Status, &body)
	if err != nil {
		return out, notFound(err)
	}
	out.Request, err = decode[wire.ScheduleCreate](body)
	return out, err
}

const scheduleColumns = "id,session_id,due_ms,status,request"

func (s *Store) Schedule(ctx context.Context, session string, request wire.ScheduleCreate) (out wire.Schedule, err error) {
	id := kernel.ID("schedule", session, request.RequestID)
	body := string(kernel.JSON(request))
	hash := kernel.Digest([]byte(body))
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.liveSession(ctx, tx, session)
		if e != nil {
			return e
		}
		if len(kernel.JSON(request.Input)) > int(current.View.Config.Limits.InputBytes) ||
			request.DueMs > 1<<53-1 || kernel.Value(request.IntervalMs) > 1<<53-1 {
			return kernel.Fail(wire.ErrorCodeInvalid, "scheduled input or time exceeds its admitted bound")
		}
		if request.IntervalMs != nil && *request.IntervalMs < 1000 {
			return kernel.Fail(wire.ErrorCodeInvalid, "recurring interval must be at least one second")
		}
		var prior string
		e = tx.QueryRowContext(ctx, s.q("SELECT request_hash FROM schedules WHERE id=?"), id).Scan(&prior)
		if e == nil && prior != hash {
			return kernel.Fail(wire.ErrorCodeConflict, "schedule key has different content")
		}
		if e != nil && !errors.Is(e, sql.ErrNoRows) {
			return e
		}
		if e != nil {
			var count int
			if e := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM schedules WHERE session_id=? AND status='active'"), session).Scan(&count); e != nil {
				return e
			}
			if count >= 128 {
				return kernel.Fail(wire.ErrorCodeOverloaded, "active schedule limit reached")
			}
			_, e = tx.ExecContext(ctx, s.q("INSERT INTO schedules(id,session_id,due_ms,interval_ms,request,request_hash) VALUES(?,?,?,?,?,?)"),
				id, session, request.DueMs, kernel.Value(request.IntervalMs), body, hash)
			if e != nil {
				return e
			}
		}
		out, e = scanSchedule(tx.QueryRowContext(ctx, s.q("SELECT "+scheduleColumns+" FROM schedules WHERE id=?"), id))
		return e
	})
	return
}

func (s *Store) Schedules(ctx context.Context, session string, limit int, after ...string) ([]wire.Schedule, error) {
	rows, err := s.pageRows(ctx, scheduleColumns, "schedules", "session_id=? AND id>?", "id", s.bytes("request"), limit, session, control.After(after))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []wire.Schedule{}
	for rows.Next() {
		value, err := scanSchedule(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, value)
	}
	return out, rows.Err()
}

func (s *Store) CancelSchedule(ctx context.Context, session, id string) (out wire.Schedule, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := s.session(ctx, tx, session, true); e != nil {
			return e
		}
		var e error
		out, e = scanSchedule(tx.QueryRowContext(ctx, s.q("UPDATE schedules SET status='cancelled' WHERE session_id=? AND id=? RETURNING "+scheduleColumns), session, id))
		return e
	})
	return
}

func (s *Store) FireTimers(ctx context.Context, limit int) (count int, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		now, e := s.now(ctx, tx)
		if e != nil {
			return e
		}
		rows, e := tx.QueryContext(ctx, s.q("SELECT "+scheduleColumns+" FROM schedules WHERE status='active' AND due_ms<=? ORDER BY due_ms,id LIMIT ?"+s.lock()), now, limit)
		if e != nil {
			return e
		}
		due := []wire.Schedule{}
		for rows.Next() {
			value, e := scanSchedule(rows)
			if e != nil {
				rows.Close()
				return e
			}
			due = append(due, value)
		}
		e = rows.Err()
		rows.Close()
		if e != nil {
			return e
		}
		for _, timer := range due {
			session, e := s.session(ctx, tx, timer.SessionID, true)
			if e != nil {
				return e
			}
			if session.View.Lifecycle != wire.LifecycleOpen {
				continue
			}
			input := wire.Input{RequestID: "timer:" + timer.ID + ":" + strconv.FormatUint(timer.DueMs, 10), Data: timer.Request.Input}
			if _, e := s.accept(ctx, tx, kernel.Principal{Tenant: session.Tenant, Subject: "timer"}, timer.SessionID, input); e != nil {
				if kernel.Response(e).Code == wire.ErrorCodeOverloaded {
					continue
				}
				return e
			}
			interval := kernel.Value(timer.Request.IntervalMs)
			if interval == 0 {
				_, e = tx.ExecContext(ctx, s.q("UPDATE schedules SET status='fired' WHERE id=? AND due_ms=?"), timer.ID, timer.DueMs)
			} else {
				next := timer.DueMs + ((uint64(now)-timer.DueMs)/interval+1)*interval
				_, e = tx.ExecContext(ctx, s.q("UPDATE schedules SET due_ms=? WHERE id=? AND due_ms=?"), next, timer.ID, timer.DueMs)
			}
			if e != nil {
				return e
			}
			count++
		}
		return nil
	})
	return
}

func (s *Store) Grant(ctx context.Context, session string, request wire.ParticipantGrant) (out control.Participant, err error) {
	if err = kernel.ValidateParticipant(request); err != nil {
		return
	}
	id := kernel.ID("participant", session, request.RequestID)
	hash := kernel.Digest(kernel.JSON(request))
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.liveSession(ctx, tx, session)
		if e != nil {
			return e
		}
		now, e := s.now(ctx, tx)
		if e != nil {
			return e
		}
		if request.ExpiresMs <= uint64(now) || request.ExpiresMs > uint64(now)+86400000 {
			return kernel.Fail(wire.ErrorCodeInvalid, "participant token expiry must be within one day")
		}
		parent, e := s.grantAuthority(ctx, tx, current, request)
		if e != nil {
			return e
		}
		var prior, previousParent string
		var revoked int
		e = tx.QueryRowContext(ctx, s.q("SELECT request_hash,parent,revoked FROM participants WHERE id=?"), id).Scan(&prior, &previousParent, &revoked)
		if e == nil && (prior != hash || previousParent != parent) {
			return kernel.Fail(wire.ErrorCodeConflict, "participant key has different arguments or parent authority")
		}
		if e != nil && !errors.Is(e, sql.ErrNoRows) {
			return e
		}
		if e != nil {
			var count, depth int
			if e = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM participants WHERE session_id=?"), session).Scan(&count); e != nil {
				return e
			}
			if e = tx.QueryRowContext(ctx, s.q("SELECT COALESCE(MAX(depth),0) FROM participant_ancestors WHERE descendant=?"), parent).Scan(&depth); e != nil {
				return e
			}
			if count >= 1024 || depth >= 8 {
				return kernel.Fail(wire.ErrorCodeOverloaded, "participant count or delegation depth bound reached")
			}
			_, e = tx.ExecContext(ctx, s.q("INSERT INTO participants(id,session_id,subject,permissions,expires_ms,request_hash,parent) VALUES(?,?,?,?,?,?,?)"),
				id, session, request.Subject, string(kernel.JSON(request.Permissions)), request.ExpiresMs, hash, parent)
			if e != nil {
				return e
			}
			if parent != "" {
				if _, e = tx.ExecContext(ctx, s.q("INSERT INTO participant_ancestors(descendant,ancestor,depth) VALUES(?,?,1)"), id, parent); e != nil {
					return e
				}
				if _, e = tx.ExecContext(ctx, s.q("INSERT INTO participant_ancestors(descendant,ancestor,depth) SELECT ?,ancestor,depth+1 FROM participant_ancestors WHERE descendant=?"),
					id, parent); e != nil {
					return e
				}
			}
		}
		out = control.Participant{ID: id, Parent: parent, Session: session, Tenant: current.Tenant, Subject: request.Subject,
			Permissions: request.Permissions, Expires: int64(request.ExpiresMs), Revoked: revoked != 0}
		return nil
	})
	return
}

func (s *Store) Participant(ctx context.Context, id string) (control.Participant, error) {
	var out control.Participant
	var permissions string
	var revoked int
	err := s.db.QueryRowContext(ctx, s.q(`SELECT p.id,p.parent,p.session_id,s.tenant,p.subject,p.permissions,p.expires_ms,
CASE WHEN p.revoked<>0 OR EXISTS(`+s.invalidParticipantAncestor(false)+`) THEN 1 ELSE 0 END
FROM participants p JOIN sessions s ON p.session_id=s.id WHERE p.id=? AND s.lifecycle<>'deleted' AND p.expires_ms>`+s.clock()), id).
		Scan(&out.ID, &out.Parent, &out.Session, &out.Tenant, &out.Subject, &permissions, &out.Expires, &revoked)
	if err != nil {
		return out, notFound(err)
	}
	out.Revoked = revoked != 0
	out.Permissions, err = decode[[]string](permissions)
	return out, err
}

func (s *Store) RevokeParticipant(ctx context.Context, session, id string) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.session(ctx, tx, session, true); err != nil {
			return err
		}
		if authority := control.RequestAuthority(ctx); authority != nil && authority.Participant != "" && authority.Participant != id {
			var count int
			if err := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM participant_ancestors WHERE descendant=? AND ancestor=?"), id, authority.Participant).Scan(&count); err != nil {
				return err
			}
			if count != 1 {
				return kernel.Fail(wire.ErrorCodeForbidden, "participants may revoke only themselves or their descendants")
			}
		}
		return requireOne(tx.ExecContext(ctx, s.q("UPDATE participants SET revoked=1 WHERE session_id=? AND id=?"), session, id))
	})
}

var _ control.Store = (*Store)(nil)
