package history

import (
	"bytes"
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

func TestRetainedWALReconcilesOriginalBytesWithoutInventingMissingPositions(t *testing.T) {
	directory := filepath.Join("..", "..", ".test-output", "retained-"+rand.Text())
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
	for _, node := range []string{"original", "successor", "returned"} {
		if err = authority.RegisterNode(ctx, node, 60000); err != nil {
			t.Fatal(err)
		}
	}
	session, err := authority.Create(ctx, "tenant", wire.SessionCreate{RequestID: "retained", Config: wire.SessionConfig{Limits: wire.Limits{
		Calls: 100, Concurrent: 8, InputBytes: 65536, OutputBytes: 65536, RetainedBytes: 16 << 20, PendingInputs: 128,
	}}}, "", "", nil)
	if err != nil {
		t.Fatal(err)
	}
	original, err := authority.ClaimOwner(ctx, session.View.ID, "original")
	if err != nil {
		t.Fatal(err)
	}
	blobs, err := archive.OpenLocal(filepath.Join(directory, "archive"))
	if err != nil {
		t.Fatal(err)
	}
	defer blobs.Close()
	config := Config{Directory: filepath.Join(directory, "history"), Shards: 1, CacheBytes: 0, DiskBytes: 16 << 20, DirtyBytes: 8 << 20}
	records, err := Open(ctx, config, authority, blobs, original.Node)
	if err != nil {
		t.Fatal(err)
	}
	var first wire.Record
	for i := 0; i < confirmationBatch+1; i++ {
		sequence, err := authority.Allocate(ctx, session.View.ID, original)
		if err != nil {
			t.Fatal(err)
		}
		record := wire.Record{SessionID: session.View.ID, Sequence: strconv.FormatInt(sequence, 10), Kind: "historical",
			Data: kernel.JSON(map[string]int{"original": i})}
		if i == 0 {
			first = record
		}
		if _, err = records.Append(ctx, record, "", original); err != nil {
			t.Fatal(err)
		}
	}
	legacy, err := authority.Allocate(ctx, session.View.ID, original)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = records.append(ctx, wire.Record{SessionID: session.View.ID, Sequence: strconv.FormatInt(legacy, 10),
		Kind: "legacy-without-proof", Data: kernel.JSON("retained but unconfirmed")}, "", false, control.Owner{}); err != nil {
		t.Fatal(err)
	}
	unused, err := authority.Allocate(ctx, session.View.ID, original)
	if err != nil {
		t.Fatal(err)
	}
	if err = authority.SealUnused(ctx, session.View.ID, original, unused); err != nil {
		t.Fatal(err)
	}
	absent, err := authority.Allocate(ctx, session.View.ID, original)
	if err != nil {
		t.Fatal(err)
	}
	if err = records.Close(); err != nil {
		t.Fatal(err)
	}
	if err = authority.RetireNode(ctx, original.Node); err != nil {
		t.Fatal(err)
	}
	successor, err := authority.ClaimOwner(ctx, session.View.ID, "successor")
	if err != nil {
		t.Fatal(err)
	}
	records, err = Open(ctx, config, authority, blobs, "returned")
	if err != nil {
		t.Fatal(err)
	}
	defer records.Close()
	db := records.shard(session.View.ID)
	wrongKey := first
	wrongKey.Sequence = "2"
	if _, err = db.Exec("UPDATE records SET body=? WHERE session_id=? AND sequence=1", []byte("changed bytes"), session.View.ID); err != nil {
		t.Fatal(err)
	}
	if err = records.ReconcileSession(ctx, session.View.ID); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("a retained digest mismatch was ignored: %v", err)
	}
	for _, candidate := range []struct {
		name  string
		body  []byte
		epoch int64
	}{
		{"invalid JSON", []byte("{"), original.Epoch},
		{"different record key", kernel.JSON(wrongKey), original.Epoch},
		{"oversized physical row", bytes.Repeat([]byte("x"), MaxFrame+1), original.Epoch},
		{"missing original epoch", kernel.JSON(first), 0},
	} {
		if _, err = db.Exec("UPDATE records SET body=?,digest=?,original_epoch=? WHERE session_id=? AND sequence=1",
			candidate.body, kernel.Digest(candidate.body), candidate.epoch, session.View.ID); err != nil {
			t.Fatal(err)
		}
		if err = records.ReconcileSession(ctx, session.View.ID); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
			t.Fatalf("%s was accepted as original evidence: %v", candidate.name, err)
		}
	}
	if _, err = db.Exec("UPDATE records SET body=?,digest=?,original_epoch=? WHERE session_id=? AND sequence=1",
		[]byte(kernel.JSON(first)), kernel.Digest(kernel.JSON(first)), original.Epoch, session.View.ID); err != nil {
		t.Fatal(err)
	}
	if err = records.ReconcileSession(ctx, session.View.ID); kernel.Response(err).Code != wire.ErrorCodeOverloaded {
		t.Fatalf("confirmation did not yield at its finite batch bound: %v", err)
	}
	pending, err := confirmations(ctx, db, session.View.ID, confirmationCursor{})
	if err != nil || len(pending) != 1 {
		t.Fatalf("confirmation exceeded its bounded batch: %+v %v", pending, err)
	}
	if err = records.ReconcileSession(ctx, session.View.ID); err != nil {
		t.Fatal(err)
	}
	page, err := records.Page(ctx, session.View.ID, 0, 128)
	if err != nil || page.AllocatedThrough != strconv.FormatInt(absent, 10) || len(page.Records) != confirmationBatch+1 ||
		len(page.Gaps) != 3 || page.Gaps[0].Kind != wire.GapKindUnresolved ||
		page.Gaps[1].Kind != wire.GapKindUnused || page.Gaps[2].Kind != wire.GapKindUnresolved {
		t.Fatalf("returning WAL invented or dropped historical positions: %+v %v", page, err)
	}
	if string(kernel.JSON(page.Records[0])) != string(kernel.JSON(first)) {
		t.Fatal("original record identity or bytes changed")
	}
	legacyLocation, err := authority.LocateRecord(ctx, session.View.ID, legacy)
	if err != nil || legacyLocation.Status != "unresolved" {
		t.Fatalf("retained bytes without original allocation proof were promoted: %+v %v", legacyLocation, err)
	}
	if _, err = records.Context(ctx, session.View.ID, absent); kernel.Response(err).Code != wire.ErrorCodeUnavailable {
		t.Fatalf("a genuinely missing position no longer blocks selected context: %v", err)
	}
	current, err := authority.Session(ctx, "tenant", session.View.ID)
	if err != nil || current.Owner != successor {
		t.Fatalf("returning history took session execution authority: %+v %v", current, err)
	}
	location, err := authority.LocateRecord(ctx, session.View.ID, 1)
	if err != nil || location.Node != "returned" {
		t.Fatalf("returning retained bytes are not directly locatable: %+v %v", location, err)
	}
	if err = records.ReconcileSession(ctx, session.View.ID); err != nil {
		t.Fatal(err)
	}
}
