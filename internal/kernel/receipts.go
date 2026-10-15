package kernel

import (
	"encoding/hex"
	"encoding/json"
	"strings"
	"unicode/utf8"

	"github.com/aexhq/brain/internal/generated/wire"
)

const ReceiptMetadataBytes = 48 << 10

func OutputReservation(limit int64) int64 {
	return (limit+ReceiptMetadataBytes)*3 + 4096
}

func BoundedReceipt(receipt wire.Receipt, limit int64) (wire.Receipt, bool, error) {
	if !Identifier(receipt.RequestID) {
		return receipt, false, Fail(wire.ErrorCodeInvalid, "receipt request identifier exceeds its admitted bound")
	}
	if receipt.Error != nil && len(*receipt.Error) > 1024 {
		end := 1024
		for !utf8.ValidString((*receipt.Error)[:end]) {
			end--
		}
		receipt.Error = Ptr((*receipt.Error)[:end] + " [truncated]")
	}
	if receipt.Usage != nil {
		usage := *receipt.Usage
		receipt.Usage = &usage
		if usage.Provider == nil {
			receipt.Usage.Provider = json.RawMessage("{}")
		}
		provider := JSON(receipt.Usage.Provider)
		if len(provider) > 32<<10 {
			receipt.Usage.Provider = json.RawMessage("{}")
			receipt.Usage.ProviderUnavailable = &wire.UnavailablePayload{Digest: Digest(provider), Bytes: uint64(len(provider))}
		}
		if value := receipt.Usage.ProviderUnavailable; value != nil {
			_, err := hex.DecodeString(value.Digest)
			if err != nil || len(value.Digest) != 64 || strings.ToLower(value.Digest) != value.Digest || value.Bytes > 1<<53-1 {
				return receipt, false, Fail(wire.ErrorCodeInvalid, "unavailable usage evidence exceeds its admitted bound")
			}
		}
	}
	metadata := receipt
	metadata.Result = nil
	if len(JSON(metadata)) > ReceiptMetadataBytes {
		return receipt, false, Fail(wire.ErrorCodeInvalid, "receipt metadata exceeds its admitted bound")
	}
	unavailable := int64(len(receipt.Result)) > limit
	if unavailable {
		receipt.Result = nil
		receipt.Error = Ptr("result body exceeds the admitted durable receipt limit")
	}
	return receipt, unavailable, nil
}
