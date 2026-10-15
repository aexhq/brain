package kernel

import (
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
)

func TestTokenAndTimeLimitsRetainExactLanguageNeutralIntegers(t *testing.T) {
	config := wire.SessionConfig{Limits: wire.Limits{Calls: 1, Concurrent: 1, InputBytes: 1024,
		OutputBytes: 1024, RetainedBytes: 16384, PendingInputs: 1, Tokens: Ptr(uint64(1<<53 - 1)), DeadlineMs: Ptr(uint64(1<<53 - 1))}}
	if err := ValidateConfig(config); err != nil {
		t.Fatalf("maximum exact integer was rejected: %v", err)
	}
	for _, field := range []string{"tokens", "deadline"} {
		invalid := config
		if field == "tokens" {
			invalid.Limits.Tokens = Ptr(uint64(1 << 53))
		} else {
			invalid.Limits.DeadlineMs = Ptr(uint64(1 << 53))
		}
		if Response(ValidateConfig(invalid)).Code != wire.ErrorCodeInvalid {
			t.Fatalf("inexact %s limit was admitted", field)
		}
	}
}
