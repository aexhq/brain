package sqlcontrol

import (
	"context"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestCancelledDecisionCannotAdmitAnotherChild(t *testing.T) {
	store, parent := testStore(t)
	ctx := context.Background()
	run := budgetOperation(t, store, parent, "parent-decision")
	child := wire.Delegation{RequestID: "helper", Config: parent.View.Config, Message: "work"}
	decision := wire.Decision{Kind: "delegate", Delegation: &child, Checkpoint: kernel.JSON(map[string]bool{})}
	if err := store.SaveDecision(ctx, parent.View.ID, parent.Owner, run.Run, 0, decision); err != nil {
		t.Fatal(err)
	}
	request := wire.SessionCreate{RequestID: "child-at-boundary", Config: child.Config, Message: &child.Message}
	if _, err := store.Delegate(ctx, parent.View.ID, parent.Owner, run.Run, 0, 0, request, nil); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Accept(ctx, kernel.Principal{Tenant: "tenant", Subject: "operator"}, parent.View.ID,
		wire.Input{RequestID: "cancel-boundary", Data: wire.InputData{Kind: "control", Control: kernel.Ptr(wire.ControlKindCancel)}}); err != nil {
		t.Fatal(err)
	}
	request.RequestID = "another-child"
	if _, err := store.Delegate(ctx, parent.View.ID, parent.Owner, run.Run, 0, 0, request, nil); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("cancelled saved decision admitted another child: %v", err)
	}
	var count int
	if err := store.db.QueryRowContext(ctx, "SELECT COUNT(*) FROM sessions").Scan(&count); err != nil || count != 2 {
		t.Fatalf("cancelled decision changed child population: %d %v", count, err)
	}
}
