package kernel

import (
	"encoding/json"

	"github.com/aexhq/brain/internal/generated/wire"
)

func UsageCounters(usage *wire.Usage) *wire.UsageCounters {
	if usage == nil {
		return nil
	}
	return &wire.UsageCounters{InputTokens: usage.InputTokens, OutputTokens: usage.OutputTokens, CachedTokens: usage.CachedTokens}
}

func ReceiptUsage(current *wire.Usage, counters *wire.UsageCounters) (*wire.Usage, error) {
	if counters == nil {
		return nil, nil
	}
	if current == nil {
		return nil, Fail(wire.ErrorCodeCorrupt, "original receipt counters have no retained usage metadata")
	}
	original := *current
	original.InputTokens, original.OutputTokens, original.CachedTokens = counters.InputTokens, counters.OutputTokens, counters.CachedTokens
	return &original, nil
}

func RefineUsage(original *wire.Usage, counters wire.UsageCounters) (*wire.Usage, int64, bool, error) {
	next := wire.Usage{Provider: json.RawMessage("{}")}
	if original != nil {
		next = *original
	}
	before, wasComplete := ReportedTokens(original)
	changed := false
	for _, value := range []struct {
		current  **uint64
		supplied *uint64
	}{
		{&next.InputTokens, counters.InputTokens},
		{&next.OutputTokens, counters.OutputTokens},
		{&next.CachedTokens, counters.CachedTokens},
	} {
		if value.supplied == nil {
			continue
		}
		if *value.supplied > 1<<40 {
			return nil, 0, false, Fail(wire.ErrorCodeInvalid, "usage counter exceeds its admitted bound")
		}
		if *value.current != nil {
			if **value.current != *value.supplied {
				return nil, 0, false, Fail(wire.ErrorCodeConflict, "supplemental evidence cannot replace a reported counter")
			}
			continue
		}
		*value.current = Ptr(*value.supplied)
		changed = true
	}
	if !changed {
		return nil, 0, false, Fail(wire.ErrorCodeConflict, "report must fill an absent counter; reconcile the original request_id instead")
	}
	after, complete := ReportedTokens(&next)
	if after < before {
		return nil, 0, false, Fail(wire.ErrorCodeCorrupt, "usage refinement cannot erase reported accounting")
	}
	return &next, after - before, !wasComplete && complete, nil
}
