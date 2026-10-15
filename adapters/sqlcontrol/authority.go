package sqlcontrol

import (
	"context"
	"database/sql"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Store) authorize(ctx context.Context, tx *sql.Tx, expiryOnly bool) error {
	authority := control.RequestAuthority(ctx)
	if authority == nil {
		return nil
	}
	if !expiryOnly {
		session, err := s.session(ctx, tx, authority.Session, true)
		if err != nil || session.Tenant != authority.Tenant || session.View.Lifecycle == wire.LifecycleDeleted {
			return kernel.Fail(wire.ErrorCodeUnauthorized, "participant session authority is no longer valid")
		}
	}
	predicate := ""
	if !expiryOnly {
		predicate = " AND revoked=0"
	}
	var count int
	if err := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM participants WHERE id=? AND session_id=? AND expires_ms>"+s.clock()+predicate),
		authority.Participant, authority.Session).Scan(&count); err != nil {
		return err
	}
	if count != 1 {
		return kernel.Fail(wire.ErrorCodeUnauthorized, "participant authority was revoked or expired before commit")
	}
	return nil
}
