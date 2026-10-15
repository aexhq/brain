package sqlcontrol

import (
	"context"
	"database/sql"
	"encoding/hex"
	"strings"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Store) ReconcileRecord(ctx context.Context, reporter string, original control.Owner, record wire.RecordReference, bytes int64) error {
	sequence, err := kernel.Sequence(record.Sequence)
	_, hashError := hex.DecodeString(record.Digest)
	if err != nil || sequence < 1 || !kernel.Identifier(record.SessionID) || !kernel.Identifier(reporter) ||
		!kernel.Identifier(original.Node) || original.Epoch < 1 || bytes < 1 || bytes > 256<<10 ||
		hashError != nil || len(record.Digest) != 64 || strings.ToLower(record.Digest) != record.Digest {
		return kernel.Fail(wire.ErrorCodeInvalid, "retained record evidence is invalid or exceeds its bound")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := s.session(ctx, tx, record.SessionID, true)
		if err != nil {
			return err
		}
		if session.View.Lifecycle == wire.LifecycleDeleted {
			return kernel.Fail(wire.ErrorCodeNotFound, "deleted history cannot be reconciled")
		}
		var live int
		if err = tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM nodes WHERE id=? AND lease>"+s.clock()+" AND draining=0"), reporter).Scan(&live); err != nil {
			return err
		}
		if live != 1 {
			return kernel.Fail(wire.ErrorCodeConflict, "retained evidence needs a live reporting incarnation")
		}
		s.fence(tx, reporter)
		var writer control.Owner
		var status, digest string
		var size int64
		if err = tx.QueryRowContext(ctx, s.q("SELECT owner,epoch,status,digest,bytes FROM allocations WHERE session_id=? AND sequence=?"),
			record.SessionID, sequence).Scan(&writer.Node, &writer.Epoch, &status, &digest, &size); err != nil {
			return notFound(err)
		}
		if writer != original {
			return kernel.Fail(wire.ErrorCodeConflict, "retained bytes do not belong to the original allocation")
		}
		switch status {
		case "written":
			if digest != record.Digest || size != bytes {
				return kernel.Fail(wire.ErrorCodeCorrupt, "retained bytes conflict with known written evidence")
			}
		case "reserved":
			if reporter != original.Node || session.Owner != original {
				return kernel.Fail(wire.ErrorCodeConflict, "another live allocation cannot be confirmed by a replacement")
			}
		case "unresolved":
			if original.Epoch >= session.Owner.Epoch {
				return kernel.Fail(wire.ErrorCodeConflict, "unresolved evidence has no preceding ownership epoch")
			}
		default:
			return kernel.Fail(wire.ErrorCodeConflict, "positively unused positions cannot become records")
		}
		// This confirms original bytes only. It never changes a command, run,
		// ownership epoch or operation's execution claim.
		return requireOne(tx.ExecContext(ctx, s.q(`UPDATE allocations SET status='written',digest=?,bytes=?,recovered_by=?
WHERE session_id=? AND sequence=? AND owner=? AND epoch=? AND status=?`),
			record.Digest, bytes, reporter, record.SessionID, sequence, original.Node, original.Epoch, status))
	})
}
