package sqlcontrol

import (
	"context"
	"testing"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestRevokedAuthenticationSnapshotCannotCommitOrClaim(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	op := testOperation(t, store, session, "unclaimed")
	now, err := store.Clock(ctx)
	if err != nil {
		t.Fatal(err)
	}
	grant, err := store.Grant(ctx, session.View.ID, wire.ParticipantGrant{RequestID: "grant", Subject: "browser",
		Permissions: []string{"input:write", "operation:claim"}, ExpiresMs: uint64(now + 60000)})
	if err != nil {
		t.Fatal(err)
	}
	authenticated := control.WithAuthority(ctx, control.Authority{Participant: grant.ID, Session: grant.Session, Tenant: grant.Tenant})
	if err = store.RevokeParticipant(ctx, session.View.ID, grant.ID); err != nil {
		t.Fatal(err)
	}
	if _, err = store.ClaimDispatch(authenticated, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "stale-auth"); kernel.Response(err).Code != wire.ErrorCodeUnauthorized {
		t.Fatalf("stale authentication minted dispatch permission: %v", err)
	}
	if _, err = store.PutDocument(authenticated, session.View.ID, "state", "stale", wire.DocumentUpdate{RequestID: "stale", Revision: "0", Value: kernel.JSON(true)}); kernel.Response(err).Code != wire.ErrorCodeUnauthorized {
		t.Fatalf("stale authentication wrote application state: %v", err)
	}
	value, err := store.Operation(ctx, session.View.ID, op.View.ID)
	if err != nil || value.View.Status != wire.OperationStatusPrepared {
		t.Fatalf("denied claim changed operation: %+v %v", value, err)
	}
}

func TestParticipantMayRevokeItsOwnAuthority(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	now, _ := store.Clock(ctx)
	grant, err := store.Grant(ctx, session.View.ID, wire.ParticipantGrant{RequestID: "self", Subject: "browser",
		Permissions: []string{"participant:revoke"}, ExpiresMs: uint64(now + 60000)})
	if err != nil {
		t.Fatal(err)
	}
	authenticated := control.WithAuthority(ctx, control.Authority{Participant: grant.ID, Session: grant.Session, Tenant: grant.Tenant})
	if err = store.RevokeParticipant(authenticated, session.View.ID, grant.ID); err != nil {
		t.Fatal(err)
	}
	revoked, err := store.Participant(ctx, grant.ID)
	if err != nil || !revoked.Revoked {
		t.Fatalf("self revocation failed: %+v %v", revoked, err)
	}
}
