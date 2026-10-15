package sqlcontrol

import (
	"context"
	"strconv"
	"testing"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestRetainedRecordReconciliationPreservesOriginalClaimsAndPositions(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	operation := testOperation(t, store, session, "original-effect")
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, operation.View.ID, session.Owner.Node, "first-claim"); err != nil {
		t.Fatal(err)
	}
	written, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	unused, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	if err = store.SealUnused(ctx, session.View.ID, session.Owner, unused); err != nil {
		t.Fatal(err)
	}
	absent, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	reference := wire.RecordReference{SessionID: session.View.ID, Sequence: strconv.FormatInt(written, 10), Digest: kernel.Digest([]byte("original-frame"))}
	for _, node := range []string{"successor", "returning-volume"} {
		if err = store.RegisterNode(ctx, node, 60000); err != nil {
			t.Fatal(err)
		}
	}
	if err = store.ReconcileRecord(ctx, "returning-volume", session.Owner, reference, 100); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("a different node confirmed a still-live reserved allocation: %v", err)
	}
	if err = store.RetireNode(ctx, session.Owner.Node); err != nil {
		t.Fatal(err)
	}
	successor, err := store.ClaimOwner(ctx, session.View.ID, "successor")
	if err != nil {
		t.Fatal(err)
	}
	previous, err := store.Operation(ctx, session.View.ID, operation.View.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, original := range []control.Owner{{Node: session.Owner.Node, Epoch: successor.Epoch}, {Node: "different-writer", Epoch: session.Owner.Epoch}} {
		if err = store.ReconcileRecord(ctx, "returning-volume", original, reference, 100); kernel.Response(err).Code != wire.ErrorCodeConflict {
			t.Fatalf("different original allocation metadata was accepted: %v", err)
		}
	}
	sealed := reference
	sealed.Sequence = strconv.FormatInt(unused, 10)
	if err = store.ReconcileRecord(ctx, "returning-volume", session.Owner, sealed, 100); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("positively unused position became a record: %v", err)
	}
	for range 2 {
		if err = store.ReconcileRecord(ctx, "returning-volume", session.Owner, reference, 100); err != nil {
			t.Fatal(err)
		}
	}
	changed := reference
	changed.Digest = kernel.Digest([]byte("different-frame"))
	if err = store.ReconcileRecord(ctx, "returning-volume", session.Owner, changed, 100); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("conflicting historical bytes replaced known evidence: %v", err)
	}
	if err = store.ReconcileRecord(ctx, "returning-volume", session.Owner, reference, 101); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("a repeated digest silently changed its byte count: %v", err)
	}
	allocations, high, err := store.Allocations(ctx, session.View.ID, written-1, 16)
	if err != nil || high != absent || len(allocations) != 3 || allocations[0].Status != "written" ||
		allocations[1].Status != "unused" || allocations[2].Status != "unresolved" {
		t.Fatalf("reconciliation changed positions or inferred absent bytes: %+v %d %v", allocations, high, err)
	}
	location, err := store.LocateRecord(ctx, session.View.ID, written)
	if err != nil || location.Node != "returning-volume" || location.CurrentOwner != successor.Node {
		t.Fatalf("retained routing displaced session authority: %+v %v", location, err)
	}
	after, err := store.Operation(ctx, session.View.ID, operation.View.ID)
	if err != nil || string(kernel.JSON(previous)) != string(kernel.JSON(after)) {
		t.Fatalf("historical evidence changed the original execution: %+v %v", after, err)
	}
	if _, err = store.ClaimDispatch(ctx, session.View.ID, successor, operation.View.ID, successor.Node, "repeated-claim"); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("reconciliation minted another dispatch permit: %v", err)
	}
	current, err := store.Session(ctx, session.Tenant, session.View.ID)
	if err != nil || current.Owner != successor || current.View.Sequence != strconv.FormatInt(high, 10) {
		t.Fatalf("reconciliation changed ownership or the high watermark: %+v %v", current, err)
	}
	if err = store.RetireNode(ctx, "returning-volume"); err != nil {
		t.Fatal(err)
	}
	if err = store.ReconcileRecord(ctx, "returning-volume", session.Owner, reference, 100); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("an expired reporting incarnation changed history: %v", err)
	}
}

func TestOriginalLiveWriterCanReconcileOnlyItsCommittedRecord(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	sequence, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	reference := wire.RecordReference{SessionID: session.View.ID, Sequence: strconv.FormatInt(sequence, 10), Digest: kernel.Digest([]byte("durable-frame"))}
	if err = store.ReconcileRecord(ctx, session.Owner.Node, session.Owner, reference, 64); err != nil {
		t.Fatal(err)
	}
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, reference.Digest, 64); err != nil {
		t.Fatalf("the original lost acknowledgement could not be reconciled: %v", err)
	}
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, reference.Digest, 65); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("written evidence changed its confirmed byte count: %v", err)
	}
	if _, err = store.Delete(ctx, session.View.ID); err != nil {
		t.Fatal(err)
	}
	if err = store.ReconcileRecord(ctx, session.Owner.Node, session.Owner, reference, 64); kernel.Response(err).Code != wire.ErrorCodeNotFound {
		t.Fatalf("deleted history accepted a new location or body fact: %v", err)
	}
}
