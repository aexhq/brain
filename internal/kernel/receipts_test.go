package kernel

import (
	"encoding/json"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/aexhq/brain/internal/generated/wire"
)

func TestReceiptMetadataPreservesOutcomeCountersAndBoundedOriginalEvidence(t *testing.T) {
	provider := JSON(map[string]json.RawMessage{"details": JSON(strings.Repeat("界", 400000))})
	original := wire.Receipt{
		RequestID: "original", Outcome: wire.OutcomeCompleted, Result: JSON("completed"),
		Usage: &wire.Usage{InputTokens: Ptr(uint64(3)), Provider: provider},
	}
	value, unavailable, err := BoundedReceipt(original, 8192)
	if err != nil || unavailable || value.Outcome != wire.OutcomeCompleted || string(value.Result) != `"completed"` {
		t.Fatalf("oversized provider details changed known completion: %+v %t %v", value, unavailable, err)
	}
	if value.Usage.InputTokens == nil || *value.Usage.InputTokens != 3 || value.Usage.OutputTokens != nil ||
		string(value.Usage.Provider) != "{}" || value.Usage.ProviderUnavailable == nil {
		t.Fatalf("sparse counters or metadata availability changed: %+v", value.Usage)
	}
	body := JSON(provider)
	if value.Usage.ProviderUnavailable.Digest != Digest(body) || value.Usage.ProviderUnavailable.Bytes != uint64(len(body)) {
		t.Fatal("missing metadata lost its exact digest and size")
	}
	if string(original.Usage.Provider) != string(provider) || original.Usage.ProviderUnavailable != nil {
		t.Fatal("normalization mutated the caller's original receipt")
	}
	oversized, missing, err := BoundedReceipt(wire.Receipt{RequestID: "oversized", Outcome: wire.OutcomeCompleted,
		Result: JSON(strings.Repeat("x", 9000)), Error: Ptr(strings.Repeat("界", 2000))}, 8192)
	if err != nil || !missing || oversized.Outcome != wire.OutcomeCompleted || len(oversized.Result) != 0 || oversized.Error == nil {
		t.Fatalf("lost body became a retryable outcome: %+v %v", oversized, err)
	}
}

func TestReceiptFrameFitsItsOutputReserveForWorstCaseEscaping(t *testing.T) {
	for _, limit := range []int64{1, 8192, 65536, 196608} {
		receipt := wire.Receipt{RequestID: "original", Outcome: wire.OutcomeFailed,
			Result: json.RawMessage(`"` + strings.Repeat("x", max(0, int(limit)-2)) + `"`),
			Error:  Ptr(strings.Repeat("\x00界", 2000)),
			Usage:  &wire.Usage{OutputTokens: Ptr(uint64(0)), Provider: JSON(map[string]string{"details": strings.Repeat("x", 32000)})}}
		value, _, err := BoundedReceipt(receipt, limit)
		if err != nil || !utf8.ValidString(*value.Error) {
			t.Fatalf("bounded UTF-8 metadata: %+v %v", value, err)
		}

		record := wire.Record{SessionID: strings.Repeat("s", 128), Sequence: "9223372036854775807", Kind: "operation.failed",
			TimeMs: 9007199254740991, RunID: Ptr(strings.Repeat("r", 128)), OperationID: Ptr(strings.Repeat("o", 128)), Data: JSON(value)}
		frame := len(JSON(record))
		if frame > 256<<10 || int64(frame*3+1024) > OutputReservation(limit) {
			t.Fatalf("frame/reserve mismatch: limit=%d frame=%d reserve=%d", limit, frame, OutputReservation(limit))
		}
	}
}

func TestDiscardedReceiptKeepsSparseMetadataWithoutMutatingOriginalBodies(t *testing.T) {
	provider := JSON(map[string]string{"private": "original provider body"})
	original := wire.Receipt{RequestID: "original", Outcome: wire.OutcomeCompleted, Result: JSON("original body"),
		Usage: &wire.Usage{InputTokens: Ptr(uint64(3)), Provider: provider}}
	value := DiscardReceiptBodies(original)
	if value.Outcome != original.Outcome || value.RequestID != original.RequestID || value.Result != nil ||
		*value.Usage.InputTokens != 3 || value.Usage.OutputTokens != nil || string(value.Usage.Provider) != "{}" {
		t.Fatalf("disposal changed known outcome or sparse counters: %+v", value)
	}
	if value.Usage.ProviderUnavailable == nil || value.Usage.ProviderUnavailable.Digest != Digest(provider) ||
		value.Usage.ProviderUnavailable.Bytes != uint64(len(provider)) {
		t.Fatal("discarded provider payload lost its original digest and size")
	}
	if string(original.Result) != `"original body"` || string(original.Usage.Provider) != string(provider) || original.Usage.ProviderUnavailable != nil {
		t.Fatal("disposal mutated original receipt identity evidence")
	}
	repeated := DiscardReceiptBodies(value)
	if repeated.Usage.ProviderUnavailable.Digest != value.Usage.ProviderUnavailable.Digest {
		t.Fatal("repeated disposal replaced the original unavailable-payload evidence")
	}
	if value := DiscardReceiptBodies(wire.Receipt{Outcome: wire.OutcomeUnknown}); value.Usage != nil {
		t.Fatal("disposal invented usage")
	}
}
