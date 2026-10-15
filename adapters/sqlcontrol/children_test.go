package sqlcontrol

import (
	"context"
	"strconv"
	"testing"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func completeTestRun(t *testing.T, store *Store, session control.Session, run string, result any) wire.Record {
	t.Helper()
	ctx := context.Background()
	if err := store.SaveDecision(ctx, session.View.ID, session.Owner, run, 0,
		wire.Decision{Kind: "complete", Result: kernel.JSON(result), Checkpoint: kernel.JSON(map[string]any{})}); err != nil {
		t.Fatal(err)
	}
	sequence, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	record := wire.Record{SessionID: session.View.ID, Sequence: strconv.FormatInt(sequence, 10), Kind: "run.completed", TimeMs: 1,
		RunID: &run, Data: kernel.JSON(map[string]any{"result": result})}
	if err = store.CompleteRun(ctx, session.View.ID, session.Owner, run, 0, record); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("unwritten completion was accepted: %v", err)
	}
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, kernel.Digest(kernel.JSON(record)), int64(len(kernel.JSON(record)))); err != nil {
		t.Fatal(err)
	}
	if err = store.CompleteRun(ctx, session.View.ID, session.Owner, run, 0, record); err != nil {
		t.Fatal(err)
	}
	return record
}

func TestCompletionRequiresItsOriginalWrittenDecision(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	run := budgetOperation(t, store, session, "completion").Run
	if err := store.AdvanceRun(ctx, session.View.ID, session.Owner, run, 0, wire.RunStatusCompleted, nil, nil); kernel.Response(err).Code != wire.ErrorCodeInvalid {
		t.Fatalf("completion bypassed written evidence: %v", err)
	}
	record := completeTestRun(t, store, session, run, map[string]any{"answer": 42})
	if err := store.CompleteRun(ctx, session.View.ID, session.Owner, run, 0, record); err != nil {
		t.Fatalf("original completion did not reconcile: %v", err)
	}
	runs, err := store.Runs(ctx, session.View.ID, 8)
	if err != nil || len(runs) != 1 || runs[0].View.Completion == nil ||
		runs[0].View.Completion.Sequence != record.Sequence || runs[0].View.Completion.Digest != kernel.Digest(kernel.JSON(record)) {
		t.Fatalf("completed run lost its original record reference: %+v %v", runs, err)
	}
	changed := record
	changed.Data = kernel.JSON(map[string]any{"result": "different bytes"})
	if err = store.CompleteRun(ctx, session.View.ID, session.Owner, run, 0, changed); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("changed completion replaced original evidence: %v", err)
	}
	other := budgetOperation(t, store, session, "different-result").Run
	if err = store.SaveDecision(ctx, session.View.ID, session.Owner, other, 0,
		wire.Decision{Kind: "complete", Result: kernel.JSON(42), Checkpoint: kernel.JSON(map[string]any{})}); err != nil {
		t.Fatal(err)
	}
	sequence, _ := store.Allocate(ctx, session.View.ID, session.Owner)
	changed.RunID, changed.Sequence = &other, strconv.FormatInt(sequence, 10)
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, kernel.Digest(kernel.JSON(changed)), int64(len(kernel.JSON(changed)))); err != nil {
		t.Fatal(err)
	}
	if err = store.CompleteRun(ctx, session.View.ID, session.Owner, other, 0, changed); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("written bytes from another result completed the saved decision: %v", err)
	}
}

func TestForkSlotsSurviveOwnerChangeAndJoinOnlyTheirOriginalInputs(t *testing.T) {
	store, parent := testStore(t)
	ctx := context.Background()
	run := budgetOperation(t, store, parent, "fork").Run
	decision := wire.Decision{Kind: "fork", Checkpoint: kernel.JSON(map[string]any{})}
	for _, name := range []string{"alpha", "beta", "gamma"} {
		decision.Delegations = append(decision.Delegations, wire.Delegation{RequestID: name, Config: parent.View.Config, Message: name})
	}
	if err := store.SaveDecision(ctx, parent.View.ID, parent.Owner, run, 0, decision); err != nil {
		t.Fatal(err)
	}
	admit := func(index int) control.Session {
		t.Helper()
		child := decision.Delegations[index]
		value, err := store.Delegate(ctx, parent.View.ID, parent.Owner, run, 0, uint64(index),
			wire.SessionCreate{RequestID: "ignored-caller-id", Config: child.Config, Message: &child.Message}, nil)
		if err != nil {
			t.Fatal(err)
		}
		return value
	}
	first := admit(0)
	before, err := store.Children(ctx, parent.View.ID, run, 0)
	if err != nil || before.Ready || before.Expected != 3 || len(before.Children) != 1 ||
		before.Children[0].Availability != wire.ChildResultAvailabilityPending {
		t.Fatalf("partially admitted fork appeared complete: %+v %v", before, err)
	}
	if err = store.AdvanceRun(ctx, parent.View.ID, parent.Owner, run, 0, wire.RunStatusWaiting, &wire.Wait{Kind: "children", ID: "0"}, nil); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("partial admission advanced its saved decision: %v", err)
	}
	if err = store.RetireNode(ctx, parent.Owner.Node); err != nil {
		t.Fatal(err)
	}
	if err = store.RegisterNode(ctx, "fork-successor", 60000); err != nil {
		t.Fatal(err)
	}
	parent.Owner, err = store.ClaimOwner(ctx, parent.View.ID, "fork-successor")
	if err != nil {
		t.Fatal(err)
	}
	if again := admit(0); again.View.ID != first.View.ID {
		t.Fatal("owner epoch changed the child identity")
	}
	children := []control.Session{first, admit(1), admit(2)}
	if err = store.AdvanceRun(ctx, parent.View.ID, parent.Owner, run, 0, wire.RunStatusWaiting, &wire.Wait{Kind: "children", ID: "0"}, nil); err != nil {
		t.Fatal(err)
	}
	for index, child := range children {
		child.Owner, err = store.ClaimOwner(ctx, child.View.ID, parent.Owner.Node)
		if err != nil {
			t.Fatal(err)
		}
		command := kernel.ID("input", child.View.ID, kernel.ChildCreateID(parent.View.ID, run, 0, uint64(index))+":initial")
		if err = store.ApplyInput(ctx, child.View.ID, control.ApplyInput{Command: command, Owner: child.Owner, Sequence: 1, Run: true}); err != nil {
			t.Fatal(err)
		}
		childRun := kernel.ID("run", child.View.ID, command)
		switch index {
		case 0:
			completeTestRun(t, store, child, childRun, "original child answer")
			_, err = store.Accept(ctx, kernel.Principal{Tenant: "tenant", Subject: "operator"}, child.View.ID,
				wire.Input{RequestID: "unrelated-later-work", Data: wire.InputData{Kind: "message", Text: kernel.Ptr("later input")}})
		case 1:
			err = store.AdvanceRun(ctx, child.View.ID, child.Owner, childRun, 0, wire.RunStatusBlocked, nil,
				&wire.ErrorResponse{Code: wire.ErrorCodeUncertain, Message: "original effect is unknown"})
		case 2:
			_, err = store.Accept(ctx, kernel.Principal{Tenant: "tenant", Subject: "operator"}, child.View.ID,
				wire.Input{RequestID: "cancel-child", Data: wire.InputData{Kind: "control", Control: kernel.Ptr(wire.ControlKindCancel)}})
		}
		if err != nil {
			t.Fatal(err)
		}
	}
	result, err := store.Children(ctx, parent.View.ID, run, 0)
	if err != nil || !result.Ready || len(result.Children) != 3 || result.Children[0].Completion == nil ||
		result.Children[0].Status != wire.RunStatusCompleted || result.Children[1].Status != wire.RunStatusBlocked ||
		result.Children[1].Error.Code != wire.ErrorCodeUncertain || result.Children[2].Status != wire.RunStatusCancelled {
		t.Fatalf("join did not retain exact original partial outcomes: %+v %v", result, err)
	}
	ready, err := store.RunnableRuns(ctx, parent.View.ID, 8)
	if err != nil || len(ready) != 1 || ready[0].View.ID != run {
		t.Fatalf("ready join was hidden by unrelated child work: %+v %v", ready, err)
	}
	if err = store.FinishStep(ctx, parent.View.ID, parent.Owner); err != nil {
		t.Fatal(err)
	}
	var readyFlag int
	if err = store.db.QueryRowContext(ctx, store.q("SELECT ready FROM sessions WHERE id=?"), parent.View.ID).Scan(&readyFlag); err != nil || readyFlag != 1 {
		t.Fatalf("ready fork lost its durable wakeup: %d %v", readyFlag, err)
	}
}
