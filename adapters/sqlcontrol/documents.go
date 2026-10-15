package sqlcontrol

import (
	"context"
	"database/sql"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Store) documentQuota(ctx context.Context, tx *sql.Tx, session, scope, key string, value []byte) error {
	if len(value) > 65536 {
		return kernel.Fail(wire.ErrorCodeInvalid, "document exceeds its byte bound")
	}
	var count, size, previous, exists int64
	err := tx.QueryRowContext(ctx, s.q(`SELECT COUNT(*),COALESCE(SUM(`+s.bytes("value")+`),0),
COALESCE(MAX(CASE WHEN scope=? AND key=? THEN `+s.bytes("value")+` ELSE 0 END),0),
COALESCE(MAX(CASE WHEN scope=? AND key=? THEN 1 ELSE 0 END),0) FROM documents WHERE session_id=?`),
		scope, key, scope, key, session).Scan(&count, &size, &previous, &exists)
	if err != nil {
		return err
	}
	if size-previous+int64(len(value)) > 4<<20 || (exists == 0 && count >= 1024) {
		return kernel.Fail(wire.ErrorCodeOverloaded, "state and memory quota reached")
	}
	return nil
}
