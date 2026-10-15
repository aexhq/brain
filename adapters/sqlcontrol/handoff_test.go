package sqlcontrol

import (
	"context"
	"testing"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestHarnessHandoffRequiresSettledWorkAndCannotUndoLaterHandoffs(t *testing.T) {
	store, baseline := testStore(t)
	ctx := context.Background()
	first := wire.HarnessSelection{Name: "first", Version: "1"}
	second := wire.HarnessSelection{Name: "second", Version: "1"}
	config := baseline.View.Config
	config.Harness, config.HarnessTransitions = first, []wire.HarnessSelection{first, second}
	if err := store.ConfigureNode(ctx, baseline.Owner.Node, "http://fixture", kernel.Digest([]byte("custody")),
		[]string{kernel.HarnessCapability(first, config.Model.Provider), kernel.HarnessCapability(second, config.Model.Provider)}); err != nil {
		t.Fatal(err)
	}
	session := budgetSession(t, store, control.Session{}, "handoff-session", config)
	request := wire.Handoff{RequestID: "forward", Revision: session.View.Revision, Harness: second}
	p := kernel.Principal{Tenant: "tenant", Subject: "operator"}
	if _, err := store.Accept(ctx, p, session.View.ID,
		wire.Input{RequestID: "pending", Data: wire.InputData{Kind: "message", Text: kernel.Ptr("old harness work")}}); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Handoff(ctx, session.View.ID, request); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("accepted work changed harness before it was processed: %v", err)
	}
	cancel := func(id string) {
		t.Helper()
		if _, err := store.Accept(ctx, p, session.View.ID, wire.Input{RequestID: id,
			Data: wire.InputData{Kind: "control", Control: kernel.Ptr(wire.ControlKindCancel)}}); err != nil {
			t.Fatal(err)
		}
	}
	cancel("cancel-pending")
	changed, err := store.Handoff(ctx, session.View.ID, request)
	if err != nil || changed.View.Config.Harness.Name != "second" {
		t.Fatalf("settled handoff failed: %+v %v", changed, err)
	}
	again, err := store.Handoff(ctx, session.View.ID, request)
	if err != nil || again.View.Revision != changed.View.Revision {
		t.Fatalf("handoff reconciliation repeated its mutation: %+v %v", again, err)
	}
	run := budgetOperation(t, store, session, "blocked-work").Run
	if err = store.AdvanceRun(ctx, session.View.ID, session.Owner, run, 0, wire.RunStatusBlocked, nil,
		&wire.ErrorResponse{Code: wire.ErrorCodeUnsupported, Message: "unsupported continuation"}); err != nil {
		t.Fatal(err)
	}
	back := wire.Handoff{RequestID: "back", Revision: changed.View.Revision, Harness: first}
	if _, err = store.Handoff(ctx, session.View.ID, back); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("blocked checkpoint entered a different harness: %v", err)
	}
	cancel("cancel-blocked")
	returned, err := store.Handoff(ctx, session.View.ID, back)
	if err != nil {
		t.Fatal(err)
	}
	again, err = store.Handoff(ctx, session.View.ID, request)
	if err != nil || again.View.Config.Harness.Name != "first" || again.View.Revision != returned.View.Revision {
		t.Fatalf("replayed old request undid a later handoff: %+v %v", again, err)
	}
	var count int
	if err = store.db.QueryRowContext(ctx, store.q("SELECT COUNT(*) FROM harness_handoffs WHERE session_id=?"), session.View.ID).Scan(&count); err != nil || count != 2 {
		t.Fatalf("handoff identities were not retained exactly once: %d %v", count, err)
	}
}
