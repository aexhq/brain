package sqlcontrol

import (
	"context"
	"slices"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestCapabilitiesPinRetainedResourcesAndCannotChangeWithinAnIncarnation(t *testing.T) {
	store, seed := testStore(t)
	ctx := context.Background()
	required := []string{"model:installed", "storage:retained-original", "secret:tenant-scoped"}
	custody := kernel.Digest([]byte("test cluster key"))
	if err := store.ConfigureNode(ctx, seed.Owner.Node, "http://original", custody, required); err != nil {
		t.Fatal(err)
	}
	request := wire.SessionCreate{RequestID: "pinned-session", Config: seed.View.Config, Message: kernel.Ptr("durable input")}
	session, err := store.Create(ctx, "tenant", request, "", "", required)
	if err != nil {
		t.Fatal(err)
	}
	owner, err := store.ClaimOwner(ctx, session.View.ID, seed.Owner.Node)
	if err != nil {
		t.Fatal(err)
	}
	replacement := []string{"model:installed", "storage:new-empty-volume", "secret:tenant-scoped"}
	replayed, err := store.Create(ctx, "tenant", request, "", "", replacement)
	if err != nil || replayed.View.ID != session.View.ID {
		t.Fatalf("idempotent admission: %v %v", replayed.View.ID, err)
	}
	for _, node := range []string{"wrong-volume", "missing-secret", "restored-volume"} {
		if err = store.RegisterNode(ctx, node, 60000); err != nil {
			t.Fatal(err)
		}
		capabilities := required
		if node == "wrong-volume" {
			capabilities = replacement
		}
		if node == "missing-secret" {
			capabilities = required[:2]
		}
		if err = store.ConfigureNode(ctx, node, "http://"+node, custody, capabilities); err != nil {
			t.Fatal(err)
		}
	}
	if err = store.RetireNode(ctx, owner.Node); err != nil {
		t.Fatal(err)
	}
	for _, node := range []string{"wrong-volume", "missing-secret"} {
		if _, err = store.ClaimOwner(ctx, session.View.ID, node); kernel.Response(err).Code != wire.ErrorCodeConflict {
			t.Fatalf("ineligible node %s claimed authority: %v", node, err)
		}
		pending, e := store.Pending(ctx, node, 32)
		if e != nil || slices.Contains(pending, session.View.ID) {
			t.Fatalf("ineligible node %s was scheduled: %v %v", node, pending, e)
		}
	}
	restored, err := store.ClaimOwner(ctx, session.View.ID, "restored-volume")
	if err != nil || restored.Epoch != owner.Epoch+1 {
		t.Fatalf("restored retained resource was not eligible: %+v %v", restored, err)
	}
	if err = store.ConfigureNode(ctx, restored.Node, "http://restored-volume", custody, replacement); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("incarnation capabilities changed: %v", err)
	}
	if _, err = store.ClaimOwner(ctx, session.View.ID, restored.Node); err != nil {
		t.Fatalf("failed reconfiguration changed admitted capabilities: %v", err)
	}
}

func TestResultBindingNeedsWrittenMatchingEvidence(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	op := testOperation(t, store, session, "bound-result")
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "dispatch"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.RecordReceipt(ctx, session.View.ID, op.View.ID, wire.Receipt{RequestID: "receipt", Outcome: wire.OutcomeCompleted, Result: kernel.JSON("known completion")}, 0, ""); err != nil {
		t.Fatal(err)
	}
	sequence, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	digest := kernel.Digest([]byte("result evidence"))
	if err = store.BindResult(ctx, session.View.ID, session.Owner, op.View.ID, sequence, digest); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("reserved position became result evidence: %v", err)
	}
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, digest, 64); err != nil {
		t.Fatal(err)
	}
	if err = store.BindResult(ctx, session.View.ID, session.Owner, op.View.ID, sequence, kernel.Digest([]byte("different"))); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("different bytes became result evidence: %v", err)
	}
	if err = store.BindResult(ctx, session.View.ID, session.Owner, op.View.ID, sequence, digest); err != nil {
		t.Fatal(err)
	}
}
