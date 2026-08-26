/** Maximum UTF-8 JSON bytes accepted by the Session message endpoint. */
export const MAX_MESSAGE_REQUEST_BYTES = 192 * 1024;
export const MAX_MANAGED_TOOL_INPUT_BYTES = 192 * 1024;

/** Maximum UTF-8 JSON bytes accepted by the Session create endpoint. */
export const MAX_CREATE_SESSION_REQUEST_BYTES = 144 * 1024 * 1024;

/** Maximum serialized bytes of one session's immutable sealed configuration journal record. */
export const MAX_SEALED_CONFIG_BYTES = 256 * 1024;

/** Maximum bytes of one immutable Tool bundle handed to an Environment. */
export const MAX_TOOL_BUNDLE_BYTES = 4 * 1024 * 1024;

/** Maximum encoded bytes for one customer-Environment WebSocket command frame. */
export const MAX_CUSTOMER_WS_FRAME_BYTES = 24 * 1024;

/** Maximum UTF-8 JSON bytes accepted by customer-Environment HTTPS observation ingress. */
export const MAX_CUSTOMER_OBSERVATION_BYTES = 128 * 1024;

/** Maximum UTF-8 JSON payload bytes in one public Session event, excluding SSE framing. */
export const MAX_PUBLIC_EVENT_BYTES = 256 * 1024;

/** Maximum immutable customer Tool registrations retained by one process runner. */
export const MAX_CUSTOMER_REGISTRATIONS = 1024;

/** Maximum encoded registration descriptor bytes retained by one process runner. */
export const MAX_CUSTOMER_REGISTRATION_DESCRIPTOR_BYTES = 1024 * 1024;

/** Maximum RFC 8785 encoded bytes for an inline terminal Tool result. */
export const MAX_TOOL_TERMINAL_INLINE_BYTES = 92 * 1024;

/** Maximum encoded JSON bytes in model-supplied input sent to a trusted host executor. */
export const MAX_EXTERNAL_TOOL_INPUT_BYTES = 96 * 1024;

/** Maximum encoded JSON bytes in one complete Brain-to-host executor request. */
export const MAX_EXTERNAL_TOOL_REQUEST_BYTES = 512 * 1024;

/** Maximum encoded JSON bytes accepted from one trusted host-executor response. */
export const MAX_EXTERNAL_TOOL_RESPONSE_BYTES = 768 * 1024;
