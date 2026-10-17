package session

import (
	"context"
	"encoding/json"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/persistence"
)

// Repository is the local state consumed by the session owner. Metadata writes
// are durable before returning. Root and Dir identify the local workspace used
// by native harnesses and staged recovery; remote databases do not replace it.
// The composition root owns Close, after Engine.Shutdown has drained executions.
type Repository interface {
	Create(context.Context, core.Session) error
	Get(context.Context, string) (core.Session, error)
	Save(context.Context, core.Session) error
	List(context.Context, string, string, int) ([]core.Session, error)
	Scan(context.Context, string, int) ([]core.Session, error)
	Import(context.Context, core.Session) error
	Root() string
	Dir(string) (string, error)
}

// ToolExecutor applies authorization and dispatch policy outside scheduling.
// Calls honor cancellation, do not retry, and return model-visible failures.
type ToolExecutor interface {
	Bind(context.Context, *core.Session) error
	Describe(context.Context, core.Session) ([]core.Tool, error)
	NeedsInspection(string) bool
	Invoke(context.Context, core.Session, core.ToolScope, core.Call, func(json.RawMessage)) (core.Result, error)
}

// Checkpoints publishes and restores immutable snapshots without inference.
// List is eventual discovery; it must never be used as an ownership lock.
type Checkpoints interface {
	Publish(context.Context, persistence.Snapshot) (persistence.Manifest, error)
	Restore(context.Context, string, string, string, string) (persistence.Manifest, string, error)
	List(context.Context, string, string, int) ([]core.Session, error)
}
