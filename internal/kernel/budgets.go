package kernel

import (
	"bytes"
	"encoding/json"
	"reflect"
	"strconv"

	"github.com/aexhq/brain/internal/generated/wire"
)

const MaxDelegationDepth = 8

func EqualJSON(a, b any) bool {
	decode := func(value any) any {
		var out any
		reader := json.NewDecoder(bytes.NewReader(JSON(value)))
		reader.UseNumber()
		if reader.Decode(&out) != nil {
			return nil
		}
		return out
	}
	return reflect.DeepEqual(decode(a), decode(b))
}

func ValidateDelegation(parent, child wire.SessionConfig) error {
	if err := ValidateConfig(child); err != nil {
		return err
	}
	p, c := parent.Limits, child.Limits
	if !Subset(child.Grants, parent.Grants) || !Subset(child.Skills, parent.Skills) ||
		c.Calls > p.Calls || c.Concurrent > p.Concurrent || c.InputBytes > p.InputBytes ||
		c.OutputBytes > p.OutputBytes || c.RetainedBytes > p.RetainedBytes || c.PendingInputs > p.PendingInputs ||
		(p.Tokens != nil && (c.Tokens == nil || *c.Tokens > *p.Tokens)) ||
		(p.DeadlineMs != nil && (c.DeadlineMs == nil || *c.DeadlineMs > *p.DeadlineMs)) {
		return Fail(wire.ErrorCodeForbidden, "delegation cannot amplify grants, skills or resource limits")
	}
	if !EqualJSON(parent.Model, child.Model) {
		return Fail(wire.ErrorCodeForbidden, "delegation cannot replace admitted model authority")
	}
	admitted := func(selection wire.HarnessSelection) bool {
		if EqualJSON(parent.Harness, selection) {
			return true
		}
		for _, original := range parent.HarnessTransitions {
			if EqualJSON(original, selection) {
				return true
			}
		}
		return false
	}
	if !admitted(child.Harness) {
		return Fail(wire.ErrorCodeForbidden, "delegated harness is not an admitted transition")
	}
	for _, selection := range child.HarnessTransitions {
		if !admitted(selection) {
			return Fail(wire.ErrorCodeForbidden, "delegation cannot add harness transitions")
		}
	}
	for _, connection := range child.Connections {
		found := false
		for _, original := range parent.Connections {
			found = found || EqualJSON(original, connection)
		}
		if !found {
			return Fail(wire.ErrorCodeForbidden, "delegation cannot add connections or secrets")
		}
	}
	for _, action := range child.Actions {
		original, err := Action(parent, action.Name)
		if err != nil || action.MaxOutputBytes > original.MaxOutputBytes ||
			action.TimeoutMs > original.TimeoutMs || (original.Approval && !action.Approval) {
			return Fail(wire.ErrorCodeForbidden, "delegation cannot amplify action authority")
		}
		original.MaxOutputBytes, original.TimeoutMs, original.Approval = action.MaxOutputBytes, action.TimeoutMs, action.Approval
		if !EqualJSON(original, action) {
			return Fail(wire.ErrorCodeForbidden, "delegation cannot replace action schemas or placement")
		}
	}
	for _, route := range child.Events {
		found := false
		for _, original := range parent.Events {
			if route.Name != original.Name || route.Version != original.Version {
				continue
			}
			if len(original.Sources) != 0 && (len(route.Sources) == 0 || !Subset(route.Sources, original.Sources)) {
				return Fail(wire.ErrorCodeForbidden, "delegation cannot amplify event sources")
			}
			original.Sources = route.Sources
			found = EqualJSON(original, route)
		}
		if !found {
			return Fail(wire.ErrorCodeForbidden, "delegation cannot replace event routing authority")
		}
	}
	return nil
}

func ModelOutputTokens(selection wire.ModelSelection) (int64, error) {
	var options map[string]json.RawMessage
	if len(selection.Native) != 0 && json.Unmarshal(selection.Native, &options) != nil {
		return 0, Fail(wire.ErrorCodeInvalid, "native model options must be an object")
	}
	limit := int64(4096)
	found := false
	for _, name := range []string{"max_tokens", "max_completion_tokens", "max_output_tokens"} {
		if value, ok := options[name]; ok {
			if found || json.Unmarshal(value, &limit) != nil || limit < 1 || limit > 1048576 {
				return 0, Fail(wire.ErrorCodeInvalid, "one bounded integral model output-token limit is required")
			}
			found = true
		}
	}
	return limit, nil
}

func ModelTokenReservation(config wire.SessionConfig, arguments json.RawMessage) (int64, error) {
	output, err := ModelOutputTokens(config.Model)
	if err != nil {
		return 0, err
	}
	if config.Limits.Tokens != nil {
		var options map[string]json.RawMessage
		_ = json.Unmarshal(config.Model.Native, &options)
		for _, key := range []string{"previous_response_id", "conversation", "container"} {
			if _, ok := options[key]; ok {
				return 0, Fail(wire.ErrorCodeUnsupported, "token-bounded sessions require explicit local model context")
			}
		}
		var request wire.ModelRequest
		if json.Unmarshal(arguments, &request) != nil {
			return 0, Fail(wire.ErrorCodeInvalid, "model request is invalid")
		}
		for _, message := range request.Messages {
			var text *string
			if json.Unmarshal(message.Content, &text) != nil {
				return 0, Fail(wire.ErrorCodeUnsupported, "token-bounded sessions currently require text model messages")
			}
		}
	}
	// Text bytes plus framing overestimate ordinary text tokenization. The
	// provider's actual report replaces the estimate; missing usage keeps it.
	return int64(len(arguments)+len(JSON(config.Model))) + output + 1024, nil
}

func ReportedTokens(usage *wire.Usage) (int64, bool) {
	if usage == nil {
		return 0, false
	}
	if Value(usage.InputTokens) > 1<<40 || Value(usage.OutputTokens) > 1<<40 {
		return 0, false
	}
	return int64(Value(usage.InputTokens) + Value(usage.OutputTokens)), usage.InputTokens != nil && usage.OutputTokens != nil
}

func AdmitBudget(budget wire.SessionBudget, bytes, tokens int64) error {
	if budget.RemainingCalls == 0 {
		return Fail(wire.ErrorCodeForbidden, "session or ancestor call limit reached")
	}
	if budget.AvailableOperations == 0 || budget.AvailableBytes < uint64(bytes) {
		return Fail(wire.ErrorCodeOverloaded, "session or ancestor concurrency or output reserve is exhausted")
	}
	if budget.AvailableTokens != nil && tokens > 0 {
		remaining, err := strconv.ParseInt(*budget.AvailableTokens, 10, 64)
		if err != nil || remaining < 0 {
			return Fail(wire.ErrorCodeCorrupt, "token budget availability is invalid")
		}
		if remaining < tokens {
			if budget.TokenUsagePending {
				return Fail(wire.ErrorCodeOverloaded, "token budget is held by in-flight or incomplete usage")
			}
			return Fail(wire.ErrorCodeForbidden, "model request does not fit the remaining token budget")
		}
	}
	return nil
}
