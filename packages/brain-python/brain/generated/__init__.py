"""Generated from brain-protocol Rust types. Do not edit."""
from __future__ import annotations
from typing import Any, Literal, NotRequired, TypeAlias, TypedDict

class Action(TypedDict):
    approval: NotRequired[bool]
    connection: str
    description: str
    input_schema: Any
    max_output_bytes: int
    name: str
    output_schema: NotRequired[Any]
    package: NotRequired[str]
    timeout_ms: int

class CatalogEntry(TypedDict):
    capabilities: list[str]
    configuration_schema: Any
    credentials: list[str]
    kind: str
    limitations: list[str]
    name: str
    version: str

class ChildResult(TypedDict):
    availability: ChildResultAvailability
    command_id: str
    completion: NotRequired[RecordReference]
    error: NotRequired[ErrorResponse]
    index: int
    result: NotRequired[Any]
    run_id: str
    session_id: str
    status: RunStatus

class ChildResults(TypedDict):
    children: list[ChildResult]
    expected: int
    ready: bool
    run_id: str
    session_id: str
    step: int

class Claim(TypedDict):
    connection_id: str
    generation: str
    request_id: str

class CommandReceipt(TypedDict):
    accepted_ms: int
    error: NotRequired[ErrorResponse]
    id: str
    run_id: NotRequired[str]
    sequence: NotRequired[str]
    session_id: str
    status: str

class Connection(TypedDict):
    generation: str
    id: str
    ownership: Ownership
    resource_id: NotRequired[str]
    session_id: str
    status: str
    template: str

class ConnectionAttach(TypedDict):
    request_id: str
    resource_id: NotRequired[str]
    template: str

class ConnectionTemplate(TypedDict):
    capabilities: NotRequired[list[str]]
    configuration: NotRequired[Any]
    kind: ConnectionKind
    name: str
    ownership: Ownership
    provider: str
    secrets: NotRequired[dict[str, str]]

class DecisionCall(TypedDict):
    action: str
    arguments: Any
    checkpoint: Any
    effect: EffectKind
    kind: Literal["call"]

class DecisionComplete(TypedDict):
    checkpoint: Any
    kind: Literal["complete"]
    result: Any

class DecisionWait(TypedDict):
    checkpoint: Any
    kind: Literal["wait"]
    wait: Wait

class DecisionEmit(TypedDict):
    checkpoint: Any
    data: Any
    kind: Literal["emit"]
    name: str
    version: str

class DecisionInteract(TypedDict):
    checkpoint: Any
    interaction: InteractionCreate
    kind: Literal["interact"]

class DecisionDelegate(TypedDict):
    checkpoint: Any
    delegation: Delegation
    kind: Literal["delegate"]

class DecisionFork(TypedDict):
    checkpoint: Any
    delegations: list[Delegation]
    kind: Literal["fork"]

class Delegation(TypedDict):
    config: SessionConfig
    context: list[str]
    message: str
    request_id: str

class Document(TypedDict):
    expires_ms: NotRequired[int]
    key: str
    revision: str
    value: Any

class DocumentUpdate(TypedDict):
    expires_ms: NotRequired[int]
    request_id: str
    revision: str
    value: Any

class ErrorResponse(TypedDict):
    code: ErrorCode
    message: str
    request_id: NotRequired[str]
    retryable: bool

class EventRoute(TypedDict):
    action: NotRequired[str]
    disposition: Disposition
    name: str
    schema: Any
    sources: NotRequired[list[str]]
    version: str

class Execution(TypedDict):
    arguments: Any
    deadline_ms: int
    max_output_bytes: int
    operation: Operation
    receipt_token: str

class ExportManifest(TypedDict):
    complete: bool
    connections: list[Connection]
    continuation: str
    files: list[FileInfo]
    format: str
    history: RecordPage
    interactions: list[Interaction]
    memory: list[Document]
    operations: list[Operation]
    runs: list[Run]
    schedules: list[Schedule]
    section: ExportSection
    session: SessionView
    state: list[Document]

class FileInfo(TypedDict):
    digest: str
    media_type: str
    name: str
    provenance: str
    size: int

class Gap(TypedDict):
    first: str
    kind: GapKind
    last: str

class Handoff(TypedDict):
    harness: HarnessSelection
    request_id: str
    revision: str

class HarnessRequest(TypedDict):
    checkpoint: Any
    children: NotRequired[list[ChildResult]]
    command_id: str
    config: SessionConfig
    context: list[Record]
    input: InputData
    outcome: NotRequired[Operation]
    protocol: str
    run_id: str
    session_id: str
    step: int

class HarnessSelection(TypedDict):
    configuration: NotRequired[Any]
    name: str
    version: str

class Input(TypedDict):
    data: InputData
    request_id: str

class InputDataMessage(TypedDict):
    kind: Literal["message"]
    text: str

class InputDataEvent(TypedDict):
    correlation: NotRequired[str]
    data: Any
    kind: Literal["event"]
    name: str
    version: str

class InputDataAction(TypedDict):
    arguments: Any
    kind: Literal["action"]
    name: str
    retry_of: NotRequired[str]

class InputDataControl(TypedDict):
    control: ControlKind
    kind: Literal["control"]

class Interaction(TypedDict):
    answer: NotRequired[Any]
    id: str
    request: InteractionCreate
    revision: str
    session_id: str
    status: InteractionStatus

class InteractionAnswer(TypedDict):
    accept: bool
    answer: Any
    request_id: str
    revision: str
    subject_digest: str

class InteractionCreate(TypedDict):
    expires_ms: int
    kind: InteractionKind
    prompt: str
    request_id: str
    run_id: str
    schema: Any
    subject: str
    subject_digest: str

class Limits(TypedDict):
    calls: int
    concurrent: int
    deadline_ms: NotRequired[int]
    input_bytes: int
    output_bytes: int
    pending_inputs: int
    retained_bytes: int
    tokens: NotRequired[int]

class ManagedInvocation(TypedDict):
    callback_url: str
    execution: Execution
    package: Package
    secrets: dict[str, str]

class ModelMessage(TypedDict):
    content: Any
    role: str
    tool_call_id: NotRequired[str]
    tool_calls: NotRequired[list[ModelToolCall]]

class ModelRequest(TypedDict):
    actions: list[Action]
    messages: list[ModelMessage]

class ModelResponse(TypedDict):
    message: ModelMessage
    native: NotRequired[Any]
    usage: Usage

class ModelSelection(TypedDict):
    credential: NotRequired[str]
    format: NotRequired[Any]
    name: str
    native: NotRequired[Any]
    provider: str

class ModelToolCall(TypedDict):
    arguments: Any
    id: str
    name: str

class NodeInfo(TypedDict):
    draining: bool
    endpoint: str
    id: str
    lease_expires_ms: int

class Operation(TypedDict):
    action: str
    arguments_digest: str
    error: NotRequired[str]
    id: str
    kind: EffectKind
    recipient: str
    result: NotRequired[Any]
    result_available: bool
    retry_of: NotRequired[str]
    revision: str
    run_id: str
    session_id: str
    status: OperationStatus
    step_key: str
    usage: NotRequired[Usage]

class Package(TypedDict):
    digest: str
    error: NotRequired[str]
    id: str
    image: NotRequired[str]
    manifest: PackageManifest
    status: str

class PackageAdmission(TypedDict):
    builder: str
    manifest: PackageManifest
    request_id: str
    session_id: str

class PackageBuildResult(TypedDict):
    base_image: str
    builder: str
    image: str
    package_id: str
    source_digest: str

class PackageManifest(TypedDict):
    actions: list[Action]
    cpu_millis: int
    entrypoint: str
    files: dict[str, str]
    language: RuntimeLanguage
    lockfile: str
    memory_bytes: int
    name: str
    network: list[str]
    secrets: list[str]
    version: str
    wall_ms: int

class ParticipantGrant(TypedDict):
    expires_ms: int
    permissions: list[str]
    request_id: str
    subject: str

class ParticipantToken(TypedDict):
    expires_ms: int
    id: str
    token: str

class Receipt(TypedDict):
    error: NotRequired[str]
    outcome: Outcome
    request_id: str
    result: NotRequired[Any]
    usage: NotRequired[Usage]

class Record(TypedDict):
    data: Any
    kind: str
    operation_id: NotRequired[str]
    run_id: NotRequired[str]
    sequence: str
    session_id: str
    time_ms: int

class RecordPage(TypedDict):
    allocated_through: str
    gaps: list[Gap]
    more: bool
    next: str
    records: list[Record]

class RecordReference(TypedDict):
    digest: str
    sequence: str
    session_id: str

class Retry(TypedDict):
    acknowledge_uncertainty: bool
    request_id: str

class Run(TypedDict):
    command_id: str
    completion: NotRequired[RecordReference]
    error: NotRequired[ErrorResponse]
    id: str
    session_id: str
    status: RunStatus
    step: int
    wait: NotRequired[Wait]

class Schedule(TypedDict):
    due_ms: int
    id: str
    request: ScheduleCreate
    session_id: str
    status: str

class ScheduleCreate(TypedDict):
    due_ms: int
    input: InputData
    interval_ms: NotRequired[int]
    request_id: str

class SessionBudget(TypedDict):
    active_operations: int
    available_bytes: int
    available_operations: int
    available_tokens: NotRequired[str]
    calls: int
    incomplete_usages: int
    remaining_calls: int
    reported_tokens: str
    reserved_output_bytes: int
    reserved_tokens: str
    retained_bytes: int
    session_id: str
    token_usage_pending: bool

class SessionConfig(TypedDict):
    actions: NotRequired[list[Action]]
    connections: NotRequired[list[ConnectionTemplate]]
    events: NotRequired[list[EventRoute]]
    grants: NotRequired[list[str]]
    harness: HarnessSelection
    harness_transitions: NotRequired[list[HarnessSelection]]
    instructions: NotRequired[str]
    limits: Limits
    model: ModelSelection
    skills: NotRequired[list[str]]

class SessionCreate(TypedDict):
    config: SessionConfig
    message: NotRequired[str]
    request_id: str

class SessionPage(TypedDict):
    next: NotRequired[str]
    sessions: list[SessionView]

class SessionView(TypedDict):
    config: SessionConfig
    created_ms: int
    id: str
    lifecycle: Lifecycle
    owner_epoch: str
    parent: NotRequired[str]
    paused: bool
    revision: str
    sequence: str

class StorageReport(TypedDict):
    accepting: bool
    archive_queue_bytes: int
    cache_bytes: int
    format: str
    oldest_unarchived_ms: int
    reserved_output_bytes: int
    retained_bytes: int
    sessions: int
    unarchived_bytes: int

class Transfer(TypedDict):
    checkpoint: NotRequired[RecordReference]
    error: NotRequired[ErrorResponse]
    id: str
    phase: TransferPhase
    session_id: str
    source_epoch: str
    source_node: str
    target_node: str
    through: str
    verified_runs: int

class TransferCreate(TypedDict):
    owner_epoch: str
    request_id: str
    target_node: str

class UnavailablePayload(TypedDict):
    bytes: int
    digest: str

class Usage(TypedDict):
    cached_tokens: NotRequired[int]
    input_tokens: NotRequired[int]
    output_tokens: NotRequired[int]
    provider: NotRequired[Any]
    provider_unavailable: NotRequired[UnavailablePayload]

class Wait(TypedDict):
    id: str
    kind: str

ChildResultAvailability: TypeAlias = Literal["pending", "inline", "referenced", "unavailable", "none"]
ConnectionKind: TypeAlias = Literal["application", "http", "local", "modal", "fly", "mcp", "storage"]
ControlKind: TypeAlias = Literal["pause", "resume", "cancel", "close"]
Decision: TypeAlias = DecisionCall | DecisionComplete | DecisionWait | DecisionEmit | DecisionInteract | DecisionDelegate | DecisionFork
Disposition: TypeAlias = Literal["application", "state", "context", "wake", "steer", "interrupt"]
EffectKind: TypeAlias = Literal["model", "action", "build", "lifecycle", "webhook"]
ErrorCode: TypeAlias = Literal["invalid", "unauthorized", "forbidden", "not_found", "conflict", "overloaded", "unsupported", "unavailable", "uncertain", "corrupt", "internal"]
ExportSection: TypeAlias = Literal["history", "runs", "operations", "interactions", "connections", "files", "state", "memory", "schedules"]
GapKind: TypeAlias = Literal["reserved", "unused", "unavailable", "unresolved"]
InputData: TypeAlias = InputDataMessage | InputDataEvent | InputDataAction | InputDataControl
InteractionKind: TypeAlias = Literal["question", "form", "approval"]
InteractionStatus: TypeAlias = Literal["pending", "answered", "rejected", "expired", "cancelled"]
Lifecycle: TypeAlias = Literal["open", "closing", "closed", "deleted"]
OperationStatus: TypeAlias = Literal["prepared", "dispatched", "completed", "failed", "unknown", "cancelled"]
Outcome: TypeAlias = Literal["completed", "failed", "unknown"]
Ownership: TypeAlias = Literal["managed", "borrowed"]
RunStatus: TypeAlias = Literal["queued", "active", "waiting", "paused", "completed", "failed", "cancelled", "blocked"]
RuntimeLanguage: TypeAlias = Literal["node", "python"]
TransferPhase: TypeAlias = Literal["draining", "verifying", "transferred", "cancelled", "failed"]
