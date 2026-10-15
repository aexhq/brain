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
	if authority.Participant == "" && (authority.Expires <= 0 || !kernel.Identifier(authority.Subject) || !kernel.Identifier(authority.Tenant)) {
		return kernel.Fail(wire.ErrorCodeUnauthorized, "identity authority is incomplete")
	}
	if authority.Expires > 0 {
		var live int
		if err := tx.QueryRowContext(ctx, s.q("SELECT CASE WHEN ?>"+s.clock()+" THEN 1 ELSE 0 END"), authority.Expires).Scan(&live); err != nil {
			return err
		}
		if live != 1 {
			return kernel.Fail(wire.ErrorCodeUnauthorized, "identity authority expired before commit")
		}
	}
	if !expiryOnly && authority.Session != "" {
		session, err := s.session(ctx, tx, authority.Session, true)
		if err != nil || session.Tenant != authority.Tenant || session.View.Lifecycle == wire.LifecycleDeleted {
			return kernel.Fail(wire.ErrorCodeUnauthorized, "participant session authority is no longer valid")
		}
	}
	if authority.Participant == "" {
		return nil
	}
	predicate := ""
	if !expiryOnly {
		predicate = " AND p.revoked=0"
	}
	var count int
	if err := tx.QueryRowContext(ctx, s.q("SELECT COUNT(*) FROM participants p WHERE p.id=? AND p.session_id=? AND p.expires_ms>"+
		s.clock()+predicate+" AND NOT EXISTS("+s.invalidParticipantAncestor(expiryOnly)+")"),
		authority.Participant, authority.Session).Scan(&count); err != nil {
		return err
	}
	if count != 1 {
		return kernel.Fail(wire.ErrorCodeUnauthorized, "participant authority was revoked or expired before commit")
	}
	return nil
}

func (s *Store) invalidParticipantAncestor(expiryOnly bool) string {
	predicate := ""
	if !expiryOnly {
		predicate = " OR a.revoked<>0"
	}
	return `SELECT 1 FROM participant_ancestors l LEFT JOIN participants a ON a.id=l.ancestor
WHERE l.descendant=p.id AND (a.id IS NULL OR a.session_id<>p.session_id OR a.expires_ms<=` + s.clock() + predicate + ")"
}

func (s *Store) grantAuthority(ctx context.Context, tx *sql.Tx, session control.Session, request wire.ParticipantGrant) (string, error) {
	authority := control.RequestAuthority(ctx)
	if authority == nil {
		return "", nil
	}
	if authority.Tenant != session.Tenant || (authority.Session != "" && authority.Session != session.View.ID) {
		return "", kernel.Fail(wire.ErrorCodeForbidden, "participant grant exceeds its tenant or session authority")
	}
	subject, permissions, expires := authority.Subject, authority.Permissions, authority.Expires
	if authority.Participant != "" {
		var body string
		if err := tx.QueryRowContext(ctx, s.q("SELECT subject,permissions,expires_ms FROM participants WHERE id=? AND session_id=?"),
			authority.Participant, session.View.ID).Scan(&subject, &body, &expires); err != nil {
			return "", notFound(err)
		}
		var err error
		permissions, err = decode[[]string](body)
		if err != nil {
			return "", err
		}
	}
	if request.Subject != subject || !kernel.PermissionSubset(request.Permissions, permissions) ||
		expires <= 0 || request.ExpiresMs > uint64(expires) {
		return "", kernel.Fail(wire.ErrorCodeForbidden, "participant cannot amplify permissions, lifetime or source identity")
	}
	return authority.Participant, nil
}

func (s *Store) authoritySubject(ctx context.Context, tx *sql.Tx) (string, error) {
	authority := control.RequestAuthority(ctx)
	if authority == nil {
		return "", nil
	}
	if authority.Participant == "" {
		return authority.Subject, nil
	}
	var subject string
	err := tx.QueryRowContext(ctx, s.q("SELECT subject FROM participants WHERE id=? AND session_id=?"), authority.Participant, authority.Session).Scan(&subject)
	return subject, notFound(err)
}
