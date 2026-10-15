package sqlcontrol

import (
	"context"
	"database/sql"
	"errors"
	"math"
	"slices"
	"strconv"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

const transferColumns = `t.id,t.session_id,t.source_node,t.source_epoch,t.target_node,t.phase,
t.through_sequence,t.checkpoint_digest,t.run_cursor,t.tail_verified,t.verified_runs,t.error`
const transferActive = "phase IN ('draining','verifying')"
const transferRuns = "status IN ('queued','active','waiting','paused','blocked')"
const transferDrained = `NOT EXISTS(SELECT 1 FROM operations o WHERE o.session_id=t.session_id AND
(o.status='dispatched' OR (o.status IN ('completed','failed','unknown') AND o.result_sequence=0)))
AND NOT EXISTS(SELECT 1 FROM allocations a WHERE a.session_id=t.session_id AND a.status IN ('reserved','unresolved'))`
const transferArchived = `NOT EXISTS(SELECT 1 FROM allocations a WHERE a.session_id=t.session_id
AND a.sequence<=t.through_sequence AND a.status='written' AND a.archive_key='')`

func scanTransfer(row scanner) (out control.Transfer, err error) {
	var epoch, through int64
	var digest, failure string
	var tail, drained, archived, live int
	err = row.Scan(&out.View.ID, &out.View.SessionID, &out.View.SourceNode, &epoch, &out.View.TargetNode, &out.View.Phase,
		&through, &digest, &out.Cursor, &tail, &out.View.VerifiedRuns, &failure, &drained, &archived, &live)
	if err != nil {
		return out, notFound(err)
	}
	out.View.SourceEpoch, out.View.Through = strconv.FormatInt(epoch, 10), strconv.FormatInt(through, 10)
	out.TailVerified, out.Drained, out.Archived = tail != 0, drained != 0, archived != 0
	out.TargetLive = live != 0
	if through != 0 {
		out.View.Checkpoint = &wire.RecordReference{SessionID: out.View.SessionID, Sequence: out.View.Through, Digest: digest}
	}
	if failure != "" {
		out.View.Error, err = decode[*wire.ErrorResponse](failure)
	}
	return
}

func (s *Store) transfer(ctx context.Context, q queryer, session, id string) (control.Transfer, error) {
	return scanTransfer(q.QueryRowContext(ctx, s.q("SELECT "+transferColumns+",CASE WHEN "+transferDrained+
		" THEN 1 ELSE 0 END,CASE WHEN "+transferArchived+" THEN 1 ELSE 0 END,"+s.transferTargetLive()+
		" FROM ownership_transfers t WHERE t.session_id=? AND t.id=?"), session, id))
}

func (s *Store) transferTargetLive() string {
	return "CASE WHEN EXISTS(SELECT 1 FROM nodes WHERE id=t.target_node AND lease>" + s.clock() + " AND draining=0) THEN 1 ELSE 0 END"
}

func (s *Store) Transfer(ctx context.Context, session, id string) (control.Transfer, error) {
	return s.transfer(ctx, s.db, session, id)
}

func (s *Store) Nodes(ctx context.Context, after string, limit int) ([]wire.NodeInfo, error) {
	rows, err := s.pageRows(ctx, "id,endpoint,lease,draining", "nodes", "id>?", "id", s.bytes("endpoint"), limit, after)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []wire.NodeInfo{}
	for rows.Next() {
		var node wire.NodeInfo
		var draining int
		if err = rows.Scan(&node.ID, &node.Endpoint, &node.LeaseExpiresMs, &draining); err != nil {
			return nil, err
		}
		node.Draining = draining != 0
		out = append(out, node)
	}
	return out, rows.Err()
}

func (s *Store) unfrozen(ctx context.Context, q queryer, session string, allocation bool) error {
	phase := transferActive
	if allocation {
		phase = "phase='verifying'"
	}
	var frozen int
	if err := q.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM ownership_transfers WHERE session_id=? AND "+phase), session).Scan(&frozen); err != nil {
		return err
	}
	if frozen != 0 {
		return kernel.Fail(wire.ErrorCodeConflict, "session is frozen for a planned ownership transfer")
	}
	return nil
}

func (s *Store) transferTarget(ctx context.Context, q queryer, current control.Session, node string) error {
	var valid int
	err := q.QueryRowContext(ctx, s.q(`SELECT COUNT(*) FROM nodes n WHERE n.id=? AND n.lease>`+s.clock()+` AND n.draining=0
AND NOT EXISTS(SELECT 1 FROM session_requirements r WHERE r.session_id=?
AND NOT EXISTS(SELECT 1 FROM node_capabilities c WHERE c.node_id=n.id AND c.capability=r.capability))`), node, current.View.ID).Scan(&valid)
	if err != nil {
		return err
	}
	if valid != 1 {
		return kernel.Fail(wire.ErrorCodeUnsupported, "target incarnation lacks a live compatible lease or retained resources")
	}
	s.fence(q, node)
	var started, opaque int
	err = q.QueryRowContext(ctx, s.q(`SELECT COUNT(*),COALESCE(SUM(CASE WHEN decision_repeatable=0 THEN 1 ELSE 0 END),0)
FROM runs WHERE session_id=? AND `+transferRuns+` AND harness_process<>''`), current.View.ID).Scan(&started, &opaque)
	if err != nil {
		return err
	}
	if opaque != 0 {
		return kernel.Fail(wire.ErrorCodeUnsupported, "a process-bound harness has unsettled work; resolve or cancel it before transfer")
	}
	if started != 0 {
		harness := current.View.Config.Harness
		capability := kernel.Capability("continuation", harness.Name, harness.Version, current.View.Config.Model.Provider)
		err = q.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM node_capabilities WHERE node_id=? AND capability=?"), node, capability).Scan(&valid)
		if err != nil {
			return err
		}
		if valid != 1 {
			return kernel.Fail(wire.ErrorCodeUnsupported, "target incarnation cannot resume the admitted harness checkpoint")
		}
	}
	return nil
}

func (s *Store) BeginTransfer(ctx context.Context, session string, request wire.TransferCreate) (out control.Transfer, err error) {
	if !kernel.Identifier(request.RequestID) || !kernel.Identifier(request.TargetNode) {
		return out, kernel.Fail(wire.ErrorCodeInvalid, "transfer identity and target must be bounded identifiers")
	}
	id := kernel.ID("transfer", session, request.RequestID)
	hash := kernel.Digest(kernel.JSON(request))
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := s.session(ctx, tx, session, true)
		if e != nil {
			return e
		}
		var prior string
		e = tx.QueryRowContext(ctx, s.q("SELECT request_hash FROM ownership_transfers WHERE session_id=? AND id=?"), session, id).Scan(&prior)
		if e == nil {
			if prior != hash {
				return kernel.Fail(wire.ErrorCodeConflict, "transfer request id has different arguments")
			}
			out, e = s.transfer(ctx, tx, session, id)
			return e
		}
		if !errors.Is(e, sql.ErrNoRows) {
			return e
		}
		if current.View.OwnerEpoch != request.OwnerEpoch || current.Owner.Node == request.TargetNode || current.View.Lifecycle != wire.LifecycleOpen {
			return kernel.Fail(wire.ErrorCodeConflict, "transfer requires the expected live owner and a different target")
		}
		if _, e = s.guard(ctx, tx, session, current.Owner, false); e != nil {
			return e
		}
		if e = s.unfrozen(ctx, tx, session, false); e != nil {
			return e
		}
		if e = s.transferTarget(ctx, tx, current, request.TargetNode); e != nil {
			return e
		}
		if _, e = tx.ExecContext(ctx, s.q(`INSERT INTO ownership_transfers
(id,session_id,request_key,request_hash,source_node,source_epoch,target_node,updated_ms)
VALUES(?,?,?,?,?,?,?,`+s.clock()+`)`), id, session, request.RequestID, hash, current.Owner.Node, current.Owner.Epoch, request.TargetNode); e != nil {
			return e
		}
		if _, e = tx.ExecContext(ctx, s.q("UPDATE sessions SET ready=1 WHERE id=?"), session); e != nil {
			return e
		}
		out, e = s.transfer(ctx, tx, session, id)
		return e
	})
	return
}

func (s *Store) PendingTransfers(ctx context.Context, node, after string, limit int) ([]control.Transfer, error) {
	if limit < 1 || limit > 16 {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "transfer work page exceeds its bound")
	}
	rows, err := s.db.QueryContext(ctx, s.q("SELECT "+transferColumns+",CASE WHEN "+transferDrained+
		" THEN 1 ELSE 0 END,CASE WHEN "+transferArchived+" THEN 1 ELSE 0 END,"+s.transferTargetLive()+`
FROM ownership_transfers t WHERE t.phase IN ('draining','verifying') AND (t.source_node=? OR t.target_node=?) AND t.id>?
ORDER BY t.id LIMIT ?`), node, node, after, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []control.Transfer{}
	for rows.Next() {
		value, err := scanTransfer(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, value)
	}
	return out, rows.Err()
}

func (s *Store) transferGuard(ctx context.Context, tx *sql.Tx, session, id, node string) (control.Transfer, control.Session, error) {
	current, err := s.session(ctx, tx, session, true)
	if err != nil {
		return control.Transfer{}, current, err
	}
	value, err := s.transfer(ctx, tx, session, id)
	if err != nil {
		return value, current, err
	}
	if value.View.Phase != wire.TransferPhaseDraining && value.View.Phase != wire.TransferPhaseVerifying {
		return value, current, kernel.Fail(wire.ErrorCodeConflict, "ownership transfer is already settled")
	}
	if current.Owner.Node != value.View.SourceNode || current.View.OwnerEpoch != value.View.SourceEpoch ||
		(node != value.View.SourceNode && node != value.View.TargetNode) {
		return value, current, kernel.Fail(wire.ErrorCodeConflict, "transfer owner or assigned participant changed")
	}
	if _, err = s.guard(ctx, tx, session, current.Owner, false); err != nil {
		return value, current, err
	}
	if current.View.Lifecycle != wire.LifecycleOpen {
		return value, current, kernel.Fail(wire.ErrorCodeConflict, "transfer session is no longer open")
	}
	err = s.transferTarget(ctx, tx, current, value.View.TargetNode)
	return value, current, err
}

func (s *Store) SealTransfer(ctx context.Context, session string, owner control.Owner, id string, record wire.Record) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		value, current, err := s.transferGuard(ctx, tx, session, id, owner.Node)
		if err != nil {
			return err
		}
		if current.Owner != owner || value.View.SourceNode != owner.Node || value.View.Phase != wire.TransferPhaseDraining || !value.Drained {
			return kernel.Fail(wire.ErrorCodeConflict, "transfer has not drained its original work and allocations")
		}
		expected := map[string]any{"format": "brain-ownership-checkpoint/1", "transfer_id": id,
			"source_epoch": value.View.SourceEpoch, "target_node": value.View.TargetNode}
		if record.SessionID != session || record.Sequence != current.View.Sequence || record.Kind != "ownership.checkpoint" ||
			!kernel.EqualJSON(record.Data, expected) || record.RunID != nil || record.OperationID != nil {
			return kernel.Fail(wire.ErrorCodeConflict, "transfer checkpoint does not match its exact frozen boundary")
		}
		digest := kernel.Digest(kernel.JSON(record))
		var written int
		err = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM allocations WHERE session_id=? AND sequence=? AND status='written' AND digest=?"),
			session, record.Sequence, digest).Scan(&written)
		if err != nil {
			return err
		}
		if written != 1 {
			return kernel.Fail(wire.ErrorCodeConflict, "transfer checkpoint has no matching written evidence")
		}
		_, err = tx.ExecContext(ctx, s.q("UPDATE ownership_transfers SET phase='verifying',through_sequence=?,checkpoint_digest=?,updated_ms="+s.clock()+" WHERE id=?"),
			record.Sequence, digest, id)
		return err
	})
}

func (s *Store) TransferRuns(ctx context.Context, session, id, after string, limit int) ([]control.Run, error) {
	value, err := s.Transfer(ctx, session, id)
	if err != nil {
		return nil, err
	}
	if value.View.Phase != wire.TransferPhaseVerifying || value.Cursor != after || limit < 1 || limit > 4 {
		return nil, kernel.Fail(wire.ErrorCodeConflict, "transfer verification cursor changed")
	}
	rows, err := s.pageRows(ctx, runColumns, "runs r JOIN commands c ON c.id=r.command_id",
		"r.session_id=? AND r.id>? AND r."+transferRuns, "r.id", s.runBytes(), limit, session, after)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []control.Run{}
	for rows.Next() {
		value, err := scanRun(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, value)
	}
	return out, rows.Err()
}

func (s *Store) VerifyTransferTail(ctx context.Context, session, id, node string, reference wire.RecordReference) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		value, current, err := s.transferGuard(ctx, tx, session, id, node)
		if err != nil {
			return err
		}
		if value.View.Phase != wire.TransferPhaseVerifying || value.View.TargetNode != node || !value.Archived ||
			current.View.Sequence != value.View.Through || value.View.Checkpoint == nil || *value.View.Checkpoint != reference {
			return kernel.Fail(wire.ErrorCodeConflict, "target cannot attest a different or uncovered checkpoint")
		}
		_, err = tx.ExecContext(ctx, s.q("UPDATE ownership_transfers SET tail_verified=1,updated_ms="+s.clock()+" WHERE id=?"), id)
		return err
	})
}

func (s *Store) VerifyTransferRuns(ctx context.Context, session, id, node, after string, verified []string) (out control.Transfer, err error) {
	if len(verified) > 4 {
		return out, kernel.Fail(wire.ErrorCodeInvalid, "transfer proof page exceeds its bound")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		value, current, e := s.transferGuard(ctx, tx, session, id, node)
		if e != nil {
			return e
		}
		if value.View.Phase != wire.TransferPhaseVerifying || value.View.TargetNode != node || value.Cursor != after ||
			!value.TailVerified || !value.Archived || current.View.Sequence != value.View.Through {
			return kernel.Fail(wire.ErrorCodeConflict, "transfer checkpoint, proof cursor or archive coverage changed")
		}
		rows, e := tx.QueryContext(ctx, s.q("SELECT id FROM runs WHERE session_id=? AND id>? AND "+transferRuns+" ORDER BY id LIMIT ?"),
			session, after, max(1, len(verified)))
		if e != nil {
			return e
		}
		ids := []string{}
		for rows.Next() {
			var item string
			if e = rows.Scan(&item); e != nil {
				rows.Close()
				return e
			}
			ids = append(ids, item)
		}
		e = rows.Err()
		rows.Close()
		if e != nil {
			return e
		}
		if !slices.Equal(ids, verified) {
			return kernel.Fail(wire.ErrorCodeConflict, "transfer proof skipped or changed a retained run")
		}
		if len(ids) != 0 {
			_, e = tx.ExecContext(ctx, s.q("UPDATE ownership_transfers SET run_cursor=?,verified_runs=verified_runs+?,updated_ms="+s.clock()+" WHERE id=?"),
				ids[len(ids)-1], len(ids), id)
		} else {
			if current.Owner.Epoch == math.MaxInt64 {
				return kernel.Fail(wire.ErrorCodeOverloaded, "owner epoch exhausted")
			}
			if e = requireOne(tx.ExecContext(ctx, s.q(`UPDATE sessions SET owner=?,epoch=epoch+1,ready=1,ready_ms=`+s.clock()+`
WHERE id=? AND owner=? AND epoch=? AND sequence=?`), node, session, value.View.SourceNode, current.Owner.Epoch, value.View.Through)); e != nil {
				return e
			}
			_, e = tx.ExecContext(ctx, s.q("UPDATE ownership_transfers SET phase='transferred',updated_ms="+s.clock()+" WHERE id=?"), id)
		}
		if e != nil {
			return e
		}
		out, e = s.transfer(ctx, tx, session, id)
		return e
	})
	return
}

func (s *Store) CancelTransfer(ctx context.Context, session, id string) (out control.Transfer, err error) {
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := s.session(ctx, tx, session, true); e != nil {
			return e
		}
		value, e := s.transfer(ctx, tx, session, id)
		if e != nil {
			return e
		}
		if value.View.Phase == wire.TransferPhaseTransferred {
			return kernel.Fail(wire.ErrorCodeConflict, "completed ownership transfer cannot be undone")
		}
		if value.View.Phase == wire.TransferPhaseDraining || value.View.Phase == wire.TransferPhaseVerifying {
			if _, e = tx.ExecContext(ctx, s.q("UPDATE ownership_transfers SET phase='cancelled',updated_ms="+s.clock()+" WHERE id=?"), id); e != nil {
				return e
			}
			if _, e = tx.ExecContext(ctx, s.q("UPDATE sessions SET ready=1 WHERE id=?"), session); e != nil {
				return e
			}
		}
		out, e = s.transfer(ctx, tx, session, id)
		return e
	})
	return
}

func (s *Store) FailTransfer(ctx context.Context, session, id, node string, failure wire.ErrorResponse) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := s.session(ctx, tx, session, true); err != nil {
			return err
		}
		value, err := s.transfer(ctx, tx, session, id)
		if err != nil {
			return err
		}
		if node != value.View.SourceNode && node != value.View.TargetNode {
			return kernel.Fail(wire.ErrorCodeForbidden, "only an assigned transfer node may record its failure")
		}
		if err = requireOne(tx.ExecContext(ctx, s.q("UPDATE ownership_transfers SET phase='failed',error=?,updated_ms="+s.clock()+
			" WHERE id=? AND "+transferActive+" AND EXISTS(SELECT 1 FROM nodes WHERE id=? AND lease>"+s.clock()+" AND draining=0)"),
			string(kernel.JSON(failure)), id, node)); err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, s.q("UPDATE sessions SET ready=1 WHERE id=?"), session)
		return err
	})
}
