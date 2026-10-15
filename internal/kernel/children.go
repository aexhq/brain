package kernel

import (
	"encoding/json"
	"strconv"
	"strings"

	"github.com/aexhq/brain/internal/generated/wire"
)

const MaxForkChildren = 8

func Delegations(decision wire.Decision) ([]wire.Delegation, error) {
	if decision.Kind == "delegate" && decision.Delegation != nil {
		return []wire.Delegation{*decision.Delegation}, nil
	}
	if decision.Kind == "fork" && len(decision.Delegations) > 0 && len(decision.Delegations) <= MaxForkChildren {
		return decision.Delegations, nil
	}
	return nil, Fail(wire.ErrorCodeInvalid, "a delegation decision requires one to eight children")
}

func ChildCreateID(session, run string, step, index uint64) string {
	return ID("child", session, ID("delegation", session, run, strconv.FormatUint(step, 10), strconv.FormatUint(index, 10)))
}

func ValidateChildDecision(decision wire.Decision, index uint64, request wire.SessionCreate) error {
	children, err := Delegations(decision)
	if err != nil {
		return err
	}
	if index >= uint64(len(children)) || request.Message == nil {
		return Fail(wire.ErrorCodeConflict, "child is outside its saved decision")
	}
	child := children[index]
	if !EqualJSON(child.Config, request.Config) || (len(child.Context) == 0 && child.Message != *request.Message) ||
		(len(child.Context) != 0 && !strings.HasPrefix(*request.Message, child.Message+"\nUntrusted shared observation:\n")) {
		return Fail(wire.ErrorCodeConflict, "child differs from its saved decision")
	}
	return nil
}

func CompletionReference(session, run string, record wire.Record) (wire.RecordReference, error) {
	sequence, err := Sequence(record.Sequence)
	if err != nil || sequence <= 0 || record.SessionID != session || Value(record.RunID) != run ||
		record.Kind != "run.completed" || record.OperationID != nil || !json.Valid(record.Data) {
		return wire.RecordReference{}, Fail(wire.ErrorCodeInvalid, "completion must name its original run record")
	}
	return wire.RecordReference{SessionID: session, Sequence: record.Sequence, Digest: Digest(JSON(record))}, nil
}

func ValidateCompletion(decision wire.Decision, record wire.Record) error {
	if decision.Kind != "complete" || !json.Valid(decision.Result) ||
		!EqualJSON(record.Data, JSON(map[string]any{"result": json.RawMessage(decision.Result)})) {
		return Fail(wire.ErrorCodeConflict, "completion differs from its saved decision")
	}
	return nil
}
