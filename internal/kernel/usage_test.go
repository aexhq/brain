package kernel

import (
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
)

func TestReceiptUsageKeepsOriginalCountersWhenAccountingIsRefinedBeforeBinding(t *testing.T) {
	original := &wire.Usage{InputTokens: Ptr(uint64(7)), Provider: JSON(map[string]string{"id": "original"})}
	counters := UsageCounters(original)
	refined, _, _, err := RefineUsage(original, wire.UsageCounters{OutputTokens: Ptr(uint64(0)), CachedTokens: Ptr(uint64(2))})
	if err != nil {
		t.Fatal(err)
	}
	receipt, err := ReceiptUsage(refined, counters)
	if err != nil || string(JSON(receipt)) != string(JSON(original)) || refined.OutputTokens == nil || refined.CachedTokens == nil {
		t.Fatalf("original receipt inherited later usage or erased current accounting: %+v %v", receipt, err)
	}
	if absent, err := ReceiptUsage(refined, UsageCounters(nil)); err != nil || absent != nil {
		t.Fatalf("absent original usage was manufactured: %+v %v", absent, err)
	}
	if _, err := ReceiptUsage(nil, counters); Response(err).Code != wire.ErrorCodeCorrupt {
		t.Fatalf("missing original metadata was silently accepted: %v", err)
	}
}

func TestUsageRefinementPreservesAbsenceZerosAndOriginalProviderEvidence(t *testing.T) {
	original := &wire.Usage{InputTokens: Ptr(uint64(0)), Provider: JSON(map[string]string{"id": "original"})}
	usage, difference, complete, err := RefineUsage(original, wire.UsageCounters{CachedTokens: Ptr(uint64(2))})
	if err != nil || difference != 0 || complete || usage.OutputTokens != nil || original.CachedTokens != nil {
		t.Fatalf("cached-only refinement changed absence or its original evidence: %+v %d %v %v", usage, difference, complete, err)
	}
	usage, difference, complete, err = RefineUsage(usage, wire.UsageCounters{InputTokens: Ptr(uint64(0)), OutputTokens: Ptr(uint64(5))})
	if err != nil || difference != 5 || !complete || string(usage.Provider) != string(original.Provider) ||
		*usage.InputTokens != 0 || *usage.OutputTokens != 5 || *usage.CachedTokens != 2 || original.OutputTokens != nil {
		t.Fatalf("sparse completion changed prior values: %+v %d %v %v", usage, difference, complete, err)
	}
	for _, counters := range []wire.UsageCounters{
		{}, {InputTokens: Ptr(uint64(0))}, {OutputTokens: Ptr(uint64(6))}, {CachedTokens: Ptr(uint64(0))},
	} {
		if _, _, _, err := RefineUsage(usage, counters); Response(err).Code != wire.ErrorCodeConflict {
			t.Fatalf("counter erasure, replacement or a no-op produced evidence: %v", err)
		}
	}
	if _, _, _, err := RefineUsage(nil, wire.UsageCounters{InputTokens: Ptr(uint64(1<<40 + 1))}); Response(err).Code != wire.ErrorCodeInvalid {
		t.Fatalf("oversized counter was admitted: %v", err)
	}
	zero, difference, complete, err := RefineUsage(nil, wire.UsageCounters{InputTokens: Ptr(uint64(0)), OutputTokens: Ptr(uint64(0))})
	if err != nil || difference != 0 || !complete || zero.InputTokens == nil || zero.OutputTokens == nil || zero.CachedTokens != nil {
		t.Fatalf("reported zero was confused with absence: %+v %d %v %v", zero, difference, complete, err)
	}
	_, difference, complete, err = RefineUsage(zero, wire.UsageCounters{CachedTokens: Ptr(uint64(0))})
	if err != nil || difference != 0 || complete {
		t.Fatalf("cached refinement completed the same accounting twice: %d %v %v", difference, complete, err)
	}
}
