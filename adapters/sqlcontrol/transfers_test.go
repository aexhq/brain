package sqlcontrol

import (
	"context"
	"database/sql"
	"strconv"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func transferNode(t *testing.T, store *Store, session control.Session) string {
	t.Helper()
	node := "transfer-target"
	ctx := context.Background()
	if err := store.RegisterNode(ctx, node, 60000); err != nil {
		t.Fatal(err)
	}
	harness := session.View.Config.Harness
	if err := store.ConfigureNode(ctx, node, "http://target", kernel.Digest([]byte("custody")), []string{
		kernel.HarnessCapability(harness, session.View.Config.Model.Provider),
		kernel.Capability("continuation", harness.Name, harness.Version, session.View.Config.Model.Provider),
	}); err != nil {
		t.Fatal(err)
	}
	return node
}

func TestTransferRequiresWrittenCoveredCheckpointAndEveryRetainedRun(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	node := transferNode(t, store, session)
	request := budgetOperation(t, store, session, "prepared-before-transfer")
	operation, err := store.Prepare(ctx, session.Owner, request)
	if err != nil {
		t.Fatal(err)
	}
	transferRequest := wire.TransferCreate{RequestID: "move", OwnerEpoch: strconv.FormatInt(session.Owner.Epoch, 10), TargetNode: node}
	transfer, err := store.BeginTransfer(ctx, session.View.ID, transferRequest)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = store.ClaimDispatch(ctx, session.View.ID, session.Owner, operation.View.ID, "sender", "claim"); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("source dispatched new work while draining: %v", err)
	}
	if _, _, _, err = store.NextInput(ctx, session.View.ID, session.Owner); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("source processed new input while frozen: %v", err)
	}
	sequence, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	record := wire.Record{SessionID: session.View.ID, Sequence: strconv.FormatInt(sequence, 10), Kind: "ownership.checkpoint",
		Data: kernel.JSON(map[string]any{"format": "brain-ownership-checkpoint/1", "transfer_id": transfer.View.ID,
			"source_epoch": transfer.View.SourceEpoch, "target_node": node})}
	if err = store.SealTransfer(ctx, session.View.ID, session.Owner, transfer.View.ID, record); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("unwritten checkpoint was sealed: %v", err)
	}
	digest := kernel.Digest(kernel.JSON(record))
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, digest, int64(len(kernel.JSON(record)))); err != nil {
		t.Fatal(err)
	}
	if err = store.SealTransfer(ctx, session.View.ID, session.Owner, transfer.View.ID, record); err != nil {
		t.Fatal(err)
	}
	if _, err = store.Allocate(ctx, session.View.ID, session.Owner); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("frozen verification barrier accepted another allocation: %v", err)
	}
	reference := wire.RecordReference{SessionID: session.View.ID, Sequence: record.Sequence, Digest: digest}
	if err = store.VerifyTransferTail(ctx, session.View.ID, transfer.View.ID, node, reference); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("target attested a checkpoint without archive coverage: %v", err)
	}
	members, _, err := store.Allocations(ctx, session.View.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	if err = store.PublishCoverage(ctx, session.Owner, control.Coverage{Session: session.View.ID, First: 1, Last: sequence,
		Key: "immutable/transfer", Digest: kernel.Digest([]byte("manifest")), Bytes: 512, Generation: session.Generation, Records: members}); err != nil {
		t.Fatal(err)
	}
	if _, err = store.VerifyTransferRuns(ctx, session.View.ID, transfer.View.ID, node, "", nil); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("ownership transferred without target checkpoint verification: %v", err)
	}
	if err = store.VerifyTransferTail(ctx, session.View.ID, transfer.View.ID, node, reference); err != nil {
		t.Fatal(err)
	}
	if _, err = store.VerifyTransferRuns(ctx, session.View.ID, transfer.View.ID, node, "", nil); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("ownership transferred without checking its retained run: %v", err)
	}
	runs, err := store.TransferRuns(ctx, session.View.ID, transfer.View.ID, "", 4)
	if err != nil || len(runs) != 1 || runs[0].View.ID != request.Run {
		t.Fatalf("incorrect frozen checkpoint page: %+v %v", runs, err)
	}
	proof, err := store.VerifyTransferRuns(ctx, session.View.ID, transfer.View.ID, node, "", []string{request.Run})
	if err != nil || proof.View.VerifiedRuns != 1 {
		t.Fatalf("run verification did not persist: %+v %v", proof, err)
	}
	completed, err := store.VerifyTransferRuns(ctx, session.View.ID, transfer.View.ID, node, request.Run, nil)
	if err != nil || completed.View.Phase != wire.TransferPhaseTransferred {
		t.Fatalf("verified transfer failed: %+v %v", completed, err)
	}
	if _, err = store.Allocate(ctx, session.View.ID, session.Owner); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("old live owner retained allocation authority: %v", err)
	}
	owner, err := store.ClaimOwner(ctx, session.View.ID, node)
	if err != nil || owner.Epoch != session.Owner.Epoch+1 {
		t.Fatalf("ownership epoch did not advance exactly once: %+v %v", owner, err)
	}
	if _, err = store.ClaimDispatch(ctx, session.View.ID, owner, operation.View.ID, "sender", "claim"); err != nil {
		t.Fatal(err)
	}
	if _, err = store.ClaimDispatch(ctx, session.View.ID, owner, operation.View.ID, "sender", "claim"); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("transfer duplicated the original dispatch permission: %v", err)
	}
	replay, err := store.BeginTransfer(ctx, session.View.ID, transferRequest)
	if err != nil || replay.View.Phase != wire.TransferPhaseTransferred {
		t.Fatalf("original transfer request did not reconcile: %+v %v", replay, err)
	}
	if _, err = store.CancelTransfer(ctx, session.View.ID, transfer.View.ID); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("completed ownership was reversed: %v", err)
	}
}

func TestTransferRejectsOpaqueContinuationAndAbortsOnUnplannedOwnerLoss(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	node := transferNode(t, store, session)
	run := budgetOperation(t, store, session, "opaque").Run
	if err := store.BeginDecision(ctx, session.View.ID, session.Owner, run, 0, kernel.Digest([]byte("request")), "original-process", false); err != nil {
		t.Fatal(err)
	}
	request := wire.TransferCreate{RequestID: "move", OwnerEpoch: strconv.FormatInt(session.Owner.Epoch, 10), TargetNode: node}
	if _, err := store.BeginTransfer(ctx, session.View.ID, request); kernel.Response(err).Code != wire.ErrorCodeUnsupported {
		t.Fatalf("an opaque process was promised automatic transfer: %v", err)
	}
	if _, err := store.Accept(ctx, kernel.Principal{Tenant: "tenant", Subject: "operator"}, session.View.ID, wire.Input{
		RequestID: "cancel", Data: wire.InputData{Kind: "control", Control: kernel.Ptr(wire.ControlKindCancel)},
	}); err != nil {
		t.Fatal(err)
	}
	transfer, err := store.BeginTransfer(ctx, session.View.ID, request)
	if err != nil {
		t.Fatal(err)
	}
	if err = store.FinishStep(ctx, session.View.ID, session.Owner); err != nil {
		t.Fatal(err)
	}
	if err = store.RetireNode(ctx, session.Owner.Node); err != nil {
		t.Fatal(err)
	}
	ready, err := store.Pending(ctx, node, 16)
	if err != nil || len(ready) != 1 || ready[0] != session.View.ID {
		t.Fatalf("unfinished handoff hid its lost owner from recovery: %+v %v", ready, err)
	}
	if _, err = store.ClaimOwner(ctx, session.View.ID, node); err != nil {
		t.Fatal(err)
	}
	transfer, err = store.Transfer(ctx, session.View.ID, transfer.View.ID)
	if err != nil || transfer.View.Phase != wire.TransferPhaseFailed || transfer.View.Error == nil || transfer.View.Error.Code != wire.ErrorCodeUncertain {
		t.Fatalf("unplanned recovery masqueraded as a verified transfer: %+v %v", transfer, err)
	}
}

func TestAuthorityLeaseIsRecheckedAfterTransactionWork(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	if _, err := store.db.ExecContext(ctx, store.q("UPDATE nodes SET lease="+store.clock()+"+300 WHERE id=?"), session.Owner.Node); err != nil {
		t.Fatal(err)
	}
	err := store.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := store.guard(ctx, tx, session.View.ID, session.Owner, false); err != nil {
			return err
		}
		time.Sleep(350 * time.Millisecond)
		_, err := tx.ExecContext(ctx, store.q("UPDATE sessions SET revision=revision+1 WHERE id=?"), session.View.ID)
		return err
	})
	if kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("expired authority committed after its first lease check: %v", err)
	}
	after, err := store.Session(ctx, "tenant", session.View.ID)
	if err != nil || after.View.Revision != session.View.Revision {
		t.Fatalf("expired authority changed retained state: %+v %v", after, err)
	}
}
