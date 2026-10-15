package history

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"strconv"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

const confirmationBatch = 32

type confirmationCursor struct {
	session  string
	sequence int64
}

func (s *Store) Confirm(ctx context.Context, record wire.Record) error {
	sequence, err := kernel.Sequence(record.Sequence)
	if err != nil {
		return err
	}
	_, err = s.shard(record.SessionID).ExecContext(ctx, "UPDATE records SET confirmed=1 WHERE session_id=? AND sequence=? AND digest=?",
		record.SessionID, sequence, kernel.Digest(kernel.JSON(record)))
	return err
}

func confirmations(ctx context.Context, db *sql.DB, session string, after confirmationCursor) ([]confirmationCursor, error) {
	predicate := ""
	arguments := []any{after.session, after.session, after.sequence}
	if session != "" {
		predicate = " AND session_id=?"
		arguments = append(arguments, session)
	}
	rows, err := db.QueryContext(ctx, `SELECT session_id,sequence FROM records WHERE confirmed=0 AND original_writer<>''
AND (session_id>? OR (session_id=? AND sequence>?))`+predicate+" ORDER BY session_id,sequence LIMIT ?", append(arguments, confirmationBatch)...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []confirmationCursor{}
	for rows.Next() {
		var value confirmationCursor
		if err = rows.Scan(&value.session, &value.sequence); err != nil {
			return nil, err
		}
		out = append(out, value)
	}
	return out, rows.Err()
}

func (s *Store) reconcileRecord(ctx context.Context, db *sql.DB, position confirmationCursor) error {
	var body []byte
	var digest string
	var original control.Owner
	var confirmed int
	err := db.QueryRowContext(ctx, `SELECT CASE WHEN LENGTH(body)<=? THEN body ELSE NULL END,digest,original_writer,original_epoch,confirmed FROM records
WHERE session_id=? AND sequence=?`, MaxFrame, position.session, position.sequence).
		Scan(&body, &digest, &original.Node, &original.Epoch, &confirmed)
	if errors.Is(err, sql.ErrNoRows) || (err == nil && confirmed != 0) {
		return nil
	}
	if err != nil {
		return err
	}
	var record wire.Record
	if len(body) > MaxFrame || json.Unmarshal(body, &record) != nil || record.SessionID != position.session ||
		record.Sequence != strconv.FormatInt(position.sequence, 10) || original.Node == "" || original.Epoch < 1 ||
		kernel.Digest(body) != digest || kernel.Digest(kernel.JSON(record)) != digest {
		return kernel.Fail(wire.ErrorCodeCorrupt, "retained confirmation bytes or original allocation metadata are corrupt")
	}
	reference := wire.RecordReference{SessionID: position.session, Sequence: record.Sequence, Digest: digest}
	if err = s.control.ReconcileRecord(ctx, s.node, original, reference, int64(len(body))); err != nil {
		return err
	}
	if err = s.Confirm(ctx, record); err != nil {
		return err
	}
	s.notify(position.session)
	return nil
}

func (s *Store) ReconcileSession(ctx context.Context, session string) error {
	db := s.shard(session)
	pending, err := confirmations(ctx, db, session, confirmationCursor{})
	if err != nil {
		return err
	}
	for _, position := range pending {
		if err = s.reconcileRecord(ctx, db, position); err != nil {
			return err
		}
	}
	if len(pending) == confirmationBatch {
		return kernel.Fail(wire.ErrorCodeOverloaded, "retained history confirmation continues in bounded batches before new work")
	}
	return nil
}

func (s *Store) reconcilePending(ctx context.Context, db *sql.DB, index int) error {
	pending, err := confirmations(ctx, db, "", s.confirmCursors[index])
	if err != nil {
		return err
	}
	if len(pending) == 0 {
		s.confirmCursors[index] = confirmationCursor{}
	}
	for _, position := range pending {
		s.confirmCursors[index] = position
		if err = s.reconcileRecord(ctx, db, position); err != nil {
			code := kernel.Response(err).Code
			if code != wire.ErrorCodeConflict && code != wire.ErrorCodeNotFound {
				return err
			}
		}
	}
	return nil
}
