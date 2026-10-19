package session

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/aexhq/brain/internal/core"
)

func TestModelEvidenceRetainsPartialUsageWhenOutcomeIsUnknown(t *testing.T) {
	e, _, g := setup(t)
	s := create(t, e, g)
	a, err := e.load(context.Background(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	defer e.release(a)
	a.current = core.ID()
	request := core.ModelRequest{RequestID: core.ID(), Purpose: "primary", Boundary: "request", State: "started", Attempts: "unobserved", Usage: core.UnknownUsage()}
	observe := func() {
		t.Helper()
		data, _ := json.Marshal(request)
		if err := e.observeModel(a, data); err != nil {
			t.Fatal(err)
		}
	}
	observe()
	observe()
	input := int64(17)
	request.Usage.Availability = "partial"
	request.Usage.InputTokens = &input
	observe()
	observe()
	if pending, err := e.settleModelRequests(a, a.current); !pending || err != nil {
		t.Fatalf("settlement: %v %v", pending, err)
	}
	if pending, err := e.settleModelRequests(a, a.current); pending || err != nil {
		t.Fatalf("duplicate settlement: %v %v", pending, err)
	}
	events, err := a.log.Read("", 100, -1)
	if err != nil || len(events) != 3 {
		t.Fatalf("snapshots: %d %v", len(events), err)
	}
	var last core.ModelRequest
	if err := json.Unmarshal(events[2].Data, &last); err != nil {
		t.Fatal(err)
	}
	if last.State != "unknown" || last.Usage.Availability != "partial" || last.Usage.InputTokens == nil || *last.Usage.InputTokens != 17 {
		t.Fatalf("partial evidence discarded: %+v", last)
	}
}
