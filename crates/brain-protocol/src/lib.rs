//! Brain-owned public protocols.
//!
//! JSON Schemas under `contracts/` are authoritative. [`environment`], [`session`] and [`agentloop`] are
//! generated from them; [`contract`] contains the exact digests and operation hashing shared by
//! Brain, Environments and loop hosts.

#[allow(clippy::all, clippy::pedantic)]
pub mod agentloop;
pub mod contract;
#[allow(clippy::all, clippy::pedantic)]
pub mod environment;
pub mod network;
mod redaction;
#[allow(clippy::all, clippy::pedantic)]
pub mod session;

pub const SESSION_SCHEMA_JSON: &str = include_str!("../../../contracts/session/v1/schemas.json");

/// Maximum UTF-8 JSON bytes accepted for `POST /v1/sessions/{id}/messages`.
pub const MAX_MESSAGE_REQUEST_BYTES: usize = 192 * 1024;

/// Maximum RFC 8785 bytes in one inline managed-Environment Tool argument. Managed execution and Environment
/// binding admission use this distinct contract even though it currently equals the message
/// request ceiling; the 96-KiB external-executor input bound is a separate route.
pub const MAX_MANAGED_TOOL_INPUT_BYTES: usize = 192 * 1024;

/// Maximum UTF-8 JSON bytes accepted for `POST /v1/sessions`. This accommodates the sealed
/// 16-MiB raw bundle aggregate after base64 expansion while retaining a hard allocation bound.
pub const MAX_CREATE_SESSION_REQUEST_BYTES: usize = 24 * 1024 * 1024;

/// Maximum decoded bytes in one immutable managed Tool bundle and across every bundle in one
/// Session create. The request ceiling separately includes base64/JSON framing.
pub const MAX_TOOL_BUNDLE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SESSION_BUNDLE_BYTES: usize = 16 * 1024 * 1024;

/// Managed-Tool session secret bounds. Names are restricted to the ASCII environment grammar,
/// while the schema's 2,048-Unicode-scalar value bound needs 8 KiB of UTF-8 headroom. The whole
/// canonical secret document is additionally capped at the direct-custody plaintext ceiling.
pub const MAX_SESSION_SECRET_NAMES: usize = 128;
pub const MAX_SESSION_SECRET_NAME_BYTES: usize = 128;
pub const MAX_SESSION_SECRET_VALUE_UTF8_BYTES: usize = 8 * 1024;
pub const MAX_SESSION_SECRET_DOCUMENT_BYTES: usize = 4 * 1024;

/// Maximum aggregate model-visible Tool definitions in one session.
/// This is the portable provider ceiling and is enforced independently of execution concurrency.
pub const MAX_MODEL_TOOLS: usize = 128;

/// Maximum RFC 8785 bytes for the aggregate model-visible Tool name, description, and input
/// schema projection. Immutable session configuration has its own enclosing item bound.
pub const MAX_MODEL_TOOL_DEFINITION_BYTES: usize = 192 * 1024;

/// Maximum provider output-token request accepted into an immutable session seal.
pub const MAX_MODEL_OUTPUT_TOKENS: u32 = 128 * 1024;

/// Conservative context window sealed when an arbitrary model does not declare one. Brain does
/// not infer capacity from a mutable provider/model-name catalog.
pub const DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS: u32 = 32 * 1024;
pub const MIN_MODEL_CONTEXT_WINDOW_TOKENS: u32 = 8 * 1024;
pub const MAX_MODEL_CONTEXT_WINDOW_TOKENS: u32 = 2_000_000;

/// Maximum encoded bytes for one customer-Environment WebSocket command frame.
pub const MAX_CUSTOMER_WS_FRAME_BYTES: usize = 24 * 1024;

/// Maximum UTF-8 JSON bytes accepted by the customer-Environment HTTPS observation ingress.
pub const MAX_CUSTOMER_OBSERVATION_BYTES: usize = 128 * 1024;

/// Maximum UTF-8 JSON bytes in one public Session event. SSE framing (`event`, optional durable
/// `id`, the single `data` prefix, and delimiters) is not included in this payload ceiling.
pub const MAX_PUBLIC_EVENT_BYTES: usize = 256 * 1024;

/// Maximum immutable customer Tool registrations held by one process-scoped runner.
pub const MAX_CUSTOMER_REGISTRATIONS: usize = 1024;

/// Maximum encoded registration descriptor bytes retained for one customer runner.
pub const MAX_CUSTOMER_REGISTRATION_DESCRIPTOR_BYTES: usize = 1024 * 1024;

/// Maximum RFC 8785 encoded bytes for `TerminalResult.inline`. The remaining 4 KiB of the
/// journal's 96-KiB result-content budget is reserved for outcome/digest/timing metadata.
pub const MAX_TOOL_TERMINAL_INLINE_BYTES: usize = 92 * 1024;

/// Maximum encoded JSON bytes in the model-supplied input projection sent to a trusted host
/// executor. This is checked independently from the enclosing request wire ceiling.
pub const MAX_EXTERNAL_TOOL_INPUT_BYTES: usize = 96 * 1024;

/// Maximum encoded JSON bytes in one complete Brain-to-host executor request. A valid call can
/// carry an independently bounded 96-KiB model input plus the bounded metadata of its active
/// 192-KiB message; the remaining headroom covers identifiers, capability context and JSON
/// framing. Hosted ingress must authenticate before reading at most this many bytes.
pub const MAX_EXTERNAL_TOOL_REQUEST_BYTES: usize = 512 * 1024;

/// Maximum encoded JSON bytes accepted from one trusted host-executor response. Content and the
/// structured terminal projection are independently limited to 92 KiB, but JSON escaping can
/// expand a valid UTF-8 content string by up to six times. This enclosing wire bound leaves
/// deterministic headroom for that worst case and the remaining response fields.
pub const MAX_EXTERNAL_TOOL_RESPONSE_BYTES: usize = 768 * 1024;

/// Maximum UTF-8 bytes in one idempotent stdin append. This matches the canonical minimum
/// `PIPE_BUF`, allowing a Environment to perform one bounded write without partial-effect ambiguity.
pub const MAX_WRITE_STDIN_BYTES: usize = 4 * 1024;

/// Agentloop bounds the schema cannot express in bytes; enforced at the execution boundary.
/// A loop-authored `custom`/`event` entry's RFC 8785 data stays inside the journal result budget.
pub const MAX_LOOP_ENTRY_DATA_BYTES: usize = 96 * 1024;
/// A `mark` carries the loop's compacted context and is chunked internally beyond record size.
pub const MAX_LOOP_MARK_DATA_BYTES: usize = 2 * 1024 * 1024;
/// The current kernel accepts only marks that fit one journal record inline; larger marks (up
/// to [`MAX_LOOP_MARK_DATA_BYTES`]) need the chunking machinery, which lands with the loop SDK.
/// Sized under the 256 KiB record cap with envelope headroom.
pub const MAX_LOOP_MARK_INLINE_BYTES: usize = 192 * 1024;
/// Keyed loop state: per-session key count and per-value RFC 8785 bytes.
pub const MAX_LOOP_KV_KEYS: usize = 64;
pub const MAX_LOOP_KV_VALUE_BYTES: usize = 8 * 1024;
/// Maximum encoded bytes in one complete activation request (session_start carries the tail).
pub const MAX_ACTIVATION_REQUEST_BYTES: usize = 4 * 1024 * 1024;
/// Maximum encoded bytes in one ctx operation request or response.
pub const MAX_CTX_OP_BYTES: usize = 768 * 1024;
/// Upload bound for a custom loop source bundle. The frozen agentloop contract's
/// `source_bundle_bytes.maximum` is the authority; a unit test pins this constant to it.
pub const MAX_LOOP_BUNDLE_BYTES: usize = 8 * 1024 * 1024;
