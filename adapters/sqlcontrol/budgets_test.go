package sqlcontrol

import (
	"context"
	"strconv"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func budgetSession(t *testing.T, store *Store, parent control.Session, name string, config wire.SessionConfig) control.Session {
	t.Helper()
	session, err := store.Create(context.Background(), "tenant", wire.SessionCreate{RequestID: name, Config: config},
		parent.View.ID, parent.Budget, nil)
	if err != nil {
		t.Fatal(err)
	}
	session.Owner, err = store.ClaimOwner(context.Background(), session.View.ID, "test-incarnation")
	if err != nil {
		t.Fatal(err)
	}
	return session
}

func budgetOperation(t *testing.T, store *Store, session control.Session, name string) control.Prepare {
	t.Helper()
	ctx := context.Background()
	command, err := store.Accept(ctx, kernel.Principal{Tenant: "tenant", Subject: "operator"}, session.View.ID,
		wire.Input{RequestID: name, Data: wire.InputData{Kind: "message", Text: kernel.Ptr(name)}})
	if err != nil {
		t.Fatal(err)
	}
	if err = store.ApplyInput(ctx, session.View.ID, control.ApplyInput{Command: command.ID, Owner: session.Owner, Run: true, Sequence: 1}); err != nil {
		t.Fatal(err)
	}
	sequence, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	digest := kernel.Digest([]byte(name))
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, digest, 64); err != nil {
		t.Fatal(err)
	}
	return control.Prepare{ID: name, Session: session.View.ID, Run: kernel.ID("run", session.View.ID, command.ID),
		StepKey: "0", Kind: wire.EffectKindModel, Action: "generate",
		Arguments: kernel.JSON(wire.ModelRequest{Messages: []wire.ModelMessage{{Role: "user", Content: kernel.JSON(name)}}}),
		Recipient: "model", OutputLimit: 8192, Timeout: 30000, IntentSequence: sequence, IntentDigest: digest}
}

func TestHierarchicalBudgetsAndIncompleteUsage(t *testing.T) {
	store, baseline := testStore(t)
	ctx := context.Background()
	config := baseline.View.Config
	config.Limits.Calls, config.Limits.Concurrent, config.Limits.Tokens = 3, 2, kernel.Ptr(uint64(100000))
	root := budgetSession(t, store, control.Session{}, "budget-root", config)
	config.Limits.Calls = 1
	a := budgetSession(t, store, root, "budget-a", config)
	b := budgetSession(t, store, root, "budget-b", config)
	c := budgetSession(t, store, root, "budget-c", config)
	for _, session := range []control.Session{a, b} {
		request := budgetOperation(t, store, session, session.View.ID)
		if _, err := store.Prepare(ctx, session.Owner, request); err != nil {
			t.Fatal(err)
		}
	}
	request := budgetOperation(t, store, c, c.View.ID)
	if _, err := store.Prepare(ctx, c.Owner, request); kernel.Response(err).Code != wire.ErrorCodeOverloaded {
		t.Fatalf("shared concurrency admitted a third operation: %v", err)
	}
	denied, err := store.Budget(ctx, c.View.ID)
	if err != nil || denied.Calls != 0 || denied.AvailableOperations != 0 {
		t.Fatalf("denied reservation changed child budget: %+v %v", denied, err)
	}
	settle := func(session control.Session, outcome wire.Outcome, usage *wire.Usage) {
		t.Helper()
		prior, err := store.Operation(ctx, session.View.ID, session.View.ID)
		if err != nil {
			t.Fatal(err)
		}
		if prior.View.Status == wire.OperationStatusPrepared {
			if _, err = store.ClaimDispatch(ctx, session.View.ID, session.Owner, session.View.ID, session.Owner.Node, "claim"); err != nil {
				t.Fatal(err)
			}
		}
		if _, err = store.RecordReceipt(ctx, session.View.ID, session.View.ID, wire.Receipt{
			RequestID: string(outcome), Outcome: outcome, Usage: usage, Result: kernel.JSON("evidence"),
		}, 0, ""); err != nil {
			t.Fatal(err)
		}
	}
	settle(a, wire.OutcomeCompleted, &wire.Usage{InputTokens: kernel.Ptr(uint64(3)), OutputTokens: kernel.Ptr(uint64(5))})
	if _, err = store.Prepare(ctx, c.Owner, request); err != nil {
		t.Fatal(err)
	}
	extra := budgetOperation(t, store, a, "extra")
	if _, err = store.Prepare(ctx, a.Owner, extra); kernel.Response(err).Code != wire.ErrorCodeForbidden {
		t.Fatalf("parent or child call limit was amplified: %v", err)
	}
	settle(b, wire.OutcomeCompleted, &wire.Usage{InputTokens: kernel.Ptr(uint64(7))})
	settle(c, wire.OutcomeUnknown, nil)
	before, err := store.Budget(ctx, root.View.ID)
	if err != nil || before.ActiveOperations != 1 || before.Calls != 3 || before.ReportedTokens != "15" || before.IncompleteUsages != 1 {
		t.Fatalf("unknown or incomplete usage lost its hold: %+v %v", before, err)
	}
	settle(c, wire.OutcomeCompleted, &wire.Usage{InputTokens: kernel.Ptr(uint64(2)), OutputTokens: kernel.Ptr(uint64(3))})
	after, err := store.Budget(ctx, root.View.ID)
	if err != nil || after.ActiveOperations != 0 || after.ReportedTokens != "20" || !after.TokenUsagePending {
		t.Fatalf("late original receipt did not settle exactly once: %+v %v", after, err)
	}
	child, err := store.Budget(ctx, b.View.ID)
	if err != nil || child.ReservedTokens == "0" || child.ReservedTokens != after.ReservedTokens || child.IncompleteUsages != 1 {
		t.Fatalf("missing counter was treated as zero: %+v %+v %v", child, after, err)
	}
	held, _ := strconv.ParseInt(child.ReservedTokens, 10, 64)
	if *after.AvailableTokens != strconv.FormatInt(100000-20-held, 10) {
		t.Fatalf("shared token availability is wrong: %+v", after)
	}
	report, err := store.Report(ctx)
	if err != nil || report.RetainedBytes != after.RetainedBytes {
		t.Fatalf("storage report double-counted ancestor pools: %+v %+v %v", report, after, err)
	}
}

func TestNeverSentBudgetReservationAndDelegationDepth(t *testing.T) {
	store, parent := testStore(t)
	ctx := context.Background()
	deadline := uint64(time.Now().Add(2 * time.Second).UnixMilli())
	config := parent.View.Config
	config.Limits.DeadlineMs = &deadline
	config.Limits.Tokens = kernel.Ptr(uint64(100000))
	root := budgetSession(t, store, control.Session{}, "deadline-root", config)
	request := budgetOperation(t, store, root, "unsent")
	op, err := store.Prepare(ctx, root.Owner, request)
	if err != nil || op.Deadline != int64(deadline) {
		t.Fatalf("session deadline did not bound dispatch: %+v %v", op, err)
	}
	time.Sleep(time.Until(time.UnixMilli(int64(deadline))) + 10*time.Millisecond)
	if _, err = store.ExpireOperations(ctx, 16); err != nil {
		t.Fatal(err)
	}
	usage, err := store.Budget(ctx, root.View.ID)
	if err != nil || usage.ActiveOperations != 0 || usage.ReservedTokens != "0" || usage.IncompleteUsages != 0 {
		t.Fatalf("never-sent operation retained phantom usage: %+v %v", usage, err)
	}
	child := parent
	for depth := 1; depth <= kernel.MaxDelegationDepth; depth++ {
		child = budgetSession(t, store, child, "depth-"+strconv.Itoa(depth), child.View.Config)
	}
	_, err = store.Create(ctx, "tenant", wire.SessionCreate{RequestID: "too-deep", Config: child.View.Config}, child.View.ID, child.Budget, nil)
	if kernel.Response(err).Code != wire.ErrorCodeForbidden {
		t.Fatalf("unbounded delegation was admitted: %v", err)
	}
}
