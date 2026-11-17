package session

import (
	"context"
	"encoding/json"
	"github.com/aexhq/brain/services/storage/api/credential"

	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/storage/local"
)

// Repository supplies Storage-owned persistence and runtime state operations.
// The composition root closes it after Engine.Shutdown drains executions.
type Repository interface {
	Deleted(context.Context, string) (string, error)
	Delete(context.Context, core.Session) error
	Create(context.Context, core.Session) error
	Get(context.Context, string) (core.Session, error)
	Save(context.Context, core.Session) error
	List(context.Context, string, string, int) ([]core.Session, error)
	Scan(context.Context, string, int) ([]core.Session, error)
	Import(context.Context, core.Session) error
	OpenLog(context.Context, core.Session, int64) (*store.Log, error)
	PrepareNative(context.Context, core.Session, bool) (string, error)
	ValidateNative(context.Context, core.Session, []core.NativeFile) error
	RemoveWorkingCopy(context.Context, string) error
	Snapshot(context.Context, core.Session, int64) (api.Snapshot, error)
	StageRestore(context.Context, api.Checkpoints, string, string) (api.Manifest, string, error)
	InstallRestore(context.Context, core.Session, string) error
	DiscardRestore(string) error
	SyncState(context.Context, core.Session) (*api.Manifest, int64, error)
}

// ToolExecutor applies authorization and dispatch policy outside scheduling.
// Calls honor cancellation, do not retry, and return model-visible failures.
type ToolExecutor interface {
	PrepareCredentials(context.Context, core.Session, []core.CredentialInput) ([]credential.Record, error)
	ValidateCredentials(context.Context, core.Session) error
	Bind(context.Context, *core.Session) error
	Describe(context.Context, core.Session) ([]core.Tool, error)
	NeedsInspection(string) bool
	Invoke(context.Context, core.Session, core.ToolScope, core.Call, func(json.RawMessage)) (core.Result, error)
}

// Checkpoints publishes and restores immutable snapshots without inference.
// List is eventual discovery; it must never be used as an ownership lock.
type Checkpoints = api.Checkpoints
