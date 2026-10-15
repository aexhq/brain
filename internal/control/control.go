// Package control defines finite atomic domain operations, not a remote SQL or
// arbitrary transaction interface. An ambiguous commit never returns authority.
package control

import (
	"context"
	"encoding/json"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

type Owner struct {
	Node  string
	Epoch int64
}

type Session struct {
	View       wire.SessionView
	Tenant     string
	Owner      Owner
	Generation int64
	Budget     string
}

type Run struct {
	View           wire.Run
	Input          wire.InputData
	Checkpoint     json.RawMessage
	Decision       *wire.Decision
	InputSequence  int64
	HarnessProcess string
}

type Transfer struct {
	View         wire.Transfer
	Cursor       string
	TailVerified bool
	Drained      bool
	Archived     bool
	TargetLive   bool
}

type Operation struct {
	View           wire.Operation
	Arguments      json.RawMessage
	OutputLimit    int64
	Deadline       int64
	ResultSequence int64
	ResultDigest   string
	Generation     string
	Claimant       string
	IntentSequence int64
	IntentDigest   string
	TokenReserve   int64
}

type Allocation struct {
	Sequence     int64
	Status       string
	Digest       string
	Bytes        int64
	Node         string
	Endpoint     string
	CurrentOwner string
}

type Coverage struct {
	Session    string
	First      int64
	Last       int64
	Key        string
	Digest     string
	Bytes      int64
	Generation int64
	Records    []Allocation
}

type Participant struct {
	ID          string
	Session     string
	Tenant      string
	Subject     string
	Permissions []string
	Expires     int64
	Revoked     bool
}

type ChildState struct {
	Pending   int64
	Failed    int64
	Completed int64
}

type ChildOrigin struct {
	Owner Owner
	Run   string
	Step  uint64
	Index uint64
}

func After(values []string) string {
	if len(values) == 0 {
		return ""
	}
	return values[0]
}

type Prepare struct {
	ID             string
	Session        string
	Run            string
	StepKey        string
	Kind           wire.EffectKind
	Action         string
	Arguments      json.RawMessage
	Recipient      string
	OutputLimit    int64
	Timeout        int64
	Generation     string
	Approval       bool
	RetryOf        string
	IntentSequence int64
	IntentDigest   string
	TokenReserve   int64
}

type ApplyInput struct {
	Command     string
	Owner       Owner
	Sequence    int64
	Run         bool
	Disposition wire.Disposition
	StateKey    string
}

type Store interface {
	Close() error
	Clock(context.Context) (int64, error)
	RegisterNode(context.Context, string, int64) error
	ConfigureNode(context.Context, string, string, string, []string) error
	RenewNode(context.Context, string, int64) error
	RetireNode(context.Context, string) error
	Create(context.Context, string, wire.SessionCreate, string, string, []string) (Session, error)
	Delegate(context.Context, string, Owner, string, uint64, uint64, wire.SessionCreate, []string) (Session, error)
	Session(context.Context, string, string) (Session, error)
	List(context.Context, string, string, int) ([]Session, error)
	ClaimOwner(context.Context, string, string) (Owner, error)
	Nodes(context.Context, string, int) ([]wire.NodeInfo, error)
	BeginTransfer(context.Context, string, wire.TransferCreate) (Transfer, error)
	Transfer(context.Context, string, string) (Transfer, error)
	PendingTransfers(context.Context, string, string, int) ([]Transfer, error)
	SealTransfer(context.Context, string, Owner, string, wire.Record) error
	TransferRuns(context.Context, string, string, string, int) ([]Run, error)
	VerifyTransferTail(context.Context, string, string, string, wire.RecordReference) error
	VerifyTransferRuns(context.Context, string, string, string, string, []string) (Transfer, error)
	CancelTransfer(context.Context, string, string) (Transfer, error)
	FailTransfer(context.Context, string, string, string, wire.ErrorResponse) error
	Accept(context.Context, kernel.Principal, string, wire.Input) (wire.CommandReceipt, error)
	Command(context.Context, string, string) (wire.CommandReceipt, error)
	Pending(context.Context, string, int) ([]string, error)
	FinishStep(context.Context, string, Owner) error
	NextInput(context.Context, string, Owner) (wire.Input, string, string, error)
	ApplyInput(context.Context, string, ApplyInput) error
	Allocate(context.Context, string, Owner) (int64, error)
	Written(context.Context, string, Owner, int64, string, int64) error
	SealUnused(context.Context, string, Owner, int64) error
	Allocations(context.Context, string, int64, int) ([]Allocation, int64, error)
	LocateRecord(context.Context, string, int64) (Allocation, error)
	Runs(context.Context, string, int, ...string) ([]Run, error)
	RunnableRuns(context.Context, string, int) ([]Run, error)
	ChildState(context.Context, string) (ChildState, error)
	Children(context.Context, string, string, uint64) (wire.ChildResults, error)
	Budget(context.Context, string) (wire.SessionBudget, error)
	BeginDecision(context.Context, string, Owner, string, uint64, string, string, bool) error
	SaveDecision(context.Context, string, Owner, string, uint64, wire.Decision, ...string) error
	CompleteRun(context.Context, string, Owner, string, uint64, wire.Record) error
	AdvanceRun(context.Context, string, Owner, string, uint64, wire.RunStatus, *wire.Wait, *wire.ErrorResponse) error
	Prepare(context.Context, Owner, Prepare) (Operation, error)
	Operation(context.Context, string, string) (Operation, error)
	Operations(context.Context, string, int, ...string) ([]Operation, error)
	PendingResults(context.Context, string, int) ([]Operation, error)
	ClaimDispatch(context.Context, string, Owner, string, string, string) (Operation, error)
	RecordReceipt(context.Context, string, string, wire.Receipt, int64, string) (Operation, error)
	BindResult(context.Context, string, Owner, string, int64, string) error
	ExpireOperations(context.Context, int) (int, error)
	PropagateControls(context.Context, int) ([]string, error)
	Interact(context.Context, string, wire.InteractionCreate) (wire.Interaction, error)
	Interactions(context.Context, string, int, ...string) ([]wire.Interaction, error)
	Interaction(context.Context, string, string) (wire.Interaction, error)
	Answer(context.Context, string, string, wire.InteractionAnswer) (wire.Interaction, error)
	Attach(context.Context, string, wire.ConnectionAttach) (wire.Connection, error)
	Connections(context.Context, string, int, ...string) ([]wire.Connection, error)
	ActiveConnection(context.Context, string, string) (wire.Connection, error)
	RevokeConnection(context.Context, string, string) (wire.Connection, error)
	Documents(context.Context, string, string, int, ...string) ([]wire.Document, error)
	PutDocument(context.Context, string, string, string, wire.DocumentUpdate) (wire.Document, error)
	DeleteDocument(context.Context, string, string, string) (wire.Document, error)
	PutFile(context.Context, string, wire.FileInfo) error
	Files(context.Context, string, int, ...string) ([]wire.FileInfo, error)
	File(context.Context, string, string) (wire.FileInfo, error)
	DeleteFile(context.Context, string, string) (wire.FileInfo, error)
	Schedule(context.Context, string, wire.ScheduleCreate) (wire.Schedule, error)
	Schedules(context.Context, string, int, ...string) ([]wire.Schedule, error)
	CancelSchedule(context.Context, string, string) (wire.Schedule, error)
	FireTimers(context.Context, int) (int, error)
	Grant(context.Context, string, wire.ParticipantGrant) (Participant, error)
	Participant(context.Context, string) (Participant, error)
	RevokeParticipant(context.Context, string, string) error
	Handoff(context.Context, string, wire.Handoff) (Session, error)
	Delete(context.Context, string) (Session, error)
	PublishCoverage(context.Context, Owner, Coverage) error
	Coverage(context.Context, string, int64, int) ([]Coverage, error)
	Report(context.Context) (wire.StorageReport, error)
	PutPackage(context.Context, string, wire.Package) error
	Package(context.Context, string, string) (wire.Package, error)
}
