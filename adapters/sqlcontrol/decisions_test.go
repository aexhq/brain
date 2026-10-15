package sqlcontrol

import (
	"context"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestDecisionAdmissionPreservesNonrepeatabilityAndInputDigest(t *testing.T) {
	store, session := testStore(t)
	ctx := context.Background()
	for _, repeatable := range []bool{false, true} {
		name := map[bool]string{false: "nonrepeatable", true: "checkpointed"}[repeatable]
		run := budgetOperation(t, store, session, name).Run
		hash := kernel.Digest(kernel.JSON(name))
		if err := store.BeginDecision(ctx, session.View.ID, session.Owner, run, 0, hash, "original-process", repeatable); err != nil {
			t.Fatal(err)
		}
		err := store.BeginDecision(ctx, session.View.ID, session.Owner, run, 0, hash, "replacement-process", true)
		if repeatable && err != nil {
			t.Fatal(err)
		}
		if !repeatable && kernel.Response(err).Code != wire.ErrorCodeUnsupported {
			t.Fatalf("new manifest amplified an old nonrepeatable invocation: %v", err)
		}
		if err = store.BeginDecision(ctx, session.View.ID, session.Owner, run, 0, kernel.Digest(kernel.JSON("changed")),
			"replacement-process", true); kernel.Response(err).Code != wire.ErrorCodeCorrupt {
			t.Fatalf("divergent decision arguments were admitted: %v", err)
		}
		expected := map[bool]string{false: "original-process", true: "replacement-process"}[repeatable]
		runs, err := store.Runs(ctx, session.View.ID, 64)
		if err != nil {
			t.Fatal(err)
		}
		found := false
		for _, value := range runs {
			if value.View.ID == run {
				found = value.HarnessProcess == expected
			}
		}
		if !found {
			t.Fatal("the durable run lost its admitted process identity")
		}
		decision := wire.Decision{Kind: "complete", Result: kernel.JSON("answer"), Checkpoint: kernel.JSON(map[string]any{})}
		if err = store.SaveDecision(ctx, session.View.ID, session.Owner, run, 0, decision, "unadmitted-process"); kernel.Response(err).Code != wire.ErrorCodeConflict {
			t.Fatalf("an unadmitted process saved a decision: %v", err)
		}
		if err = store.SaveDecision(ctx, session.View.ID, session.Owner, run, 0, decision, expected); err != nil {
			t.Fatal(err)
		}
	}
}
