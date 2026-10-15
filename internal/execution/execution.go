package execution

import (
	"context"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

type Identity interface {
	Authenticate(context.Context, string) (kernel.Principal, error)
}

type Harness interface {
	Validate(wire.SessionConfig) error
	Capabilities() []string
	Resume(context.Context, wire.SessionConfig, string, string) error
	Decide(context.Context, wire.HarnessRequest, string, func(string, bool) error) (wire.Decision, string, error)
	Close() error
}

type Effects interface {
	Model(context.Context, string, wire.ModelSelection, wire.ModelRequest, int64) (wire.Receipt, error)
	Action(context.Context, string, wire.ConnectionTemplate, wire.Execution) (wire.Receipt, error)
	Validate(context.Context, string, wire.SessionConfig) error
	Catalog() []wire.CatalogEntry
	Capabilities() []string
	Requirements(string, wire.SessionConfig) []string
}
