package history_test

import (
	"context"
	"crypto/rand"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/aexhq/brain/adapters/archive"
	"github.com/aexhq/brain/adapters/sqlcontrol"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/history"
	"github.com/aexhq/brain/internal/kernel"
)

func TestContextUsesNewestBoundedSnapshotAndIgnoresFutureGaps(t *testing.T) {
	directory := filepath.Join("..", "..", ".test-output", "context-"+rand.Text())
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(directory) })
	ctx := context.Background()
	control, err := sqlcontrol.Open(ctx, filepath.Join(directory, "control.sqlite"), true)
	if err != nil {
		t.Fatal(err)
	}
	defer control.Close()
	if err = control.RegisterNode(ctx, "owner", 60000); err != nil {
		t.Fatal(err)
	}
	session, err := control.Create(ctx, "tenant", wire.SessionCreate{RequestID: "new", Config: wire.SessionConfig{Limits: wire.Limits{
		Calls: 100, Concurrent: 8, InputBytes: 65536, OutputBytes: 65536, RetainedBytes: 16 << 20, PendingInputs: 128,
	}}}, "", "", nil)
	if err != nil {
		t.Fatal(err)
	}
	owner, err := control.ClaimOwner(ctx, session.View.ID, "owner")
	if err != nil {
		t.Fatal(err)
	}
	blobs, err := archive.OpenLocal(filepath.Join(directory, "archive"))
	if err != nil {
		t.Fatal(err)
	}
	defer blobs.Close()
	records, err := history.Open(ctx, history.Config{Directory: filepath.Join(directory, "history"), Shards: 1,
		CacheBytes: 0, DiskBytes: 16 << 20, DirtyBytes: 8 << 20}, control, blobs, "owner")
	if err != nil {
		t.Fatal(err)
	}
	defer records.Close()
	var last int64
	for i := 0; i < 10; i++ {
		last, err = control.Allocate(ctx, session.View.ID, owner)
		if err != nil {
			t.Fatal(err)
		}
		record := wire.Record{SessionID: session.View.ID, Sequence: strconv.FormatInt(last, 10), Kind: "context",
			Data: kernel.JSON(map[string]string{"body": strings.Repeat("x", 190000)})}
		if _, err = records.Append(ctx, record, ""); err != nil {
			t.Fatal(err)
		}
		if err = control.Written(ctx, session.View.ID, owner, last, kernel.Digest(kernel.JSON(record)), int64(len(kernel.JSON(record)))); err != nil {
			t.Fatal(err)
		}
	}
	future, err := control.Allocate(ctx, session.View.ID, owner)
	if err != nil {
		t.Fatal(err)
	}
	snapshot, err := records.Context(ctx, session.View.ID, last)
	if err != nil || len(snapshot) != 2 || snapshot[len(snapshot)-1].Sequence != strconv.FormatInt(last, 10) {
		t.Fatalf("snapshot dropped newest records or included a later gap: %d %v", len(snapshot), err)
	}
	if _, err = records.Context(ctx, session.View.ID, future); err == nil {
		t.Fatal("unresolved snapshot position was silently omitted")
	}
	if err = control.SealUnused(ctx, session.View.ID, owner, future); err != nil {
		t.Fatal(err)
	}
	if _, err = records.Context(ctx, session.View.ID, future); err != nil {
		t.Fatalf("positively unused position blocked valid context: %v", err)
	}
}
