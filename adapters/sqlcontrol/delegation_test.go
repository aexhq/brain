package sqlcontrol

import (
	"context"
	"strings"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func TestDelegationLinkRollsBackWithChildAdmission(t *testing.T) {
	store, parent := testStore(t)
	ctx := context.Background()
	config := parent.View.Config
	config.Limits.InputBytes = 128
	request := wire.SessionCreate{RequestID: "invalid-child", Config: config, Message: kernel.Ptr(strings.Repeat("x", 129))}
	_, err := store.Create(ctx, "tenant", request, parent.View.ID, parent.Budget, nil)
	if kernel.Response(err).Code != wire.ErrorCodeInvalid {
		t.Fatalf("oversized child input was admitted: %v", err)
	}
	var sessions, commands, pools int
	if err = store.db.QueryRowContext(ctx, "SELECT COUNT(*) FROM sessions").Scan(&sessions); err != nil {
		t.Fatal(err)
	}
	if err = store.db.QueryRowContext(ctx, "SELECT COUNT(*) FROM commands").Scan(&commands); err != nil {
		t.Fatal(err)
	}
	if err = store.db.QueryRowContext(ctx, "SELECT COUNT(*) FROM budgets").Scan(&pools); err != nil {
		t.Fatal(err)
	}
	if sessions != 1 || pools != 1 || commands != 0 {
		t.Fatalf("partial child or parent audit escaped rollback: sessions=%d pools=%d commands=%d", sessions, pools, commands)
	}
	request.Message = kernel.Ptr("small")
	child, err := store.Create(ctx, "tenant", request, parent.View.ID, parent.Budget, nil)
	if err != nil {
		t.Fatal(err)
	}
	repeated, err := store.Create(ctx, "tenant", request, parent.View.ID, parent.Budget, nil)
	if err != nil || repeated.View.ID != child.View.ID {
		t.Fatalf("delegation reconciliation created another child: %+v %v", repeated, err)
	}
	if err = store.db.QueryRowContext(ctx, "SELECT COUNT(*) FROM commands").Scan(&commands); err != nil {
		t.Fatal(err)
	}
	if commands != 2 {
		t.Fatalf("child input or parent linkage was repeated: %d", commands)
	}
}
