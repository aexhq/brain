package sqlcontrol

import (
	"context"
	"fmt"
	"reflect"
	"testing"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestUsageSupplementsSettleSparseAncestorHoldsWithoutRewritingReceipts(t *testing.T) {
	store, parent := testStore(t)
	ctx := context.Background()
	child := budgetSession(t, store, parent, "usage-child", parent.View.Config)
	op := testOperation(t, store, child, "original-model")
	if _, err := store.ClaimDispatch(ctx, child.View.ID, child.Owner, op.View.ID, child.Owner.Node, "one-send"); err != nil {
		t.Fatal(err)
	}
	receipt := wire.Receipt{RequestID: "original-receipt", Outcome: wire.OutcomeCompleted, Result: kernel.JSON("original response"),
		Usage: &wire.Usage{Provider: kernel.JSON(map[string]string{"id": "original-provider-evidence"})}}
	if _, err := store.RecordReceipt(ctx, child.View.ID, op.View.ID, receipt, 0, ""); err != nil {
		t.Fatal(err)
	}
	sequence, err := store.Allocate(ctx, child.View.ID, child.Owner)
	if err != nil {
		t.Fatal(err)
	}
	digest := kernel.Digest(kernel.JSON(receipt))
	if err := store.Written(ctx, child.View.ID, child.Owner, sequence, digest, int64(len(kernel.JSON(receipt)))); err != nil {
		t.Fatal(err)
	}
	if err := store.BindResult(ctx, child.View.ID, child.Owner, op.View.ID, sequence, digest); err != nil {
		t.Fatal(err)
	}
	type original struct{ body, hash, key string }
	snapshot := func() original {
		t.Helper()
		var value original
		if err := store.db.QueryRowContext(ctx, store.q("SELECT result_body,receipt_hash,receipt_key FROM operations WHERE id=?"), op.View.ID).
			Scan(&value.body, &value.hash, &value.key); err != nil {
			t.Fatal(err)
		}
		return value
	}
	before := snapshot()
	reporter := control.UsageReporter{Kind: wire.UsageSourceReceipt, Subject: child.Owner.Node}
	requests := []wire.UsageSupplement{
		{RequestID: "reported-zero", Counters: wire.UsageCounters{InputTokens: kernel.Ptr(uint64(0))}},
		{RequestID: "reported-output", Counters: wire.UsageCounters{OutputTokens: kernel.Ptr(uint64(5))}},
		{RequestID: "reported-cache", Counters: wire.UsageCounters{InputTokens: kernel.Ptr(uint64(0)), CachedTokens: kernel.Ptr(uint64(2))}},
	}
	var last control.Operation
	for index, request := range requests {
		last, err = store.ReportUsage(ctx, child.View.ID, op.View.ID, request, reporter)
		if err != nil || last.View.Status != wire.OperationStatusCompleted || last.ResultSequence != sequence ||
			last.ResultDigest != digest || last.Claimant != child.Owner.Node {
			t.Fatalf("supplement altered the original effect: %+v %v", last, err)
		}
		for _, session := range []string{parent.View.ID, child.View.ID} {
			budget, err := store.Budget(ctx, session)
			if err != nil || budget.Calls != 1 || budget.ActiveOperations != 0 || budget.ReservedOutputBytes != 0 {
				t.Fatalf("usage refinement changed execution or output capacity: %+v %v", budget, err)
			}
			if index == 0 {
				if budget.ReportedTokens != "0" || budget.ReservedTokens == "0" || budget.IncompleteUsages != 1 {
					t.Fatalf("reported zero prematurely settled missing output: %+v", budget)
				}
			} else if budget.ReportedTokens != "5" || budget.ReservedTokens != "0" || budget.IncompleteUsages != 0 {
				t.Fatalf("ancestor usage hold was not settled exactly once: %+v", budget)
			}
		}
		repeated, err := store.ReportUsage(ctx, child.View.ID, op.View.ID, request, reporter)
		if err != nil || !reflect.DeepEqual(repeated, last) {
			t.Fatalf("same original evidence was not idempotent: %+v %v", repeated, err)
		}
	}
	if after := snapshot(); after != before {
		t.Fatal("usage supplementation rewrote the original receipt identity or bytes")
	}
	replayed, err := store.RecordReceipt(ctx, child.View.ID, op.View.ID, receipt, 0, "")
	if err != nil || !reflect.DeepEqual(replayed.View.Usage, last.View.Usage) {
		t.Fatalf("original receipt replay erased supplemental usage: %+v %v", replayed, err)
	}
	current, err := store.Session(ctx, "tenant", child.View.ID)
	if err != nil || current.View.Sequence != fmt.Sprint(sequence) {
		t.Fatalf("usage facts allocated new transcript positions: %+v %v", current, err)
	}
	var evidence []wire.UsageEvidence
	after := ""
	for {
		page, err := store.UsageEvidence(ctx, child.View.ID, op.View.ID, 1, after)
		if err != nil {
			t.Fatal(err)
		}
		if len(page) == 0 {
			break
		}
		evidence = append(evidence, page...)
		after = page[len(page)-1].ID
	}
	if len(evidence) != 3 {
		t.Fatalf("usage evidence was missing or unbounded: %+v", evidence)
	}
	for _, value := range evidence {
		if value.Source != wire.UsageSourceReceipt || value.Subject != child.Owner.Node || value.SessionID != child.View.ID ||
			value.OperationID != op.View.ID || value.ID != kernel.ID("usage", child.View.ID, op.View.ID, value.RequestID) || value.AcceptedMs == 0 {
			t.Fatalf("usage evidence lost its immutable attribution: %+v", value)
		}
	}
	changed := requests[0]
	changed.Counters.InputTokens = kernel.Ptr(uint64(1))
	for _, request := range []wire.UsageSupplement{changed, {RequestID: "different-key", Counters: requests[0].Counters}} {
		if _, err := store.ReportUsage(ctx, child.View.ID, op.View.ID, request, reporter); kernel.Response(err).Code != wire.ErrorCodeConflict {
			t.Fatalf("changed or no-op evidence was admitted: %v", err)
		}
	}
	if _, err := store.ReportUsage(ctx, child.View.ID, op.View.ID, requests[0],
		control.UsageReporter{Kind: wire.UsageSourceOperator, Subject: "operator", Tenant: "tenant"}); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("original evidence attribution was replaced: %v", err)
	}
}

func TestUsageSupplementContentionCreatesOneFactAndOneRefund(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	op := testOperation(t, store, session, "contended-model")
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "once"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.RecordReceipt(ctx, session.View.ID, op.View.ID,
		wire.Receipt{RequestID: "known-failure", Outcome: wire.OutcomeFailed, Usage: &wire.Usage{InputTokens: kernel.Ptr(uint64(0))}}, 0, ""); err != nil {
		t.Fatal(err)
	}
	reporter := control.UsageReporter{Kind: wire.UsageSourceOperator, Tenant: "tenant", Subject: "operator"}
	results := make(chan error, 8)
	for index := range 8 {
		go func() {
			_, err := store.ReportUsage(ctx, session.View.ID, op.View.ID,
				wire.UsageSupplement{RequestID: fmt.Sprintf("report-%d", index), Counters: wire.UsageCounters{OutputTokens: kernel.Ptr(uint64(0))}}, reporter)
			results <- err
		}()
	}
	successes := 0
	for range 8 {
		if err := <-results; err == nil {
			successes++
		} else if kernel.Response(err).Code != wire.ErrorCodeConflict {
			t.Fatal(err)
		}
	}
	evidence, err := store.UsageEvidence(ctx, session.View.ID, "", 64)
	if err != nil || successes != 1 || len(evidence) != 1 {
		t.Fatalf("same-counter contention inserted duplicate facts: %d %+v %v", successes, evidence, err)
	}
	budget, err := store.Budget(ctx, session.View.ID)
	if err != nil || budget.Calls != 1 || budget.ActiveOperations != 0 || budget.ReportedTokens != "0" ||
		budget.ReservedTokens != "0" || budget.IncompleteUsages != 0 || budget.ReservedOutputBytes == 0 {
		t.Fatalf("known failure refinement released the wrong reservations: %+v %v", budget, err)
	}
}

func TestUsageEvidenceDeniesUnsettledOrUnsentEffectsAndWrongAuthority(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	request := wire.UsageSupplement{RequestID: "new-counters", Counters: wire.UsageCounters{InputTokens: kernel.Ptr(uint64(0)), OutputTokens: kernel.Ptr(uint64(0))}}
	reporter := control.UsageReporter{Kind: wire.UsageSourceOperator, Tenant: "tenant", Subject: "operator"}
	for _, status := range []string{"prepared", "dispatched", "unknown", "completed"} {
		op := testOperation(t, store, session, status)
		if status != "prepared" {
			if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "once"); err != nil {
				t.Fatal(err)
			}
		}
		if status == "unknown" || status == "completed" {
			if _, err := store.RecordReceipt(ctx, session.View.ID, op.View.ID,
				wire.Receipt{RequestID: "original", Outcome: wire.Outcome(status)}, 0, ""); err != nil {
				t.Fatal(err)
			}
		}
		if status != "completed" {
			if _, err := store.ReportUsage(ctx, session.View.ID, op.View.ID, request, reporter); kernel.Response(err).Code != wire.ErrorCodeConflict {
				t.Fatalf("%s accepted usage that would release uncertain charges: %v", status, err)
			}
		}
	}
	before, err := store.Budget(ctx, session.View.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, wrong := range []control.UsageReporter{
		{Kind: wire.UsageSourceOperator, Tenant: "other", Subject: "operator"},
		{Kind: wire.UsageSourceReceipt, Subject: "different-worker"},
		{Kind: wire.UsageSource("participant"), Tenant: "tenant", Subject: "operator"},
	} {
		if _, err := store.ReportUsage(ctx, session.View.ID, "completed", request, wrong); err == nil {
			t.Fatal("wrong usage authority was admitted")
		}
	}
	after, err := store.Budget(ctx, session.View.ID)
	if err != nil || !reflect.DeepEqual(before, after) {
		t.Fatalf("denied usage changed accounting: %+v %v", after, err)
	}
	if _, err := store.Delete(ctx, session.View.ID); err != nil {
		t.Fatal(err)
	}
	if count, err := store.ExpireOperations(ctx, 32); err != nil || count != 1 {
		t.Fatalf("known never-dispatched failure setup: %d %v", count, err)
	}
	if _, err := store.ReportUsage(ctx, session.View.ID, "prepared", request, reporter); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("never-dispatched failure gained a usage refund: %v", err)
	}
	if rows, err := store.UsageEvidence(ctx, session.View.ID, "", 64); err != nil || len(rows) != 0 {
		t.Fatalf("denied reports left durable facts: %+v %v", rows, err)
	}
}

func TestDeletedUsageEvidenceSettlesAccountingWithoutRestoringBytesOrWork(t *testing.T) {
	store, parent := testStore(t)
	ctx := context.Background()
	session := budgetSession(t, store, parent, "deleted-usage", parent.View.Config)
	op := testOperation(t, store, session, "original-model")
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "once"); err != nil {
		t.Fatal(err)
	}
	receipt := wire.Receipt{RequestID: "original", Outcome: wire.OutcomeCompleted, Result: kernel.JSON("private body"),
		Usage: &wire.Usage{InputTokens: kernel.Ptr(uint64(7)), Provider: kernel.JSON(map[string]string{"private": "provider body"})}}
	if _, err := store.RecordReceipt(ctx, session.View.ID, op.View.ID, receipt, 0, ""); err != nil {
		t.Fatal(err)
	}
	request := wire.UsageSupplement{RequestID: "late-output", Counters: wire.UsageCounters{OutputTokens: kernel.Ptr(uint64(0))}}
	reporter := control.UsageReporter{Kind: wire.UsageSourceOperator, Tenant: "tenant", Subject: "operator"}
	results := make(chan error, 2)
	go func() { _, err := store.Delete(ctx, session.View.ID); results <- err }()
	go func() {
		_, err := store.ReportUsage(ctx, session.View.ID, op.View.ID, request, reporter)
		results <- err
	}()
	for range 2 {
		if err := <-results; err != nil {
			t.Fatal(err)
		}
	}
	if err := store.RetireNode(ctx, session.Owner.Node); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		value, err := store.ReportUsage(ctx, session.View.ID, op.View.ID, request, reporter)
		if err != nil || !value.Discarded || value.View.ResultAvailable || value.View.Status != wire.OperationStatusCompleted ||
			string(value.View.Usage.Provider) != "{}" || value.View.Usage.ProviderUnavailable == nil {
			t.Fatalf("deleted usage restored bytes or lost original evidence: %+v %v", value, err)
		}
		value, err = store.RecordReceipt(ctx, session.View.ID, op.View.ID, receipt, 0, "")
		if err != nil || value.View.Usage.OutputTokens == nil || *value.View.Usage.OutputTokens != 0 {
			t.Fatalf("original deleted receipt erased later sparse evidence: %+v %v", value, err)
		}
	}
	for _, id := range []string{parent.View.ID, session.View.ID} {
		budget, err := store.Budget(ctx, id)
		if err != nil || budget.ReportedTokens != "7" || budget.ReservedTokens != "0" || budget.IncompleteUsages != 0 ||
			budget.ActiveOperations != 0 || budget.ReservedOutputBytes != 0 || budget.Calls != 1 {
			t.Fatalf("deleted descendant refinement duplicated settlement: %+v %v", budget, err)
		}
	}
	var lifecycle string
	var ready, sequence int64
	if err := store.db.QueryRowContext(ctx, store.q("SELECT lifecycle,ready,sequence FROM sessions WHERE id=?"), session.View.ID).
		Scan(&lifecycle, &ready, &sequence); err != nil {
		t.Fatal(err)
	}
	if lifecycle != "deleted" || ready != 0 || sequence != op.IntentSequence {
		t.Fatal("late usage allocated new history or revived deleted work")
	}
}
