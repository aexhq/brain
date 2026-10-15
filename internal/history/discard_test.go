package history

import (
	"context"
	"crypto/rand"
	"os"
	"path/filepath"
	"strconv"
	"testing"

	"github.com/aexhq/brain/adapters/archive"
	"github.com/aexhq/brain/adapters/sqlcontrol"
	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestRetainedReservesRequirePositiveKnownDisposalAndNotMerelyMissingControl(t *testing.T) {
	directory := filepath.Join("..", "..", ".test-output", "discarded-"+rand.Text())
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(directory) })
	ctx := context.Background()
	authority, err := sqlcontrol.Open(ctx, filepath.Join(directory, "control.sqlite"), true)
	if err != nil {
		t.Fatal(err)
	}
	defer authority.Close()
	for _, node := range []string{"original", "returned"} {
		if err := authority.RegisterNode(ctx, node, 60000); err != nil {
			t.Fatal(err)
		}
	}
	session, err := authority.Create(ctx, "tenant", wire.SessionCreate{RequestID: "original", Config: wire.SessionConfig{Limits: wire.Limits{
		Calls: 100, Concurrent: 8, InputBytes: 65536, OutputBytes: 65536, RetainedBytes: 16 << 20, PendingInputs: 128,
	}}}, "", "", nil)
	if err != nil {
		t.Fatal(err)
	}
	owner, err := authority.ClaimOwner(ctx, session.View.ID, "original")
	if err != nil {
		t.Fatal(err)
	}
	blobs, err := archive.OpenLocal(filepath.Join(directory, "archive"))
	if err != nil {
		t.Fatal(err)
	}
	defer blobs.Close()
	config := Config{Directory: filepath.Join(directory, "history"), Shards: 1, CacheBytes: 0, DiskBytes: 16 << 20, DirtyBytes: 8 << 20}
	records, err := Open(ctx, config, authority, blobs, "original")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if records != nil {
			_ = records.Close()
		}
	}()
	appendRecord := func(kind, value string) (int64, string) {
		t.Helper()
		sequence, err := authority.Allocate(ctx, session.View.ID, owner)
		if err != nil {
			t.Fatal(err)
		}
		record := wire.Record{SessionID: session.View.ID, Sequence: strconv.FormatInt(sequence, 10), Kind: kind, Data: kernel.JSON(value)}
		hash, err := records.Append(ctx, record, "", owner)
		if err != nil {
			t.Fatal(err)
		}
		if err := authority.Written(ctx, session.View.ID, owner, sequence, hash, int64(len(kernel.JSON(record)))); err != nil {
			t.Fatal(err)
		}
		if err := records.Confirm(ctx, record); err != nil {
			t.Fatal(err)
		}
		return sequence, hash
	}
	reserve := kernel.OutputReservation(8192)
	for _, name := range []string{"known", "unknown"} {
		input, err := authority.Accept(ctx, kernel.Principal{Tenant: "tenant", Subject: "operator"}, session.View.ID,
			wire.Input{RequestID: name, Data: wire.InputData{Kind: "message", Text: kernel.Ptr(name)}})
		if err != nil {
			t.Fatal(err)
		}
		sequence, _ := appendRecord("input", name)
		if err := authority.ApplyInput(ctx, session.View.ID, control.ApplyInput{Command: input.ID, Owner: owner, Run: true, Sequence: sequence}); err != nil {
			t.Fatal(err)
		}
		if err := records.Reserve(ctx, session.View.ID, name, reserve); err != nil {
			t.Fatal(err)
		}
		sequence, hash := appendRecord("operation.intent", name)
		if _, err := authority.Prepare(ctx, owner, control.Prepare{ID: name, Session: session.View.ID, Run: kernel.ID("run", session.View.ID, input.ID),
			StepKey: "0", Kind: wire.EffectKindModel, Action: "generate", Arguments: kernel.JSON(map[string]string{"input": name}),
			Recipient: "model", OutputLimit: 8192, Timeout: 30000, IntentSequence: sequence, IntentDigest: hash}); err != nil {
			t.Fatal(err)
		}
		if _, err := authority.ClaimDispatch(ctx, session.View.ID, owner, name, owner.Node, "once"); err != nil {
			t.Fatal(err)
		}
	}
	if err := records.Reserve(ctx, session.View.ID, "missing-control", reserve); err != nil {
		t.Fatal(err)
	}
	if _, err := authority.RecordReceipt(ctx, session.View.ID, "known", wire.Receipt{
		RequestID: "known", Outcome: wire.OutcomeCompleted, Result: kernel.JSON("known original body"),
	}, 0, ""); err != nil {
		t.Fatal(err)
	}
	if _, err := authority.Delete(ctx, session.View.ID); err != nil {
		t.Fatal(err)
	}
	if count, err := authority.DiscardDeletedResults(ctx, 32); err != nil || count != 2 {
		t.Fatalf("known disposal: %d %v", count, err)
	}
	if _, err := authority.RecordReceipt(ctx, session.View.ID, "unknown", wire.Receipt{
		RequestID: "unconfirmed", Outcome: wire.OutcomeUnknown,
	}, 0, ""); err != nil {
		t.Fatal(err)
	}
	if err := records.Close(); err != nil {
		t.Fatal(err)
	}
	records, err = Open(ctx, config, authority, blobs, "returned")
	if err != nil {
		t.Fatal(err)
	}
	for range 2 {
		if err := records.Maintain(ctx); err != nil {
			t.Fatal(err)
		}
		_, _, held, _, err := records.Stats(ctx)
		if err != nil || held != 2*reserve {
			t.Fatalf("unknown or missing control released a retained hold: %d %v", held, err)
		}
	}
	if _, err := authority.RecordReceipt(ctx, session.View.ID, "unknown", wire.Receipt{
		RequestID: "original-known-result", Outcome: wire.OutcomeCompleted, Result: kernel.JSON("late original bytes"),
	}, 0, ""); err != nil {
		t.Fatal(err)
	}
	if err := records.Maintain(ctx); err != nil {
		t.Fatal(err)
	}
	_, _, held, _, err := records.Stats(ctx)
	if err != nil || held != reserve {
		t.Fatalf("positive known disposal did not release exactly its retained reservation: %d %v", held, err)
	}
}
