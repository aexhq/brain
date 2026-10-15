// Code generated from brain-protocol Rust types. DO NOT EDIT.
package wire

import (
	_ "embed"
	"encoding/json"
)

//go:embed schema.json
var Schema []byte

//go:embed catalog.json
var Catalog []byte

type Action struct {
	Approval       bool            `json:"approval,omitempty"`
	Connection     string          `json:"connection"`
	Description    string          `json:"description"`
	InputSchema    json.RawMessage `json:"input_schema"`
	MaxOutputBytes uint64          `json:"max_output_bytes"`
	Name           string          `json:"name"`
	OutputSchema   json.RawMessage `json:"output_schema,omitempty"`
	Package        *string         `json:"package,omitempty"`
	TimeoutMs      uint64          `json:"timeout_ms"`
}

type CatalogEntry struct {
	Capabilities        []string        `json:"capabilities"`
	ConfigurationSchema json.RawMessage `json:"configuration_schema"`
	Credentials         []string        `json:"credentials"`
	Kind                string          `json:"kind"`
	Limitations         []string        `json:"limitations"`
	Name                string          `json:"name"`
	Version             string          `json:"version"`
}

type ChildResult struct {
	Availability ChildResultAvailability `json:"availability"`
	CommandID    string                  `json:"command_id"`
	Completion   *RecordReference        `json:"completion,omitempty"`
	Error        *ErrorResponse          `json:"error,omitempty"`
	Index        uint64                  `json:"index"`
	Result       json.RawMessage         `json:"result,omitempty"`
	RunID        string                  `json:"run_id"`
	SessionID    string                  `json:"session_id"`
	Status       RunStatus               `json:"status"`
}

type ChildResultAvailability string

const (
	ChildResultAvailabilityPending     ChildResultAvailability = "pending"
	ChildResultAvailabilityInline      ChildResultAvailability = "inline"
	ChildResultAvailabilityReferenced  ChildResultAvailability = "referenced"
	ChildResultAvailabilityUnavailable ChildResultAvailability = "unavailable"
	ChildResultAvailabilityNone        ChildResultAvailability = "none"
)

type ChildResults struct {
	Children  []ChildResult `json:"children"`
	Expected  uint64        `json:"expected"`
	Ready     bool          `json:"ready"`
	RunID     string        `json:"run_id"`
	SessionID string        `json:"session_id"`
	Step      uint64        `json:"step"`
}

type Claim struct {
	ConnectionID string `json:"connection_id"`
	Generation   string `json:"generation"`
	RequestID    string `json:"request_id"`
}

type CommandReceipt struct {
	AcceptedMs uint64         `json:"accepted_ms"`
	Error      *ErrorResponse `json:"error,omitempty"`
	ID         string         `json:"id"`
	RunID      *string        `json:"run_id,omitempty"`
	Sequence   *string        `json:"sequence,omitempty"`
	SessionID  string         `json:"session_id"`
	Status     string         `json:"status"`
}

type Connection struct {
	Generation string    `json:"generation"`
	ID         string    `json:"id"`
	Ownership  Ownership `json:"ownership"`
	ResourceID *string   `json:"resource_id,omitempty"`
	SessionID  string    `json:"session_id"`
	Status     string    `json:"status"`
	Template   string    `json:"template"`
}

type ConnectionAttach struct {
	RequestID  string  `json:"request_id"`
	ResourceID *string `json:"resource_id,omitempty"`
	Template   string  `json:"template"`
}

type ConnectionKind string

const (
	ConnectionKindApplication ConnectionKind = "application"
	ConnectionKindHttp        ConnectionKind = "http"
	ConnectionKindLocal       ConnectionKind = "local"
	ConnectionKindModal       ConnectionKind = "modal"
	ConnectionKindFly         ConnectionKind = "fly"
	ConnectionKindMcp         ConnectionKind = "mcp"
	ConnectionKindStorage     ConnectionKind = "storage"
)

type ConnectionTemplate struct {
	Capabilities  []string          `json:"capabilities,omitempty"`
	Configuration json.RawMessage   `json:"configuration,omitempty"`
	Kind          ConnectionKind    `json:"kind"`
	Name          string            `json:"name"`
	Ownership     Ownership         `json:"ownership"`
	Provider      string            `json:"provider"`
	Secrets       map[string]string `json:"secrets,omitempty"`
}

type ControlKind string

const (
	ControlKindPause  ControlKind = "pause"
	ControlKindResume ControlKind = "resume"
	ControlKindCancel ControlKind = "cancel"
	ControlKindClose  ControlKind = "close"
)

type DecisionCall struct {
	Action     string          `json:"action"`
	Arguments  json.RawMessage `json:"arguments"`
	Checkpoint json.RawMessage `json:"checkpoint"`
	Effect     EffectKind      `json:"effect"`
	Kind       string          `json:"kind"`
}

type DecisionComplete struct {
	Checkpoint json.RawMessage `json:"checkpoint"`
	Kind       string          `json:"kind"`
	Result     json.RawMessage `json:"result"`
}

type DecisionWait struct {
	Checkpoint json.RawMessage `json:"checkpoint"`
	Kind       string          `json:"kind"`
	Wait       Wait            `json:"wait"`
}

type DecisionEmit struct {
	Checkpoint json.RawMessage `json:"checkpoint"`
	Data       json.RawMessage `json:"data"`
	Kind       string          `json:"kind"`
	Name       string          `json:"name"`
	Version    string          `json:"version"`
}

type DecisionInteract struct {
	Checkpoint  json.RawMessage   `json:"checkpoint"`
	Interaction InteractionCreate `json:"interaction"`
	Kind        string            `json:"kind"`
}

type DecisionDelegate struct {
	Checkpoint json.RawMessage `json:"checkpoint"`
	Delegation Delegation      `json:"delegation"`
	Kind       string          `json:"kind"`
}

type DecisionFork struct {
	Checkpoint  json.RawMessage `json:"checkpoint"`
	Delegations []Delegation    `json:"delegations"`
	Kind        string          `json:"kind"`
}

type Decision struct {
	Action      *string            `json:"action,omitempty"`
	Arguments   json.RawMessage    `json:"arguments,omitempty"`
	Checkpoint  json.RawMessage    `json:"checkpoint"`
	Effect      *EffectKind        `json:"effect,omitempty"`
	Kind        string             `json:"kind"`
	Result      json.RawMessage    `json:"result,omitempty"`
	Wait        *Wait              `json:"wait,omitempty"`
	Data        json.RawMessage    `json:"data,omitempty"`
	Name        *string            `json:"name,omitempty"`
	Version     *string            `json:"version,omitempty"`
	Interaction *InteractionCreate `json:"interaction,omitempty"`
	Delegation  *Delegation        `json:"delegation,omitempty"`
	Delegations []Delegation       `json:"delegations,omitempty"`
}

type Delegation struct {
	Config    SessionConfig `json:"config"`
	Context   []string      `json:"context"`
	Message   string        `json:"message"`
	RequestID string        `json:"request_id"`
}

type Disposition string

const (
	DispositionApplication Disposition = "application"
	DispositionState       Disposition = "state"
	DispositionContext     Disposition = "context"
	DispositionWake        Disposition = "wake"
	DispositionSteer       Disposition = "steer"
	DispositionInterrupt   Disposition = "interrupt"
)

type Document struct {
	ExpiresMs *uint64         `json:"expires_ms,omitempty"`
	Key       string          `json:"key"`
	Revision  string          `json:"revision"`
	Value     json.RawMessage `json:"value"`
}

type DocumentUpdate struct {
	ExpiresMs *uint64         `json:"expires_ms,omitempty"`
	RequestID string          `json:"request_id"`
	Revision  string          `json:"revision"`
	Value     json.RawMessage `json:"value"`
}

type EffectKind string

const (
	EffectKindModel     EffectKind = "model"
	EffectKindAction    EffectKind = "action"
	EffectKindBuild     EffectKind = "build"
	EffectKindLifecycle EffectKind = "lifecycle"
	EffectKindWebhook   EffectKind = "webhook"
)

type ErrorCode string

const (
	ErrorCodeInvalid      ErrorCode = "invalid"
	ErrorCodeUnauthorized ErrorCode = "unauthorized"
	ErrorCodeForbidden    ErrorCode = "forbidden"
	ErrorCodeNotFound     ErrorCode = "not_found"
	ErrorCodeConflict     ErrorCode = "conflict"
	ErrorCodeOverloaded   ErrorCode = "overloaded"
	ErrorCodeUnsupported  ErrorCode = "unsupported"
	ErrorCodeUnavailable  ErrorCode = "unavailable"
	ErrorCodeUncertain    ErrorCode = "uncertain"
	ErrorCodeCorrupt      ErrorCode = "corrupt"
	ErrorCodeInternal     ErrorCode = "internal"
)

type ErrorResponse struct {
	Code      ErrorCode `json:"code"`
	Message   string    `json:"message"`
	RequestID *string   `json:"request_id,omitempty"`
	Retryable bool      `json:"retryable"`
}

type EventRoute struct {
	Action      *string         `json:"action,omitempty"`
	Disposition Disposition     `json:"disposition"`
	Name        string          `json:"name"`
	Schema      json.RawMessage `json:"schema"`
	Sources     []string        `json:"sources,omitempty"`
	Version     string          `json:"version"`
}

type Execution struct {
	Arguments      json.RawMessage `json:"arguments"`
	DeadlineMs     uint64          `json:"deadline_ms"`
	MaxOutputBytes uint64          `json:"max_output_bytes"`
	Operation      Operation       `json:"operation"`
	ReceiptToken   string          `json:"receipt_token"`
}

type ExportManifest struct {
	Complete     bool          `json:"complete"`
	Connections  []Connection  `json:"connections"`
	Continuation string        `json:"continuation"`
	Files        []FileInfo    `json:"files"`
	Format       string        `json:"format"`
	History      RecordPage    `json:"history"`
	Interactions []Interaction `json:"interactions"`
	Memory       []Document    `json:"memory"`
	Operations   []Operation   `json:"operations"`
	Runs         []Run         `json:"runs"`
	Schedules    []Schedule    `json:"schedules"`
	Section      ExportSection `json:"section"`
	Session      SessionView   `json:"session"`
	State        []Document    `json:"state"`
}

type ExportSection string

const (
	ExportSectionHistory      ExportSection = "history"
	ExportSectionRuns         ExportSection = "runs"
	ExportSectionOperations   ExportSection = "operations"
	ExportSectionInteractions ExportSection = "interactions"
	ExportSectionConnections  ExportSection = "connections"
	ExportSectionFiles        ExportSection = "files"
	ExportSectionState        ExportSection = "state"
	ExportSectionMemory       ExportSection = "memory"
	ExportSectionSchedules    ExportSection = "schedules"
)

type FileInfo struct {
	Digest     string `json:"digest"`
	MediaType  string `json:"media_type"`
	Name       string `json:"name"`
	Provenance string `json:"provenance"`
	Size       uint64 `json:"size"`
}

type Gap struct {
	First string  `json:"first"`
	Kind  GapKind `json:"kind"`
	Last  string  `json:"last"`
}

type GapKind string

const (
	GapKindReserved    GapKind = "reserved"
	GapKindUnused      GapKind = "unused"
	GapKindUnavailable GapKind = "unavailable"
	GapKindUnresolved  GapKind = "unresolved"
)

type Handoff struct {
	Harness   HarnessSelection `json:"harness"`
	RequestID string           `json:"request_id"`
	Revision  string           `json:"revision"`
}

type HarnessRequest struct {
	Checkpoint json.RawMessage `json:"checkpoint"`
	Children   []ChildResult   `json:"children,omitempty"`
	CommandID  string          `json:"command_id"`
	Config     SessionConfig   `json:"config"`
	Context    []Record        `json:"context"`
	Input      InputData       `json:"input"`
	Outcome    *Operation      `json:"outcome,omitempty"`
	Protocol   string          `json:"protocol"`
	RunID      string          `json:"run_id"`
	SessionID  string          `json:"session_id"`
	Step       uint64          `json:"step"`
}

type HarnessSelection struct {
	Configuration json.RawMessage `json:"configuration,omitempty"`
	Name          string          `json:"name"`
	Version       string          `json:"version"`
}

type Input struct {
	Data      InputData `json:"data"`
	RequestID string    `json:"request_id"`
}

type InputDataMessage struct {
	Kind string `json:"kind"`
	Text string `json:"text"`
}

type InputDataEvent struct {
	Correlation *string         `json:"correlation,omitempty"`
	Data        json.RawMessage `json:"data"`
	Kind        string          `json:"kind"`
	Name        string          `json:"name"`
	Version     string          `json:"version"`
}

type InputDataAction struct {
	Arguments json.RawMessage `json:"arguments"`
	Kind      string          `json:"kind"`
	Name      string          `json:"name"`
	RetryOf   *string         `json:"retry_of,omitempty"`
}

type InputDataControl struct {
	Control ControlKind `json:"control"`
	Kind    string      `json:"kind"`
}

type InputData struct {
	Kind        string          `json:"kind"`
	Text        *string         `json:"text,omitempty"`
	Correlation *string         `json:"correlation,omitempty"`
	Data        json.RawMessage `json:"data,omitempty"`
	Name        *string         `json:"name,omitempty"`
	Version     *string         `json:"version,omitempty"`
	Arguments   json.RawMessage `json:"arguments,omitempty"`
	RetryOf     *string         `json:"retry_of,omitempty"`
	Control     *ControlKind    `json:"control,omitempty"`
}

type Interaction struct {
	Answer    json.RawMessage   `json:"answer,omitempty"`
	ID        string            `json:"id"`
	Request   InteractionCreate `json:"request"`
	Revision  string            `json:"revision"`
	SessionID string            `json:"session_id"`
	Status    InteractionStatus `json:"status"`
}

type InteractionAnswer struct {
	Accept        bool            `json:"accept"`
	Answer        json.RawMessage `json:"answer"`
	RequestID     string          `json:"request_id"`
	Revision      string          `json:"revision"`
	SubjectDigest string          `json:"subject_digest"`
}

type InteractionCreate struct {
	ExpiresMs     uint64          `json:"expires_ms"`
	Kind          InteractionKind `json:"kind"`
	Prompt        string          `json:"prompt"`
	RequestID     string          `json:"request_id"`
	RunID         string          `json:"run_id"`
	Schema        json.RawMessage `json:"schema"`
	Subject       string          `json:"subject"`
	SubjectDigest string          `json:"subject_digest"`
}

type InteractionKind string

const (
	InteractionKindQuestion InteractionKind = "question"
	InteractionKindForm     InteractionKind = "form"
	InteractionKindApproval InteractionKind = "approval"
)

type InteractionStatus string

const (
	InteractionStatusPending   InteractionStatus = "pending"
	InteractionStatusAnswered  InteractionStatus = "answered"
	InteractionStatusRejected  InteractionStatus = "rejected"
	InteractionStatusExpired   InteractionStatus = "expired"
	InteractionStatusCancelled InteractionStatus = "cancelled"
)

type Lifecycle string

const (
	LifecycleOpen    Lifecycle = "open"
	LifecycleClosing Lifecycle = "closing"
	LifecycleClosed  Lifecycle = "closed"
	LifecycleDeleted Lifecycle = "deleted"
)

type Limits struct {
	Calls         uint64  `json:"calls"`
	Concurrent    uint64  `json:"concurrent"`
	DeadlineMs    *uint64 `json:"deadline_ms,omitempty"`
	InputBytes    uint64  `json:"input_bytes"`
	OutputBytes   uint64  `json:"output_bytes"`
	PendingInputs uint64  `json:"pending_inputs"`
	RetainedBytes uint64  `json:"retained_bytes"`
	Tokens        *uint64 `json:"tokens,omitempty"`
}

type ManagedInvocation struct {
	CallbackUrl string            `json:"callback_url"`
	Execution   Execution         `json:"execution"`
	Package     Package           `json:"package"`
	Secrets     map[string]string `json:"secrets"`
}

type ModelMessage struct {
	Content    json.RawMessage `json:"content"`
	Role       string          `json:"role"`
	ToolCallID *string         `json:"tool_call_id,omitempty"`
	ToolCalls  []ModelToolCall `json:"tool_calls,omitempty"`
}

type ModelRequest struct {
	Actions  []Action       `json:"actions"`
	Messages []ModelMessage `json:"messages"`
}

type ModelResponse struct {
	Message ModelMessage    `json:"message"`
	Native  json.RawMessage `json:"native,omitempty"`
	Usage   Usage           `json:"usage"`
}

type ModelSelection struct {
	Credential *string         `json:"credential,omitempty"`
	Format     json.RawMessage `json:"format,omitempty"`
	Name       string          `json:"name"`
	Native     json.RawMessage `json:"native,omitempty"`
	Provider   string          `json:"provider"`
}

type ModelToolCall struct {
	Arguments json.RawMessage `json:"arguments"`
	ID        string          `json:"id"`
	Name      string          `json:"name"`
}

type NodeInfo struct {
	Draining       bool   `json:"draining"`
	Endpoint       string `json:"endpoint"`
	ID             string `json:"id"`
	LeaseExpiresMs uint64 `json:"lease_expires_ms"`
}

type Operation struct {
	Action          string          `json:"action"`
	ArgumentsDigest string          `json:"arguments_digest"`
	Error           *string         `json:"error,omitempty"`
	ID              string          `json:"id"`
	Kind            EffectKind      `json:"kind"`
	Recipient       string          `json:"recipient"`
	Result          json.RawMessage `json:"result,omitempty"`
	ResultAvailable bool            `json:"result_available"`
	RetryOf         *string         `json:"retry_of,omitempty"`
	Revision        string          `json:"revision"`
	RunID           string          `json:"run_id"`
	SessionID       string          `json:"session_id"`
	Status          OperationStatus `json:"status"`
	StepKey         string          `json:"step_key"`
	Usage           *Usage          `json:"usage,omitempty"`
}

type OperationStatus string

const (
	OperationStatusPrepared   OperationStatus = "prepared"
	OperationStatusDispatched OperationStatus = "dispatched"
	OperationStatusCompleted  OperationStatus = "completed"
	OperationStatusFailed     OperationStatus = "failed"
	OperationStatusUnknown    OperationStatus = "unknown"
	OperationStatusCancelled  OperationStatus = "cancelled"
)

type Outcome string

const (
	OutcomeCompleted Outcome = "completed"
	OutcomeFailed    Outcome = "failed"
	OutcomeUnknown   Outcome = "unknown"
)

type Ownership string

const (
	OwnershipManaged  Ownership = "managed"
	OwnershipBorrowed Ownership = "borrowed"
)

type Package struct {
	Digest   string          `json:"digest"`
	Error    *string         `json:"error,omitempty"`
	ID       string          `json:"id"`
	Image    *string         `json:"image,omitempty"`
	Manifest PackageManifest `json:"manifest"`
	Status   string          `json:"status"`
}

type PackageAdmission struct {
	Builder   string          `json:"builder"`
	Manifest  PackageManifest `json:"manifest"`
	RequestID string          `json:"request_id"`
	SessionID string          `json:"session_id"`
}

type PackageBuildResult struct {
	BaseImage    string `json:"base_image"`
	Builder      string `json:"builder"`
	Image        string `json:"image"`
	PackageID    string `json:"package_id"`
	SourceDigest string `json:"source_digest"`
}

type PackageManifest struct {
	Actions     []Action          `json:"actions"`
	CpuMillis   uint64            `json:"cpu_millis"`
	Entrypoint  string            `json:"entrypoint"`
	Files       map[string]string `json:"files"`
	Language    RuntimeLanguage   `json:"language"`
	Lockfile    string            `json:"lockfile"`
	MemoryBytes uint64            `json:"memory_bytes"`
	Name        string            `json:"name"`
	Network     []string          `json:"network"`
	Secrets     []string          `json:"secrets"`
	Version     string            `json:"version"`
	WallMs      uint64            `json:"wall_ms"`
}

type ParticipantGrant struct {
	ExpiresMs   uint64   `json:"expires_ms"`
	Permissions []string `json:"permissions"`
	RequestID   string   `json:"request_id"`
	Subject     string   `json:"subject"`
}

type ParticipantToken struct {
	ExpiresMs uint64 `json:"expires_ms"`
	ID        string `json:"id"`
	Token     string `json:"token"`
}

type Receipt struct {
	Error     *string         `json:"error,omitempty"`
	Outcome   Outcome         `json:"outcome"`
	RequestID string          `json:"request_id"`
	Result    json.RawMessage `json:"result,omitempty"`
	Usage     *Usage          `json:"usage,omitempty"`
}

type Record struct {
	Data        json.RawMessage `json:"data"`
	Kind        string          `json:"kind"`
	OperationID *string         `json:"operation_id,omitempty"`
	RunID       *string         `json:"run_id,omitempty"`
	Sequence    string          `json:"sequence"`
	SessionID   string          `json:"session_id"`
	TimeMs      uint64          `json:"time_ms"`
}

type RecordPage struct {
	AllocatedThrough string   `json:"allocated_through"`
	Gaps             []Gap    `json:"gaps"`
	More             bool     `json:"more"`
	Next             string   `json:"next"`
	Records          []Record `json:"records"`
}

type RecordReference struct {
	Digest    string `json:"digest"`
	Sequence  string `json:"sequence"`
	SessionID string `json:"session_id"`
}

type Retry struct {
	AcknowledgeUncertainty bool   `json:"acknowledge_uncertainty"`
	RequestID              string `json:"request_id"`
}

type Run struct {
	CommandID  string           `json:"command_id"`
	Completion *RecordReference `json:"completion,omitempty"`
	Error      *ErrorResponse   `json:"error,omitempty"`
	ID         string           `json:"id"`
	SessionID  string           `json:"session_id"`
	Status     RunStatus        `json:"status"`
	Step       uint64           `json:"step"`
	Wait       *Wait            `json:"wait,omitempty"`
}

type RunStatus string

const (
	RunStatusQueued    RunStatus = "queued"
	RunStatusActive    RunStatus = "active"
	RunStatusWaiting   RunStatus = "waiting"
	RunStatusPaused    RunStatus = "paused"
	RunStatusCompleted RunStatus = "completed"
	RunStatusFailed    RunStatus = "failed"
	RunStatusCancelled RunStatus = "cancelled"
	RunStatusBlocked   RunStatus = "blocked"
)

type RuntimeLanguage string

const (
	RuntimeLanguageNode   RuntimeLanguage = "node"
	RuntimeLanguagePython RuntimeLanguage = "python"
)

type Schedule struct {
	DueMs     uint64         `json:"due_ms"`
	ID        string         `json:"id"`
	Request   ScheduleCreate `json:"request"`
	SessionID string         `json:"session_id"`
	Status    string         `json:"status"`
}

type ScheduleCreate struct {
	DueMs      uint64    `json:"due_ms"`
	Input      InputData `json:"input"`
	IntervalMs *uint64   `json:"interval_ms,omitempty"`
	RequestID  string    `json:"request_id"`
}

type SessionBudget struct {
	ActiveOperations    uint64  `json:"active_operations"`
	AvailableBytes      uint64  `json:"available_bytes"`
	AvailableOperations uint64  `json:"available_operations"`
	AvailableTokens     *string `json:"available_tokens,omitempty"`
	Calls               uint64  `json:"calls"`
	IncompleteUsages    uint64  `json:"incomplete_usages"`
	RemainingCalls      uint64  `json:"remaining_calls"`
	ReportedTokens      string  `json:"reported_tokens"`
	ReservedOutputBytes uint64  `json:"reserved_output_bytes"`
	ReservedTokens      string  `json:"reserved_tokens"`
	RetainedBytes       uint64  `json:"retained_bytes"`
	SessionID           string  `json:"session_id"`
	TokenUsagePending   bool    `json:"token_usage_pending"`
}

type SessionConfig struct {
	Actions            []Action             `json:"actions,omitempty"`
	Connections        []ConnectionTemplate `json:"connections,omitempty"`
	Events             []EventRoute         `json:"events,omitempty"`
	Grants             []string             `json:"grants,omitempty"`
	Harness            HarnessSelection     `json:"harness"`
	HarnessTransitions []HarnessSelection   `json:"harness_transitions,omitempty"`
	Instructions       string               `json:"instructions,omitempty"`
	Limits             Limits               `json:"limits"`
	Model              ModelSelection       `json:"model"`
	Skills             []string             `json:"skills,omitempty"`
}

type SessionCreate struct {
	Config    SessionConfig `json:"config"`
	Message   *string       `json:"message,omitempty"`
	RequestID string        `json:"request_id"`
}

type SessionPage struct {
	Next     *string       `json:"next,omitempty"`
	Sessions []SessionView `json:"sessions"`
}

type SessionView struct {
	Config     SessionConfig `json:"config"`
	CreatedMs  uint64        `json:"created_ms"`
	ID         string        `json:"id"`
	Lifecycle  Lifecycle     `json:"lifecycle"`
	OwnerEpoch string        `json:"owner_epoch"`
	Parent     *string       `json:"parent,omitempty"`
	Paused     bool          `json:"paused"`
	Revision   string        `json:"revision"`
	Sequence   string        `json:"sequence"`
}

type StorageReport struct {
	Accepting           bool   `json:"accepting"`
	ArchiveQueueBytes   uint64 `json:"archive_queue_bytes"`
	CacheBytes          uint64 `json:"cache_bytes"`
	Format              string `json:"format"`
	OldestUnarchivedMs  uint64 `json:"oldest_unarchived_ms"`
	ReservedOutputBytes uint64 `json:"reserved_output_bytes"`
	RetainedBytes       uint64 `json:"retained_bytes"`
	Sessions            uint64 `json:"sessions"`
	UnarchivedBytes     uint64 `json:"unarchived_bytes"`
}

type Transfer struct {
	Checkpoint   *RecordReference `json:"checkpoint,omitempty"`
	Error        *ErrorResponse   `json:"error,omitempty"`
	ID           string           `json:"id"`
	Phase        TransferPhase    `json:"phase"`
	SessionID    string           `json:"session_id"`
	SourceEpoch  string           `json:"source_epoch"`
	SourceNode   string           `json:"source_node"`
	TargetNode   string           `json:"target_node"`
	Through      string           `json:"through"`
	VerifiedRuns uint64           `json:"verified_runs"`
}

type TransferCreate struct {
	OwnerEpoch string `json:"owner_epoch"`
	RequestID  string `json:"request_id"`
	TargetNode string `json:"target_node"`
}

type TransferPhase string

const (
	TransferPhaseDraining    TransferPhase = "draining"
	TransferPhaseVerifying   TransferPhase = "verifying"
	TransferPhaseTransferred TransferPhase = "transferred"
	TransferPhaseCancelled   TransferPhase = "cancelled"
	TransferPhaseFailed      TransferPhase = "failed"
)

type UnavailablePayload struct {
	Bytes  uint64 `json:"bytes"`
	Digest string `json:"digest"`
}

type Usage struct {
	CachedTokens        *uint64             `json:"cached_tokens,omitempty"`
	InputTokens         *uint64             `json:"input_tokens,omitempty"`
	OutputTokens        *uint64             `json:"output_tokens,omitempty"`
	Provider            json.RawMessage     `json:"provider,omitempty"`
	ProviderUnavailable *UnavailablePayload `json:"provider_unavailable,omitempty"`
}

type Wait struct {
	ID   string `json:"id"`
	Kind string `json:"kind"`
}

var Routes = map[string]string{
	"GET /health/live":  "",
	"GET /health/ready": "",
	"GET /internal/v2/records/{session}/{sequence}":                 "",
	"POST /operator/drain":                                          "",
	"GET /operator/nodes":                                           "",
	"POST /operator/sessions/{session}/transfers":                   "TransferCreate",
	"DELETE /operator/sessions/{session}/transfers/{transfer}":      "",
	"GET /operator/sessions/{session}/transfers/{transfer}":         "",
	"GET /operator/storage":                                         "",
	"GET /v2/catalog":                                               "",
	"POST /v2/packages":                                             "PackageAdmission",
	"GET /v2/packages/{package}":                                    "",
	"GET /v2/sessions":                                              "",
	"POST /v2/sessions":                                             "SessionCreate",
	"DELETE /v2/sessions/{session}":                                 "",
	"GET /v2/sessions/{session}":                                    "",
	"GET /v2/sessions/{session}/budget":                             "",
	"GET /v2/sessions/{session}/commands/{command}":                 "",
	"GET /v2/sessions/{session}/connections":                        "",
	"POST /v2/sessions/{session}/connections":                       "ConnectionAttach",
	"DELETE /v2/sessions/{session}/connections/{connection}":        "",
	"POST /v2/sessions/{session}/delegations":                       "Delegation",
	"GET /v2/sessions/{session}/events":                             "",
	"GET /v2/sessions/{session}/export":                             "",
	"GET /v2/sessions/{session}/files":                              "",
	"DELETE /v2/sessions/{session}/files/{file}":                    "",
	"GET /v2/sessions/{session}/files/{file}":                       "",
	"PUT /v2/sessions/{session}/files/{file}":                       "",
	"POST /v2/sessions/{session}/handoff":                           "Handoff",
	"POST /v2/sessions/{session}/inputs":                            "Input",
	"GET /v2/sessions/{session}/interactions":                       "",
	"POST /v2/sessions/{session}/interactions":                      "InteractionCreate",
	"POST /v2/sessions/{session}/interactions/{interaction}":        "InteractionAnswer",
	"GET /v2/sessions/{session}/memory":                             "",
	"DELETE /v2/sessions/{session}/memory/{key}":                    "",
	"PUT /v2/sessions/{session}/memory/{key}":                       "DocumentUpdate",
	"GET /v2/sessions/{session}/operations":                         "",
	"GET /v2/sessions/{session}/operations/{operation}":             "",
	"POST /v2/sessions/{session}/operations/{operation}/claim":      "Claim",
	"POST /v2/sessions/{session}/operations/{operation}/receipt":    "Receipt",
	"POST /v2/sessions/{session}/operations/{operation}/retry":      "Retry",
	"POST /v2/sessions/{session}/participants":                      "ParticipantGrant",
	"DELETE /v2/sessions/{session}/participants/{participant}":      "",
	"GET /v2/sessions/{session}/records":                            "",
	"GET /v2/sessions/{session}/runs":                               "",
	"GET /v2/sessions/{session}/runs/{run}/children":                "",
	"GET /v2/sessions/{session}/runs/{run}/children/{child}/result": "",
	"GET /v2/sessions/{session}/schedules":                          "",
	"POST /v2/sessions/{session}/schedules":                         "ScheduleCreate",
	"DELETE /v2/sessions/{session}/schedules/{schedule}":            "",
	"GET /v2/sessions/{session}/state":                              "",
	"DELETE /v2/sessions/{session}/state/{key}":                     "",
	"PUT /v2/sessions/{session}/state/{key}":                        "DocumentUpdate",
	"POST /v2/sessions/{session}/webhooks":                          "Input",
}
