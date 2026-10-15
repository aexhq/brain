package server

import (
	"context"
	"encoding/json"
	"net/http"
	"strconv"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

func (s *Server) children(w http.ResponseWriter, r *http.Request, _ kernel.Principal) error {
	step, err := strconv.ParseUint(r.URL.Query().Get("step"), 10, 32)
	if err != nil {
		return kernel.Fail(wire.ErrorCodeInvalid, "a zero-based delegation step is required")
	}
	result, err := s.childResults(r.Context(), r.PathValue("session"), r.PathValue("run"), step)
	if err != nil {
		return err
	}
	return writeJSON(w, result)
}

func (s *Server) childResult(w http.ResponseWriter, r *http.Request, _ kernel.Principal) error {
	step, err := strconv.ParseUint(r.URL.Query().Get("step"), 10, 32)
	if err != nil {
		return kernel.Fail(wire.ErrorCodeInvalid, "a zero-based delegation step is required")
	}
	index, err := strconv.ParseUint(r.PathValue("child"), 10, 8)
	if err != nil || index >= kernel.MaxForkChildren {
		return kernel.Fail(wire.ErrorCodeInvalid, "child index must be between zero and seven")
	}
	result, err := s.Control.Children(r.Context(), r.PathValue("session"), r.PathValue("run"), step)
	if err != nil {
		return err
	}
	for _, child := range result.Children {
		if child.Index == index {
			record, _, err := s.loadChildCompletion(r.Context(), child)
			if err != nil {
				return err
			}
			return writeJSON(w, record)
		}
	}
	return kernel.Fail(wire.ErrorCodeNotFound, "this decision slot has not admitted a child")
}

func (s *Server) loadChildCompletion(ctx context.Context, child wire.ChildResult) (wire.Record, json.RawMessage, error) {
	if child.Status != wire.RunStatusCompleted {
		return wire.Record{}, nil, kernel.Fail(wire.ErrorCodeConflict, "original child has no completed result")
	}
	if child.Completion == nil {
		return wire.Record{}, nil, kernel.Fail(wire.ErrorCodeUnavailable, "known completion has no retained result reference")
	}
	reference := child.Completion
	sequence, err := kernel.Sequence(reference.Sequence)
	if err != nil {
		return wire.Record{}, nil, err
	}
	record, err := s.History.Get(ctx, reference.SessionID, sequence, reference.Digest)
	if err != nil {
		return record, nil, err
	}
	var body struct {
		Result json.RawMessage `json:"result"`
	}
	if record.Kind != "run.completed" || kernel.Value(record.RunID) != child.RunID ||
		json.Unmarshal(record.Data, &body) != nil || !json.Valid(body.Result) {
		return record, nil, kernel.Fail(wire.ErrorCodeCorrupt, "child completion record does not match its retained reference")
	}
	return record, body.Result, nil
}

func (s *Server) childResults(ctx context.Context, session, run string, step uint64) (wire.ChildResults, error) {
	results, err := s.Control.Children(ctx, session, run, step)
	if err != nil {
		return results, err
	}
	remaining := 128 << 10
	for index := range results.Children {
		child := &results.Children[index]
		if child.Status != wire.RunStatusCompleted {
			continue
		}
		_, result, err := s.loadChildCompletion(ctx, *child)
		if err != nil {
			failure := kernel.Response(err)
			if failure.Code == wire.ErrorCodeOverloaded || failure.Code == wire.ErrorCodeCorrupt {
				return results, err
			}
			child.Availability = wire.ChildResultAvailabilityUnavailable
			child.Error = &failure
			continue
		}
		child.Availability = wire.ChildResultAvailabilityReferenced
		if len(result) <= remaining {
			child.Result = result
			child.Availability = wire.ChildResultAvailabilityInline
			remaining -= len(result)
		}
	}
	return results, nil
}
