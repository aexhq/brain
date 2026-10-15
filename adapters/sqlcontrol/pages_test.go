package sqlcontrol

import (
	"context"
	"fmt"
	"strings"
	"testing"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestByteBoundedRunPagesAndReadyWorkBeyondWaitingHistory(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	payload := string(kernel.JSON(wire.InputData{Kind: "message", Text: kernel.Ptr(strings.Repeat("x", 32768))}))
	tx, err := store.db.BeginTx(ctx, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer tx.Rollback()
	for i := 0; i < 161; i++ {
		id, status, wait := fmt.Sprintf("run-%03d", i), "waiting", "input"
		if i == 160 {
			id, status, wait = "zz-ready", "queued", ""
		}
		if _, err = tx.ExecContext(ctx, store.q(`INSERT INTO commands(id,session_id,request_key,digest,payload,source,status,accepted_ms,kind)
VALUES(?,?,?,?,?,'fixture','applied',1,'message')`), id, session.View.ID, id, kernel.Digest([]byte(payload)), payload); err != nil {
			t.Fatal(err)
		}
		if _, err = tx.ExecContext(ctx, store.q("INSERT INTO runs(id,session_id,command_id,status,wait_kind,updated_ms) VALUES(?,?,?,?,?,1)"),
			id, session.View.ID, id, status, wait); err != nil {
			t.Fatal(err)
		}
	}
	if err = tx.Commit(); err != nil {
		t.Fatal(err)
	}
	after, count, pages := "", 0, 0
	for {
		page, err := store.Runs(ctx, session.View.ID, 128, after)
		if err != nil {
			t.Fatal(err)
		}
		if len(page) == 0 {
			break
		}
		if len(kernel.JSON(page)) > 2<<20 {
			t.Fatal("run page exceeded its SQL-side byte bound")
		}
		count += len(page)
		pages++
		next := page[len(page)-1].View.ID
		if next <= after {
			t.Fatal("run page did not advance")
		}
		after = next
	}
	if count != 161 || pages < 3 {
		t.Fatalf("incomplete bounded traversal: %d runs in %d pages", count, pages)
	}
	runnable, err := store.RunnableRuns(ctx, session.View.ID, 64)
	if err != nil || len(runnable) != 1 || runnable[0].View.ID != "zz-ready" {
		t.Fatalf("waiting history hid executable work: %+v %v", runnable, err)
	}
}

func TestChildSummaryIncludesAcceptedInputsAndBlockedRuns(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	command, err := store.Accept(ctx, kernel.Principal{Tenant: "tenant", Subject: "operator"}, session.View.ID,
		wire.Input{RequestID: "child-input", Data: wire.InputData{Kind: "message", Text: kernel.Ptr("child")}})
	if err != nil {
		t.Fatal(err)
	}
	state, err := store.ChildState(ctx, session.View.ID)
	if err != nil || state.Pending != 1 || state.Completed != 0 {
		t.Fatalf("an unprocessed child input looked complete: %+v %v", state, err)
	}
	if err = store.ApplyInput(ctx, session.View.ID, control.ApplyInput{Owner: session.Owner, Command: command.ID, Run: true, Sequence: 1}); err != nil {
		t.Fatal(err)
	}
	run := kernel.ID("run", session.View.ID, command.ID)
	if err = store.AdvanceRun(ctx, session.View.ID, session.Owner, run, 0, wire.RunStatusBlocked, nil, &wire.ErrorResponse{Code: wire.ErrorCodeUncertain, Message: "original result unknown"}); err != nil {
		t.Fatal(err)
	}
	state, err = store.ChildState(ctx, session.View.ID)
	if err != nil || state.Pending != 0 || state.Failed != 1 || state.Completed != 0 {
		t.Fatalf("blocked child was reported as successful completion: %+v %v", state, err)
	}
}
