package sqlcontrol

import (
	"context"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestReceiptCannotBindUnwrittenOrMismatchedEvidence(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	op := testOperation(t, store, session, "direct-receipt")
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "claim"); err != nil {
		t.Fatal(err)
	}
	sequence, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	receipt := wire.Receipt{RequestID: "original", Outcome: wire.OutcomeCompleted, Result: kernel.JSON("result")}
	digest := kernel.Digest(kernel.JSON(receipt))
	if _, err = store.RecordReceipt(ctx, session.View.ID, op.View.ID, receipt, sequence, digest); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("unwritten position accepted as outcome evidence: %v", err)
	}
	prior, err := store.Operation(ctx, session.View.ID, op.View.ID)
	if err != nil || prior.View.Status != wire.OperationStatusDispatched {
		t.Fatalf("invalid evidence settled the operation: %+v %v", prior, err)
	}
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, digest, 64); err != nil {
		t.Fatal(err)
	}
	if _, err = store.RecordReceipt(ctx, session.View.ID, op.View.ID, receipt, sequence, "different"); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("mismatched bytes accepted as outcome evidence: %v", err)
	}
	if _, err = store.RecordReceipt(ctx, session.View.ID, op.View.ID, receipt, sequence, digest); err != nil {
		t.Fatal(err)
	}
	if count, err := store.ExpireOperations(ctx, 8); err != nil || count != 0 {
		t.Fatalf("settled receipt was reclassified by expiry: %d %v", count, err)
	}
}

func TestSessionDeadlineIsCheckedAtDispatch(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	op := testOperation(t, store, session, "dispatch-deadline")
	session.View.Config.Limits.DeadlineMs = kernel.Ptr(uint64(1))
	if _, err := store.db.ExecContext(ctx, store.q("UPDATE sessions SET config=? WHERE id=?"), string(kernel.JSON(session.View.Config)), session.View.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "claim"); kernel.Response(err).Code != wire.ErrorCodeForbidden {
		t.Fatalf("expired session issued an execution permit: %v", err)
	}
	value, err := store.Operation(ctx, session.View.ID, op.View.ID)
	if err != nil || value.View.Status != wire.OperationStatusPrepared {
		t.Fatalf("rejected claim changed the operation: %+v %v", value, err)
	}
}
