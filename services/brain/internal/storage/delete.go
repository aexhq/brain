package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
)

func (s *SQLite) Deleted(ctx context.Context, id string) (string, error) {
	var tenant string
	err := s.db.QueryRowContext(ctx, `SELECT tenant FROM sessions WHERE id=? AND json_extract(data,'$.status')='deleted'`, core.SessionKey(core.Tenant(ctx), id)).Scan(&tenant)
	if errors.Is(err, sql.ErrNoRows) {
		return "", nil
	}
	return tenant, err
}

// The minimal tombstone prevents a retained checkpoint from restoring this ID.
func (s *SQLite) Delete(ctx context.Context, session core.Session) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	data, err := json.Marshal(core.Session{ID: session.ID, Tenant: session.Tenant, Status: "deleted"})
	if err != nil {
		return err
	}
	r, err := tx.ExecContext(ctx, `UPDATE sessions SET data=? WHERE id=? AND tenant=?`, data, core.SessionKey(session.Tenant, session.ID), session.Tenant)
	if err != nil {
		return err
	}
	n, err := r.RowsAffected()
	if err != nil {
		return err
	}
	if n != 1 {
		return core.E(core.NotFound, "session not found")
	}
	if _, err := tx.ExecContext(ctx, `DELETE FROM credentials WHERE session=? AND tenant=?`, core.SessionKey(session.Tenant, session.ID), session.Tenant); err != nil {
		return err
	}
	return tx.Commit()
}
