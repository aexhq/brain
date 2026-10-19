package core

import "testing"

func TestInvalidModelEvidenceCannotBecomeAccountingData(t *testing.T) {
	for _, mutate := range []func(*ModelRequest){
		func(r *ModelRequest) { r.State = "success-ish" },
		func(r *ModelRequest) { r.Usage.Availability = "reported" },
		func(r *ModelRequest) { v := int64(1); r.Usage.InputTokens = &v },
		func(r *ModelRequest) { v := int64(-1); r.Usage.Availability = "partial"; r.Usage.OutputTokens = &v },
		func(r *ModelRequest) { r.AttemptID = ID() },
		func(r *ModelRequest) { r.Usage.Semantics = "additive" },
	} {
		r := ModelRequest{RequestID: ID(), Purpose: "primary", Boundary: "request", State: "started", Attempts: "unobserved", Usage: UnknownUsage()}
		if err := r.Validate(); err != nil {
			t.Fatal(err)
		}
		mutate(&r)
		if r.Validate() == nil {
			t.Fatalf("invalid evidence accepted: %+v", r)
		}
	}
}
