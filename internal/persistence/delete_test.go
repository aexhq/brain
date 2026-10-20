package persistence

import (
	"context"
	"errors"
	"github.com/aexhq/brain/internal/core"
	"os"
	"testing"
)

type failedDelete struct {
	Objects
	fail bool
}

func (o *failedDelete) DeletePrefix(ctx context.Context, prefix string) error {
	if o.fail {
		return errors.New("storage unavailable")
	}
	return o.Objects.DeletePrefix(ctx, prefix)
}

func TestDeleteTombstoneSurvivesFailedCleanupAndBlocksEveryCheckpoint(t *testing.T) {
	a, objects, snapshot, log := fixture(t)
	ctx := context.Background()
	appendEvent(t, log, "private history")
	snapshot.HistoryBytes = log.Size()
	manifest, err := a.Publish(ctx, snapshot)
	if err != nil {
		t.Fatal(err)
	}
	failed := &failedDelete{Objects: objects, fail: true}
	a.Objects = failed
	if err := a.Delete(ctx, snapshot.Session.Tenant, snapshot.Session.ID); err == nil {
		t.Fatal("cleanup failure ignored")
	}
	for _, ref := range []string{"", manifest.ID} {
		if _, _, err := a.Restore(ctx, snapshot.Session.Tenant, snapshot.Session.ID, ref, t.TempDir()); core.ErrorCode(err) != core.NotFound {
			t.Fatal(err)
		}
	}
	if _, err := a.Publish(ctx, snapshot); core.ErrorCode(err) != core.NotFound {
		t.Fatal(err)
	}
	failed.fail = false
	if err := a.Delete(ctx, snapshot.Session.Tenant, snapshot.Session.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := objects.Get(ctx, manifestKey(snapshot.Session.ID, manifest.ID)); !os.IsNotExist(err) {
		t.Fatal(err)
	}
	if err := a.Delete(ctx, snapshot.Session.Tenant, snapshot.Session.ID); err != nil {
		t.Fatal(err)
	}
}
