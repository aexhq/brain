package sqlcontrol

import (
	"context"
	"testing"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestParentControlsFenceClaimsBeforeBoundedPropagation(t *testing.T) {
	store, parent := testStore(t)
	ctx := context.Background()
	a := budgetSession(t, store, parent, "control-a", parent.View.Config)
	b := budgetSession(t, store, parent, "control-b", parent.View.Config)
	prepare := func(session control.Session) {
		t.Helper()
		if _, err := store.Prepare(ctx, session.Owner, budgetOperation(t, store, session, session.View.ID)); err != nil {
			t.Fatal(err)
		}
	}
	prepare(a)
	prepare(b)
	controlInput := func(kind wire.ControlKind, key string) {
		t.Helper()
		if _, err := store.Accept(ctx, kernel.Principal{Tenant: "tenant", Subject: "operator"}, parent.View.ID,
			wire.Input{RequestID: key, Data: wire.InputData{Kind: "control", Control: &kind}}); err != nil {
			t.Fatal(err)
		}
	}
	claim := func(session control.Session) error {
		_, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, session.View.ID, session.Owner.Node, "claim")
		return err
	}
	controlInput(wire.ControlKindPause, "pause")
	if err := claim(a); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("parent pause issued a child dispatch permit: %v", err)
	}
	budget, err := store.Budget(ctx, a.View.ID)
	if err != nil || budget.AvailableOperations != 0 {
		t.Fatalf("inherited pause did not affect visible headroom: %+v %v", budget, err)
	}
	controlInput(wire.ControlKindResume, "resume")
	if err = claim(a); err != nil {
		t.Fatal(err)
	}
	controlInput(wire.ControlKindCancel, "cancel")
	if err = claim(b); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("parent cancellation issued a child permit before propagation: %v", err)
	}
	if _, err = store.Create(ctx, "tenant", wire.SessionCreate{RequestID: "escape", Config: a.View.Config}, a.View.ID, a.Budget, nil); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("cancelled child created a new authority branch: %v", err)
	}
	for i := 0; i < 3; i++ {
		closed, err := store.PropagateControls(ctx, 1)
		if err != nil || len(closed) > 1 {
			t.Fatalf("propagation exceeded its bound: %v %v", closed, err)
		}
	}
	for _, child := range []control.Session{a, b} {
		value, err := store.Session(ctx, "tenant", child.View.ID)
		if err != nil || value.View.Lifecycle != wire.LifecycleClosed {
			t.Fatalf("descendant did not close: %+v %v", value, err)
		}
	}
	if _, err = store.RecordReceipt(ctx, a.View.ID, a.View.ID, wire.Receipt{RequestID: "original-receipt", Outcome: wire.OutcomeCompleted,
		Result: kernel.JSON("original result"), Usage: &wire.Usage{InputTokens: kernel.Ptr(uint64(1)), OutputTokens: kernel.Ptr(uint64(2))}}, 0, ""); err != nil {
		t.Fatal(err)
	}
	if _, err = store.ExpireOperations(ctx, 16); err != nil {
		t.Fatal(err)
	}
	settled, err := store.Budget(ctx, parent.View.ID)
	if err != nil || settled.ActiveOperations != 0 || settled.ReservedTokens != "0" || settled.ReportedTokens != "3" {
		t.Fatalf("cancellation lost the original receipt or charged never-sent work: %+v %v", settled, err)
	}
	fresh := budgetSession(t, store, parent, "fresh-after-cancel", parent.View.Config)
	prepare(fresh)
	controlInput(wire.ControlKindCancel, "cancel")
	if err = claim(fresh); err != nil {
		t.Fatalf("replayed old cancellation fenced explicitly new work: %v", err)
	}
}
