package sqlcontrol

import (
	"context"
	"crypto/rand"
	"database/sql"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func testStore(t *testing.T) (*Store, control.Session) {
	t.Helper()
	directory := filepath.Join("..", "..", ".test-output", "control-"+rand.Text())
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	dsn := filepath.Join(directory, "control.sqlite")
	if configured := os.Getenv("BRAIN_TEST_POSTGRES_URL"); configured != "" {
		u, err := url.Parse(configured)
		if err != nil || u.Hostname() != "127.0.0.1" || os.Getenv("BRAIN_TEST_POSTGRES_CONTAINER") == "" {
			t.Fatal("PostgreSQL conformance needs its disposable loopback container")
		}
		admin, err := sql.Open("pgx", configured)
		if err != nil {
			t.Fatal(err)
		}
		namespace := "brain_test_" + strings.ToLower(rand.Text())
		if _, err = admin.Exec(`CREATE SCHEMA "` + namespace + `"`); err != nil {
			admin.Close()
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if _, err := admin.Exec(`DROP SCHEMA "` + namespace + `" CASCADE`); err != nil {
				t.Error(err)
			}
			admin.Close()
		})
		query := u.Query()
		query.Set("search_path", namespace)
		u.RawQuery = query.Encode()
		dsn = u.String()
	}
	store, err := Open(context.Background(), dsn, true)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close(); _ = os.RemoveAll(directory) })
	ctx := context.Background()
	if err = store.RegisterNode(ctx, "test-incarnation", 60000); err != nil {
		t.Fatal(err)
	}
	session, err := store.Create(ctx, "tenant", wire.SessionCreate{RequestID: "new-session", Config: wire.SessionConfig{Limits: wire.Limits{
		Calls: 100, Concurrent: 64, InputBytes: 65536, OutputBytes: 65536, RetainedBytes: 16 << 20, PendingInputs: 128,
	}}}, "", "", nil)
	if err != nil {
		t.Fatal(err)
	}
	session.Owner, err = store.ClaimOwner(ctx, session.View.ID, "test-incarnation")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = store.Pending(ctx, "test-incarnation", 8); err != nil {
		t.Fatal(err)
	}
	return store, session
}

func testOperation(t *testing.T, store *Store, session control.Session, name string) control.Operation {
	t.Helper()
	ctx := context.Background()
	receipt, err := store.Accept(ctx, kernel.Principal{Tenant: "tenant", Subject: "operator"}, session.View.ID, wire.Input{RequestID: name, Data: wire.InputData{Kind: "message", Text: kernel.Ptr(name)}})
	if err != nil {
		t.Fatal(err)
	}
	if err = store.ApplyInput(ctx, session.View.ID, control.ApplyInput{Command: receipt.ID, Owner: session.Owner, Run: true, Sequence: 1}); err != nil {
		t.Fatal(err)
	}
	sequence, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	digest := kernel.Digest([]byte("intent:" + name))
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, digest, 64); err != nil {
		t.Fatal(err)
	}
	op, err := store.Prepare(ctx, session.Owner, control.Prepare{ID: name, Session: session.View.ID, Run: kernel.ID("run", session.View.ID, receipt.ID),
		StepKey: "0", Kind: wire.EffectKindModel, Action: "generate", Arguments: kernel.JSON(map[string]string{"input": name}),
		Recipient: "model", OutputLimit: 8192, Timeout: 30000, IntentSequence: sequence, IntentDigest: digest})
	if err != nil {
		t.Fatal(err)
	}
	return op
}

func TestOneDurableClaimAndNoReissuedPermission(t *testing.T) {
	store, session := testStore(t)
	op := testOperation(t, store, session, "once")
	var group sync.WaitGroup
	var accepted atomic.Int32
	for i := 0; i < 32; i++ {
		group.Add(1)
		go func() {
			defer group.Done()
			_, err := store.ClaimDispatch(context.Background(), session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "same-claim")
			if err == nil {
				accepted.Add(1)
			} else if kernel.Response(err).Code != wire.ErrorCodeConflict {
				t.Error(err)
			}
		}()
	}
	group.Wait()
	if accepted.Load() != 1 {
		t.Fatalf("expected one execution permit, got %d", accepted.Load())
	}
}

func TestUnknownDeadlineRetainsReserveAndLateCompletionSettlesIt(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	op := testOperation(t, store, session, "late")
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "dispatch"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.db.ExecContext(ctx, store.q("UPDATE operations SET deadline_ms=0 WHERE id=?"), op.View.ID); err != nil {
		t.Fatal(err)
	}
	if count, err := store.ExpireOperations(ctx, 8); err != nil || count != 1 {
		t.Fatalf("expiry: %d %v", count, err)
	}
	value, err := store.Operation(ctx, session.View.ID, op.View.ID)
	if err != nil || value.View.Status != wire.OperationStatusUnknown {
		t.Fatalf("unknown: %+v %v", value, err)
	}
	var active, reserved int64
	if err = store.db.QueryRowContext(ctx, store.q("SELECT active,reserved FROM budgets WHERE id=?"), session.Budget).Scan(&active, &reserved); err != nil {
		t.Fatal(err)
	}
	if active != 1 || reserved == 0 {
		t.Fatal("uncertain outcome released its admitted capacity")
	}
	hold := reserved
	unknown, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	unknownDigest := kernel.Digest([]byte("original unknown evidence"))
	if err = store.Written(ctx, session.View.ID, session.Owner, unknown, unknownDigest, 32); err != nil {
		t.Fatal(err)
	}
	if err = store.BindResult(ctx, session.View.ID, session.Owner, op.View.ID, unknown, unknownDigest); err != nil {
		t.Fatal(err)
	}
	unknownBudget, err := store.Budget(ctx, session.View.ID)
	if err != nil || unknownBudget.ReservedOutputBytes != uint64(hold) {
		t.Fatalf("binding unknown evidence released late-result capacity: %+v %v", unknownBudget, err)
	}
	receipt := wire.Receipt{RequestID: "late-result", Outcome: wire.OutcomeCompleted, Result: kernel.JSON(map[string]bool{"delivered": true}), Usage: &wire.Usage{InputTokens: kernel.Ptr(uint64(3))}}
	value, err = store.RecordReceipt(ctx, session.View.ID, op.View.ID, receipt, 0, "")
	if err != nil || value.View.Status != wire.OperationStatusCompleted {
		t.Fatalf("late: %+v %v", value, err)
	}
	if value.View.Usage.OutputTokens != nil {
		t.Fatal("absent output usage was manufactured")
	}
	if _, err = store.RecordReceipt(ctx, session.View.ID, op.View.ID, receipt, 0, ""); err != nil {
		t.Fatal(err)
	}
	if err = store.db.QueryRowContext(ctx, store.q("SELECT active,reserved FROM budgets WHERE id=?"), session.Budget).Scan(&active, &reserved); err != nil {
		t.Fatal(err)
	}
	if active != 0 || reserved != hold {
		t.Fatal("receipt settlement released the unbound result reservation")
	}
	sequence, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	digest := kernel.Digest([]byte("original completed evidence"))
	if err = store.BindResult(ctx, session.View.ID, session.Owner, op.View.ID, sequence, digest); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("unwritten result released the reservation: %v", err)
	}
	if err = store.Written(ctx, session.View.ID, session.Owner, sequence, digest, 32); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		if err = store.BindResult(ctx, session.View.ID, session.Owner, op.View.ID, sequence, digest); err != nil {
			t.Fatal(err)
		}
	}
	if err = store.db.QueryRowContext(ctx, store.q("SELECT active,reserved FROM budgets WHERE id=?"), session.Budget).Scan(&active, &reserved); err != nil {
		t.Fatal(err)
	}
	if active != 0 || reserved != 0 {
		t.Fatal("written result reservation was not released exactly once")
	}
}

func TestCoverageDoesNotEraseAnUnincludedResult(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	op := testOperation(t, store, session, "sparse")
	if _, err := store.ClaimDispatch(ctx, session.View.ID, session.Owner, op.View.ID, session.Owner.Node, "dispatch"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.RecordReceipt(ctx, session.View.ID, op.View.ID, wire.Receipt{RequestID: "result", Outcome: wire.OutcomeCompleted, Result: kernel.JSON("retained")}, 0, ""); err != nil {
		t.Fatal(err)
	}
	result, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	resultDigest := kernel.Digest([]byte("result"))
	if err = store.Written(ctx, session.View.ID, session.Owner, result, resultDigest, 32); err != nil {
		t.Fatal(err)
	}
	if err = store.BindResult(ctx, session.View.ID, session.Owner, op.View.ID, result, resultDigest); err != nil {
		t.Fatal(err)
	}
	last, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	lastDigest := kernel.Digest([]byte("last"))
	if err = store.Written(ctx, session.View.ID, session.Owner, last, lastDigest, 32); err != nil {
		t.Fatal(err)
	}
	if err = store.PublishCoverage(ctx, session.Owner, control.Coverage{Session: session.View.ID, First: op.IntentSequence, Last: last,
		Key: "sparse-manifest", Digest: "manifest-digest", Bytes: 96, Generation: session.Generation,
		Records: []control.Allocation{{Sequence: op.IntentSequence, Digest: op.IntentDigest}, {Sequence: last, Digest: lastDigest}}}); err != nil {
		t.Fatal(err)
	}
	value, err := store.Operation(ctx, session.View.ID, op.View.ID)
	if err != nil {
		t.Fatal(err)
	}
	if !value.View.ResultAvailable || string(value.View.Result) != `"retained"` {
		t.Fatal("sparse envelope erased bytes that were never archived")
	}
	coverage, err := store.Coverage(ctx, session.View.ID, result-1, 32)
	if err != nil || len(coverage) != 0 {
		t.Fatalf("invented result coverage: %+v %v", coverage, err)
	}
}

func TestLostReservedPositionsAreNotUnusedAndNeverReused(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	first, err := store.Allocate(ctx, session.View.ID, session.Owner)
	if err != nil {
		t.Fatal(err)
	}
	if err = store.RetireNode(ctx, session.Owner.Node); err != nil {
		t.Fatal(err)
	}
	if err = store.RegisterNode(ctx, "new-incarnation", 60000); err != nil {
		t.Fatal(err)
	}
	owner, err := store.ClaimOwner(ctx, session.View.ID, "new-incarnation")
	if err != nil {
		t.Fatal(err)
	}
	allocations, _, err := store.Allocations(ctx, session.View.ID, 0, 32)
	if err != nil {
		t.Fatal(err)
	}
	if allocations[0].Status != "unresolved" {
		t.Fatalf("lost reserved position classified as %s", allocations[0].Status)
	}
	if err = store.SealUnused(ctx, session.View.ID, owner, first); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("new owner guessed an old position unused: %v", err)
	}
	next, err := store.Allocate(ctx, session.View.ID, owner)
	if err != nil || next <= first {
		t.Fatalf("position reused: %d %v", next, err)
	}
	if _, err = store.Allocate(ctx, session.View.ID, session.Owner); kernel.Response(err).Code != wire.ErrorCodeConflict {
		t.Fatalf("stale owner was not fenced: %v", err)
	}
}
