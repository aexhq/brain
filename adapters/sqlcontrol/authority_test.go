package sqlcontrol

import (
	"context"
	"database/sql"
	"fmt"
	"testing"
	"time"

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

func TestIdentityExpiryRollsBackACommitThatStartedWhileAuthorized(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	operation := testOperation(t, store, session, "identity-expiry")
	now, _ := store.Clock(ctx)
	authority := control.WithAuthority(ctx, control.Authority{Session: session.View.ID, Tenant: session.Tenant,
		Subject: "identity:browser", Permissions: []string{"operation:claim"}, Expires: now + 600})
	err := store.transaction(authority, func(tx *sql.Tx) error {
		if _, err := tx.ExecContext(authority, store.q("UPDATE operations SET status='dispatched',claimant='expired-request' WHERE id=?"), operation.View.ID); err != nil {
			return err
		}
		time.Sleep(650 * time.Millisecond)
		return nil
	})
	if kernel.Response(err).Code != wire.ErrorCodeUnauthorized {
		t.Fatalf("expired transaction committed: %v", err)
	}
	retained, err := store.Operation(ctx, session.View.ID, operation.View.ID)
	if err != nil || retained.View.Status != wire.OperationStatusPrepared || retained.Claimant != "" {
		t.Fatalf("expired authority consumed the one-use claim: %+v %v", retained, err)
	}
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, operation.View.ID, session.Owner.Node, "valid-original-claim"); err != nil {
		t.Fatal(err)
	}
}

func TestParticipantAncestorsAttenuateAndRevokeWithoutUpwardAuthority(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	now, _ := store.Clock(ctx)
	permissions := []string{"participant:grant", "participant:revoke", "session:read", "state:write"}
	root, err := store.Grant(ctx, session.View.ID, wire.ParticipantGrant{RequestID: "root", Subject: "browser",
		Permissions: permissions, ExpiresMs: uint64(now + 60000)})
	if err != nil {
		t.Fatal(err)
	}
	authenticate := func(value control.Participant) context.Context {
		return control.WithAuthority(ctx, control.Authority{Participant: value.ID, Session: value.Session, Tenant: value.Tenant})
	}
	childRequest := wire.ParticipantGrant{RequestID: "child", Subject: "browser", Permissions: permissions, ExpiresMs: uint64(root.Expires - 1)}
	for _, invalid := range []wire.ParticipantGrant{
		{RequestID: "longer", Subject: "browser", Permissions: permissions, ExpiresMs: uint64(root.Expires + 1)},
		{RequestID: "other-source", Subject: "someone-else", Permissions: permissions, ExpiresMs: childRequest.ExpiresMs},
		{RequestID: "more-power", Subject: "browser", Permissions: []string{"*"}, ExpiresMs: childRequest.ExpiresMs},
	} {
		if _, err := store.Grant(authenticate(root), session.View.ID, invalid); kernel.Response(err).Code != wire.ErrorCodeForbidden {
			t.Fatalf("amplified participant authority: %v", err)
		}
	}
	child, err := store.Grant(authenticate(root), session.View.ID, childRequest)
	if err != nil || child.Parent != root.ID {
		t.Fatalf("child parent was not retained: %+v %v", child, err)
	}
	siblingRequest := childRequest
	siblingRequest.RequestID = "sibling"
	sibling, err := store.Grant(authenticate(root), session.View.ID, siblingRequest)
	if err != nil {
		t.Fatal(err)
	}
	for _, target := range []string{root.ID, sibling.ID} {
		if err := store.RevokeParticipant(authenticate(child), session.View.ID, target); kernel.Response(err).Code != wire.ErrorCodeForbidden {
			t.Fatalf("child revoked upward or sibling authority: %v", err)
		}
	}
	if _, err := store.Grant(ctx, session.View.ID, childRequest); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("idempotency reused a different grant parent: %v", err)
	}
	leaf := child
	for depth := 2; depth <= 8; depth++ {
		childRequest.RequestID = fmt.Sprintf("depth-%d", depth)
		leaf, err = store.Grant(authenticate(leaf), session.View.ID, childRequest)
		if err != nil {
			t.Fatal(err)
		}
	}
	childRequest.RequestID = "too-deep"
	if _, err = store.Grant(authenticate(leaf), session.View.ID, childRequest); kernel.Response(err).Code != wire.ErrorCodeOverloaded {
		t.Fatalf("participant lineage exceeded its depth bound: %v", err)
	}
	captured := authenticate(leaf)
	if err = store.RevokeParticipant(authenticate(root), session.View.ID, root.ID); err != nil {
		t.Fatal(err)
	}
	revoked, err := store.Participant(ctx, leaf.ID)
	if err != nil || !revoked.Revoked {
		t.Fatalf("ancestor revocation did not invalidate leaf: %+v %v", revoked, err)
	}
	if _, err = store.PutDocument(captured, session.View.ID, "state", "stale-descendant",
		wire.DocumentUpdate{RequestID: "stale-descendant", Revision: "0", Value: kernel.JSON(true)}); kernel.Response(err).Code != wire.ErrorCodeUnauthorized {
		t.Fatalf("captured descendant authority committed after ancestor revocation: %v", err)
	}
}
