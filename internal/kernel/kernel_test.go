package kernel

import (
	"errors"
	"testing"

	"github.com/aexhq/brain/internal/generated/wire"
)

func TestSuccessfulChecksAreNotInternalFailures(t *testing.T) {
	if Response(nil).Code != "" {
		t.Fatal("a successful operation was described as an internal failure")
	}

	if Response(errors.New("private detail")).Code != wire.ErrorCodeInternal {
		t.Fatal("unknown failure was not classified")
	}
}
