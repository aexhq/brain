package sqlcontrol

import (
	"context"
	"fmt"
	"reflect"
	"testing"
	"time"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestDeletedReceiptPreservesOriginalOutcomeAndSparseAccountingWithoutBodies(t *testing.T) {
	store, parent := testStore(t)
	ctx := context.Background()
	child := budgetSession(t, store, parent, "deleted-child", parent.View.Config)
	op := testOperation(t, store, child, "original-dispatch")
	if _, err := store.ClaimDispatch(ctx, child.View.ID, child.Owner, op.View.ID, child.Owner.Node, "once"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Delete(ctx, child.View.ID); err != nil {
		t.Fatal(err)
	}
	if count, err := store.DiscardDeletedResults(ctx, 32); err != nil || count != 1 {
		t.Fatalf("deleted operation disposition: %d %v", count, err)
	}
	if err := store.RetireNode(ctx, child.Owner.Node); err != nil {
		t.Fatal(err)
	}
	if count, err := store.ExpireOperations(ctx, 32); err != nil || count != 1 {
		t.Fatalf("lost original owner: %d %v", count, err)
	}
	before, err := store.Budget(ctx, parent.View.ID)
	if err != nil || before.ActiveOperations != 1 || before.ReservedOutputBytes == 0 || before.ReservedTokens == "0" {
		t.Fatalf("deletion released an uncertain effect: %+v %v", before, err)
	}
	provider := kernel.JSON(map[string]string{"private": "original provider payload"})
	receipt := wire.Receipt{RequestID: "original-result", Outcome: wire.OutcomeCompleted,
		Result: kernel.JSON(map[string]string{"private": "original result payload"}),
		Usage:  &wire.Usage{InputTokens: kernel.Ptr(uint64(3)), Provider: provider}}
	value, err := store.RecordReceipt(ctx, child.View.ID, op.View.ID, receipt, 0, "")
	if err != nil || value.View.Status != wire.OperationStatusCompleted || !value.Discarded ||
		value.View.ResultAvailable || len(value.View.Result) != 0 || value.ResultSequence != 0 {
		t.Fatalf("original evidence restored deleted bytes or lost completion: %+v %v", value, err)
	}
	usage := value.View.Usage
	if usage == nil || usage.InputTokens == nil || *usage.InputTokens != 3 || usage.OutputTokens != nil ||
		string(usage.Provider) != "{}" || usage.ProviderUnavailable == nil ||
		usage.ProviderUnavailable.Digest != kernel.Digest(provider) || usage.ProviderUnavailable.Bytes != uint64(len(provider)) {
		t.Fatalf("deleted evidence lost sparse accounting or retained provider bodies: %+v", usage)
	}
	after, err := store.Budget(ctx, parent.View.ID)
	if err != nil || after.Calls != 1 || after.ActiveOperations != 0 || after.ReservedOutputBytes != 0 ||
		after.ReportedTokens != "3" || after.ReservedTokens != before.ReservedTokens || after.IncompleteUsages != 1 {
		t.Fatalf("late settlement did not release exactly its known capacity: %+v %v", after, err)
	}
	for range 2 {
		repeated, err := store.RecordReceipt(ctx, child.View.ID, op.View.ID, receipt, 0, "")
		if err != nil || repeated.View.Revision != value.View.Revision || !repeated.Discarded || repeated.View.ResultAvailable {
			t.Fatalf("unchanged evidence was not idempotent: %+v %v", repeated, err)
		}
	}
	repeated, err := store.Budget(ctx, parent.View.ID)
	if err != nil || !reflect.DeepEqual(after, repeated) {
		t.Fatalf("repeated receipt refunded capacity twice: %+v %v", repeated, err)
	}
	changed := receipt
	changed.Result = kernel.JSON("different outcome bytes")
	if _, err := store.RecordReceipt(ctx, child.View.ID, op.View.ID, changed, 0, ""); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("deleted receipt key accepted changed evidence: %v", err)
	}
	changed.RequestID = "different-key"
	if _, err := store.RecordReceipt(ctx, child.View.ID, op.View.ID, changed, 0, ""); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("deleted completion accepted a conflicting new receipt: %v", err)
	}
	var lifecycle, run, body, receiptHash string
	var ready, sequence int64
	if err := store.db.QueryRowContext(ctx, store.q(`SELECT s.lifecycle,s.ready,s.sequence,r.status,o.result_body,o.receipt_hash
FROM sessions s JOIN runs r ON r.session_id=s.id JOIN operations o ON o.run_id=r.id WHERE o.id=?`), op.View.ID).
		Scan(&lifecycle, &ready, &sequence, &run, &body, &receiptHash); err != nil {
		t.Fatal(err)
	}
	if lifecycle != "deleted" || ready != 0 || sequence != op.IntentSequence || run != "cancelled" || body != "" || len(receiptHash) != 64 {
		t.Fatal("late receipt revived deleted work, allocated a position or restored a control body")
	}
	if _, err := store.Session(ctx, "tenant", child.View.ID); kernel.Response(err).Code != wire.ErrorCodeNotFound {
		t.Fatalf("deleted session became readable: %v", err)
	}
	if _, err := store.ClaimDispatch(ctx, child.View.ID, child.Owner, op.View.ID, child.Owner.Node, "again"); err == nil {
		t.Fatal("receipt settlement reissued execution permission")
	}
	if _, err := store.Allocate(ctx, child.View.ID, child.Owner); err == nil {
		t.Fatal("late evidence restored deleted history allocation")
	}
}

func TestDeletionAndOriginalReceiptCommitWithoutLosingOrDuplicatingSettlement(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	op := testOperation(t, store, session, "racing-original")
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "once"); err != nil {
		t.Fatal(err)
	}
	receipt := wire.Receipt{RequestID: "original", Outcome: wire.OutcomeCompleted, Result: kernel.JSON("original body"),
		Usage: &wire.Usage{InputTokens: kernel.Ptr(uint64(2)), OutputTokens: kernel.Ptr(uint64(3)),
			Provider: kernel.JSON(map[string]string{"private": "original provider body"})}}
	results := make(chan error, 2)
	go func() {
		_, err := store.Delete(ctx, session.View.ID)
		results <- err
	}()
	go func() {
		_, err := store.RecordReceipt(ctx, session.View.ID, op.View.ID, receipt, 0, "")
		results <- err
	}()
	for range 2 {
		if err := <-results; err != nil {
			t.Fatal(err)
		}
	}
	value, err := store.RecordReceipt(ctx, session.View.ID, op.View.ID, receipt, 0, "")
	if err != nil || value.View.Status != wire.OperationStatusCompleted || !value.Discarded || value.View.ResultAvailable {
		t.Fatalf("competing deletion lost or exposed the original completion: %+v %v", value, err)
	}
	budget, err := store.Budget(ctx, session.View.ID)
	if err != nil || budget.Calls != 1 || budget.ActiveOperations != 0 || budget.ReservedOutputBytes != 0 ||
		budget.ReportedTokens != "5" || budget.ReservedTokens != "0" {
		t.Fatalf("competing deletion duplicated or lost settlement: %+v %v", budget, err)
	}
}

func TestDeletedResultsReleaseOnlyKnownUnboundReservationsOnce(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	operations := map[string]control.Operation{}
	receipts := map[string]wire.Receipt{}
	for _, name := range []string{"bound", "unbound", "failed", "unsent", "unknown"} {
		op := testOperation(t, store, session, name)
		operations[name] = op
		if name == "unsent" {
			continue
		}
		if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, name, session.Owner.Node, "claim"); err != nil {
			t.Fatal(err)
		}
		outcome := wire.OutcomeCompleted
		if name == "failed" {
			outcome = wire.OutcomeFailed
		} else if name == "unknown" {
			outcome = wire.OutcomeUnknown
		}
		receipt := wire.Receipt{RequestID: "result-" + name, Outcome: outcome, Result: kernel.JSON(name),
			Usage: &wire.Usage{InputTokens: kernel.Ptr(uint64(1)), OutputTokens: kernel.Ptr(uint64(2))}}
		receipts[name] = receipt
		if _, err := store.RecordReceipt(ctx, session.View.ID, name, receipt, 0, ""); err != nil {
			t.Fatal(err)
		}
		if name == "bound" {
			sequence, err := store.Allocate(ctx, session.View.ID, session.Owner)
			if err != nil {
				t.Fatal(err)
			}
			digest := kernel.Digest([]byte("bound original result"))
			if err := store.Written(ctx, session.View.ID, session.Owner, sequence, digest, 32); err != nil {
				t.Fatal(err)
			}
			if err := store.BindResult(ctx, session.View.ID, session.Owner, name, sequence, digest); err != nil {
				t.Fatal(err)
			}
		}
	}
	reserve := uint64(kernel.OutputReservation(operations["bound"].OutputLimit))
	if _, err := store.Delete(ctx, session.View.ID); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"bound", "unbound", "unbound"} {
		if value, err := store.RecordReceipt(ctx, session.View.ID, name, receipts[name], 0, ""); err != nil || !value.Discarded || value.View.ResultAvailable {
			t.Fatalf("settled original replay %s: %+v %v", name, value, err)
		}
	}
	count, err := store.DiscardDeletedResults(ctx, 1)
	if err != nil || count != 1 {
		t.Fatalf("bounded first disposal: %d %v", count, err)
	}
	count, err = store.DiscardDeletedResults(ctx, 32)
	if err != nil || count != 2 {
		t.Fatalf("remaining disposal: %d %v", count, err)
	}
	if count, err = store.DiscardDeletedResults(ctx, 32); err != nil || count != 0 {
		t.Fatalf("repeated disposal: %d %v", count, err)
	}
	before, err := store.Budget(ctx, session.View.ID)
	if err != nil || before.ActiveOperations != 2 || before.ReservedOutputBytes != 2*reserve {
		t.Fatalf("uncertain and never-sent work lost their pending settlement holds: %+v %v", before, err)
	}
	if count, err = store.ExpireOperations(ctx, 32); err != nil || count != 1 {
		t.Fatalf("deleted never-sent expiry: %d %v", count, err)
	}
	after, err := store.Budget(ctx, session.View.ID)
	if err != nil || after.ActiveOperations != 1 || after.ReservedOutputBytes != reserve || after.Calls != 5 {
		t.Fatalf("never-sent settlement affected the uncertain operation: %+v %v", after, err)
	}
	for name := range operations {
		value, err := store.Operation(ctx, session.View.ID, name)
		if err != nil || !value.Discarded || value.View.ResultAvailable || len(value.View.Result) != 0 {
			t.Fatalf("deleted body remained available for %s: %+v %v", name, value, err)
		}
	}
}

func TestDeletedResultDisposalYieldsAtItsFixedBatchBound(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	for i := range 33 {
		testOperation(t, store, session, fmt.Sprintf("operation-%02d", i))
	}
	other := budgetSession(t, store, control.Session{}, "other-deletion", session.View.Config)
	testOperation(t, store, other, "other-operation")
	if _, err := store.Delete(ctx, session.View.ID); err != nil {
		t.Fatal(err)
	}
	time.Sleep(2 * time.Millisecond)
	if _, err := store.Delete(ctx, other.View.ID); err != nil {
		t.Fatal(err)
	}
	time.Sleep(2 * time.Millisecond)
	for index, want := range []int{32, 1, 1, 0} {
		if count, err := store.DiscardDeletedResults(ctx, 1024); err != nil || count != want {
			t.Fatalf("fixed disposal page: got %d want %d: %v", count, want, err)
		}
		if index == 1 {
			operation, err := store.Operation(ctx, other.View.ID, "other-operation")
			if err != nil || !operation.Discarded {
				t.Fatalf("a large deletion starved another session: %+v %v", operation, err)
			}
		}
	}
	value, err := store.Budget(ctx, session.View.ID)
	if err != nil || value.ActiveOperations != 33 {
		t.Fatalf("disposal was mistaken for never-sent settlement: %+v %v", value, err)
	}
	if count, err := store.ExpireOperations(ctx, 64); err != nil || count != 34 {
		t.Fatalf("positive never-sent outcomes: %d %v", count, err)
	}
	value, err = store.Budget(ctx, session.View.ID)
	if err != nil || value.ActiveOperations != 0 || value.ReservedOutputBytes != 0 || value.ReservedTokens != "0" {
		t.Fatalf("known discarded results retained output reservations: %+v %v", value, err)
	}
	var ready, cleanup int
	if err := store.db.QueryRowContext(ctx, store.q("SELECT ready,cleanup_ready FROM sessions WHERE id=?"), session.View.ID).Scan(&ready, &cleanup); err != nil {
		t.Fatal(err)
	}
	if ready != 0 || cleanup != 0 {
		t.Fatal("deleted session was made runnable or left in the completed disposal queue")
	}
}

func TestOriginalActionReceiptSchemaSurvivesDeletion(t *testing.T) {
	store, baseline := testStore(t)
	ctx := context.Background()
	config := baseline.View.Config
	schema := kernel.JSON(map[string]string{"type": "object"})
	config.Actions = []wire.Action{{Name: "deliver", OutputSchema: schema}}
	session := budgetSession(t, store, control.Session{}, "action-policy", config)
	request := budgetOperation(t, store, session, "original-action")
	request.Kind, request.Action, request.Recipient = wire.EffectKindAction, "deliver", "internal"
	op, err := store.Prepare(ctx, session.Owner, request)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.ReceiptOutputSchema(ctx, session.View.ID, op.View.ID); kernel.Response(err).Code != wire.ErrorCodeNotFound {
		t.Fatalf("unclaimed operation exposed a receipt policy: %v", err)
	}
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "claim"); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		value, err := store.ReceiptOutputSchema(ctx, session.View.ID, op.View.ID)
		if err != nil || value != string(schema) {
			t.Fatalf("immutable receipt policy: %q %v", value, err)
		}
		if _, err := store.Delete(ctx, session.View.ID); err != nil {
			t.Fatal(err)
		}
	}
}
