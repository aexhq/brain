/* eslint-disable */
/**
 * GENERATED from contracts/session/v1/openapi.yaml by packages/contracts/scripts/gen.mjs (tools/gen.sh). DO NOT EDIT.
 */
export type paths = {
    "/v1/sessions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List sessions (newest first) */
        get: operations["listSessions"];
        put?: never;
        /** Create a session (seals model, prompt, tools; starts preparing the hand) */
        post: operations["createSession"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        /** Get a session */
        get: operations["getSession"];
        put?: never;
        post?: never;
        /** Delete a session (irreversible; releases the hand, deletes workspace, artifacts and journal) */
        delete: operations["deleteSession"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/messages": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Send a user message; starts a turn
         * @description Returns 202 as soon as the turn is admitted and journaled. Follow progress on
         *     `GET /events?after=<seq-1>`. An optional `output` requests a typed result for this turn;
         *     a successful result is attached to `turn.completed`. It does not change provider response
         *     configuration or the session's sealed prefix. 409 `session_busy` while a turn is running.
         */
        post: operations["sendMessage"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/events": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        /**
         * Server-sent events for a session, replayable from the journal
         * @description `text/event-stream`. Each event is `id: <seq>`, `event: <type>`, `data: <Event JSON>`.
         *     Pass `after` (or the `Last-Event-ID` header) to resume. With `follow=false` the stream
         *     ends after the journal is drained; otherwise it stays open across turns until the session
         *     is deleted.
         */
        get: operations["streamEvents"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/cancel": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Cancel the running turn (in-flight tool calls are cancelled; background jobs keep running) */
        post: operations["cancelTurn"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/end": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** End now — cancel the turn, stop background jobs, sync and release the hand; keep the workspace */
        post: operations["endSession"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/files": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        /** List workspace files (live from the hand when it is up, else from the last sync manifest) */
        get: operations["listFiles"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/files/{path}": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                /** @description Absolute guest path, URL-encoded, e.g. `%2Fworkspace%2Fsrc%2Fmain.rs`. */
                path: string;
            };
            cookie?: never;
        };
        /** Download one file (raw bytes; 64 MiB deployment ceiling) */
        get: operations["downloadFile"];
        /** Upload one file into the workspace (raw bytes; overwrites; checkpoints before acknowledgement; 64 MiB ceiling) */
        put: operations["uploadFile"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/persist": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Copy a workspace file into durable, named artifact storage */
        post: operations["persistArtifact"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/artifacts": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        /** List artifacts */
        get: operations["listArtifacts"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/artifacts/{name}": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                name: string;
            };
            cookie?: never;
        };
        /** Get one artifact with a short-lived download URL */
        get: operations["getArtifact"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
};
export type webhooks = Record<string, never>;
export type components = {
    schemas: {
        /**
         * @description active = a turn is running or a background job is live; idle = waiting for the next message (hand may be running, suspended or released underneath); deleted = irreversible; failed = the session cannot continue (see Session.failure).
         * @enum {string}
         */
        SessionState: "active" | "idle" | "deleted" | "failed";
        /** @enum {string} */
        ApiErrorCode: "invalid_request" | "unauthorized" | "forbidden" | "not_found" | "conflict" | "session_busy" | "session_deleted" | "session_failed" | "cancelled" | "insufficient_balance" | "rate_limited" | "provider_error" | "output_schema_error" | "output_refused" | "output_validation_error" | "hand_unavailable" | "too_large" | "internal";
        ApiError: {
            code: components["schemas"]["ApiErrorCode"];
            message: string;
            /** @description JSON pointer to the offending request field, when applicable. */
            param?: string;
            request_id?: string;
            /** @description Machine-readable failure details when available, such as bounded validation issues. */
            details?: unknown;
        };
        ApiErrorResponse: {
            error: components["schemas"]["ApiError"];
        };
        SessionId: string;
        /**
         * @description openai and anthropic are certified; the rest are available uncertified.
         * @enum {string}
         */
        Provider: "openai" | "anthropic" | "deepseek" | "moonshot" | "xai" | "openai_compatible";
        /** @description ModelConfig without the key. */
        ModelInfo: {
            provider: components["schemas"]["Provider"];
            name: string;
            /** Format: uri */
            base_url?: string;
        };
        /**
         * @description preparing = microVM launching or restoring; ready = running and connected; suspended = AWS holds RAM+disk after 180 s idle, compute free, ~1 s back; released = VM destroyed, workspace synced to storage, ~3 s back into a fresh VM; lost = the hand died mid-run (in-flight calls reported as interrupted, never replayed).
         * @enum {string}
         */
        HandState: "preparing" | "ready" | "suspended" | "released" | "lost";
        /**
         * @description Baseline memory; vCPU = memory/2; bursts to 4x. Default 1gb.
         * @enum {string}
         */
        HandShape: "1gb" | "2gb" | "4gb" | "8gb";
        /**
         * Format: date-time
         * @description RFC 3339, UTC.
         */
        Timestamp: string;
        HandInfo: {
            state: components["schemas"]["HandState"];
            shape: components["schemas"]["HandShape"];
            /** @description How many microVM incarnations this session has had. */
            generation?: number;
            /** @description When the current incarnation launched. */
            started_at?: components["schemas"]["Timestamp"];
            /** @description When the platform will sync + release this incarnation (8 h after launch). */
            wall_deadline_at?: components["schemas"]["Timestamp"];
            last_sync_at?: components["schemas"]["Timestamp"];
            /** @description Background jobs still running. */
            live_jobs?: number;
        };
        /** @description Billed storage, visible from day one. */
        StorageInfo: {
            /** @description Synced workspace objects (packs + manifests) in storage. */
            workspace_bytes: number;
            /** @description Bytes AWS holds for a suspended hand. */
            suspended_bytes: number;
            artifact_bytes: number;
        };
        TurnId: string;
        SessionFailure: {
            /** @enum {string} */
            code: "tool_manifest_mismatch" | "provider_unusable" | "hand_unavailable" | "internal";
            message: string;
            at: components["schemas"]["Timestamp"];
        };
        Session: {
            id: components["schemas"]["SessionId"];
            /** @enum {string} */
            object: "session";
            state: components["schemas"]["SessionState"];
            model: components["schemas"]["ModelInfo"];
            hand: components["schemas"]["HandInfo"];
            storage: components["schemas"]["StorageInfo"];
            created_at: components["schemas"]["Timestamp"];
            updated_at: components["schemas"]["Timestamp"];
            last_message_at?: components["schemas"]["Timestamp"];
            turns: number;
            current_turn?: components["schemas"]["TurnId"];
            failure?: components["schemas"]["SessionFailure"];
            /** @description Customer key/value; up to 16 pairs. */
            metadata: {
                [key: string]: string;
            };
        };
        SessionList: {
            /** @enum {string} */
            object: "list";
            data: components["schemas"]["Session"][];
            has_more: boolean;
            next_cursor?: string;
        };
        ModelConfig: {
            provider: components["schemas"]["Provider"];
            /** @description Provider model id, e.g. "claude-sonnet-5" or "gpt-5". */
            name: string;
            /** @description BYOK. Encrypted per session, never returned, never logged. */
            api_key: string;
            /**
             * Format: uri
             * @description Override the provider endpoint (required for openai_compatible).
             */
            base_url?: string;
            max_output_tokens?: number;
            temperature?: number;
            /**
             * @description Passed through where the provider supports it.
             * @enum {string}
             */
            reasoning_effort?: "low" | "medium" | "high";
        };
        /**
         * @description bash..ls run in the hand; task/todo run in the brain; web_search/web_fetch are managed and billed.
         * @enum {string}
         */
        BuiltinTool: "bash" | "read" | "write" | "edit" | "glob" | "grep" | "ls" | "task" | "todo" | "web_search" | "web_fetch";
        /**
         * @description auto probes server/discover and falls back to the legacy adapter (initialize + Mcp-Session-Id).
         * @enum {string}
         */
        McpProtocol: "auto" | "2026-07" | "legacy";
        McpServerConfig: {
            /** @description Prefix for its tools ("name__tool"). */
            name: string;
            /** Format: uri */
            url: string;
            /** @description Sent on every request (e.g. Authorization). Encrypted per session, never returned. */
            headers?: {
                [key: string]: string;
            };
            protocol?: components["schemas"]["McpProtocol"];
            /** @description Whitelist; default all. */
            allowed_tools?: string[];
        };
        /**
         * @description Host-executed tools are root-only in the MVP, keeping terminal control out of subagents.
         * @enum {string}
         */
        ExternalToolScope: "root";
        /**
         * @description continue returns the result to the model. return_direct may complete or fail the turn without another model call.
         * @enum {string}
         */
        ExternalToolCompletion: "continue" | "return_direct";
        /**
         * @description replay_safe promises that repeating the same session_id and call_id returns the same logical result.
         * @enum {string}
         */
        ExternalToolEffect: "opaque" | "replay_safe";
        /** @description A model-visible tool executed by the Brain host's configured external executor. The executor address and credentials are host configuration, never session data. */
        ExternalToolConfig: {
            name: string;
            description: string;
            input_schema: {
                [key: string]: unknown;
            };
            scope: components["schemas"]["ExternalToolScope"];
            completion: components["schemas"]["ExternalToolCompletion"];
            effect: components["schemas"]["ExternalToolEffect"];
            max_input_bytes: number;
        };
        /** @description Sealed at create with the rest of the prefix. Omitted tools default to an empty set. */
        ToolsConfig: {
            /** @description Built-in tools to enable. Omitted or empty means no built-in tools. */
            builtin?: components["schemas"]["BuiltinTool"][];
            mcp?: components["schemas"]["McpServerConfig"][];
            /** @description Host-executed tools sealed into the model prefix. Hosted Aex reserves its own output tool; direct Brain deployments may compose others. */
            external?: components["schemas"]["ExternalToolConfig"][];
        };
        HandConfig: {
            /**
             * @description false = no sandbox; hand tools are unavailable.
             * @default true
             */
            enabled: boolean;
            shape?: components["schemas"]["HandShape"];
            /** @description Environment for the agent's shell. Encrypted per session, never returned. */
            env?: {
                [key: string]: string;
            };
            /**
             * @description Mid-turn workspace sync period.
             * @default 600
             */
            sync_interval_seconds: number;
            /** @description Optional cap on how long a background job may keep the hand running after the turn ends. Absent or null = no cap. */
            max_background_minutes?: number | null;
        };
        /** @description Small files placed into the workspace at create (limit 1 MiB each). Larger files: PUT /files/{path} after create. */
        FileInput: {
            /** @description Relative to /workspace. */
            path: string;
            content_base64: string;
            mode?: number;
        };
        /** @description Everything here except metadata is part of the immutable prefix: it cannot change for the life of the session. */
        CreateSessionRequest: {
            model: components["schemas"]["ModelConfig"];
            system_prompt?: string;
            tools?: components["schemas"]["ToolsConfig"];
            hand?: components["schemas"]["HandConfig"];
            files?: components["schemas"]["FileInput"][];
            metadata?: {
                [key: string]: string;
            };
        };
        ContentPart: {
            /** @enum {string} */
            type: "text";
            text: string;
        } | {
            /** @enum {string} */
            type: "workspace_file";
            /** @description A file already in the workspace; the model is told about it. */
            path: string;
        };
        /** @description JSON Schema 2020-12 produced by the SDK. Aex validates it in the trusted host executor; it is never provider-native response-format configuration. */
        OutputSchema: {
            [key: string]: unknown;
        };
        Sha256Hex: string;
        MessageOutput: {
            schema: components["schemas"]["OutputSchema"];
            /** @description SHA-256 of RFC 8785 canonical JSON for schema. The server rejects a mismatch before calling the model. */
            schema_hash: components["schemas"]["Sha256Hex"];
            /**
             * @description Extra model attempts after the first invalid candidate.
             * @default 1
             */
            retries: number;
        };
        MessageRequest: {
            content: string | components["schemas"]["ContentPart"][];
            metadata?: {
                [key: string]: string;
            };
            /** @description Optional typed result requested for this turn. It is a per-message operation, not session configuration. */
            output?: components["schemas"]["MessageOutput"];
        };
        /** @description Correlation id for one output request. It is not a separately managed resource. */
        OutputId: string;
        /** @description The turn was admitted and journaled. Follow it on GET /events?after=<seq-1>. */
        MessageAccepted: {
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            /** @description Journal sequence of the turn.started event. */
            seq: number;
            /** @description Present when this message requested typed output. */
            output_id?: components["schemas"]["OutputId"];
            /** @description Present when this message requested typed output. */
            schema_hash?: components["schemas"]["Sha256Hex"];
        };
        /** @description "root" for the session's root agent; subagents get brain-minted ids. */
        AgentId: string;
        /** @description Brain-minted id of one tool call (equals the ABI operation_id for hand tools). */
        CallId: string;
        /** @enum {string} */
        ToolOutcome: "completed" | "failed" | "cancelled" | "deadline_exceeded" | "interrupted";
        /** @description Raw provider counters for one model call. A counter the provider did not send is absent here — never reported as 0. */
        ProviderUsage: {
            input_tokens?: number;
            output_tokens?: number;
            cache_read_input_tokens?: number;
            cache_creation_input_tokens?: number;
            reasoning_tokens?: number;
        };
        /** @enum {string} */
        StopReason: "end_turn" | "max_rounds" | "cancelled" | "error";
        /** @description A replayable client-facing result returned directly by a generic external tool. */
        TurnResult: {
            call_id: components["schemas"]["CallId"];
            name: string;
            value: unknown;
            metadata?: {
                [key: string]: string;
            };
        };
        /** @description One journal event, delivered over SSE as `event: <type>` with `id: <seq>` and this object as data. Discriminated by `type`. */
        Event: {
            /** @enum {string} */
            type: "turn.started";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
        } | {
            /** @enum {string} */
            type: "assistant.delta";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            agent_id: components["schemas"]["AgentId"];
            text: string;
        } | {
            /** @enum {string} */
            type: "assistant.message";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            agent_id: components["schemas"]["AgentId"];
            /** @description The complete assistant text of one model round. */
            text: string;
        } | {
            /** @enum {string} */
            type: "tool.call";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            agent_id: components["schemas"]["AgentId"];
            call_id: components["schemas"]["CallId"];
            name: string;
            input: unknown;
            detach: boolean;
        } | {
            /** @enum {string} */
            type: "tool.output";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            call_id: components["schemas"]["CallId"];
            /** @enum {string} */
            stream: "stdout" | "stderr";
            offset: number;
            /** @description Bounded, lossy UTF-8 preview of the bytes from offset. */
            text: string;
        } | {
            /** @enum {string} */
            type: "tool.result";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            agent_id: components["schemas"]["AgentId"];
            call_id: components["schemas"]["CallId"];
            name: string;
            outcome: components["schemas"]["ToolOutcome"];
            exit_code?: number | null;
            duration_ms: number;
            /** @description What the model was shown, bounded. */
            output_preview: string;
            truncated: boolean;
            /** @description Present when outcome != completed. */
            error?: string;
        } | {
            /** @enum {string} */
            type: "agent.spawned";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            agent_id: components["schemas"]["AgentId"];
            parent_agent_id: components["schemas"]["AgentId"];
            depth: number;
            description: string;
        } | {
            /** @enum {string} */
            type: "agent.finished";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            agent_id: components["schemas"]["AgentId"];
            /** @enum {string} */
            outcome: "completed" | "failed" | "cancelled";
            summary?: string;
        } | {
            /** @enum {string} */
            type: "model.usage";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            agent_id: components["schemas"]["AgentId"];
            provider: components["schemas"]["Provider"];
            model: string;
            usage: components["schemas"]["ProviderUsage"];
        } | {
            /** @enum {string} */
            type: "session.updated";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id?: components["schemas"]["TurnId"];
            state: components["schemas"]["SessionState"];
            hand: components["schemas"]["HandInfo"];
        } | {
            /** @enum {string} */
            type: "hand.lost";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id?: components["schemas"]["TurnId"];
            /** @description Calls whose outcome is unknown; they are reported to the model as interrupted and never replayed. */
            interrupted_calls: components["schemas"]["CallId"][];
            /** @description Last successful sync; work after it is lost. */
            workspace_synced_at?: components["schemas"]["Timestamp"];
        } | {
            /** @enum {string} */
            type: "turn.completed";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            stop_reason: components["schemas"]["StopReason"];
            /** @description Model calls in this turn (root agent). */
            rounds: number;
            tool_calls: number;
            /** @description Present when a return_direct external tool completed the turn. */
            result?: components["schemas"]["TurnResult"];
        } | {
            /** @enum {string} */
            type: "turn.failed";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            error: components["schemas"]["ApiError"];
        };
        FileEntry: {
            path: string;
            /** @enum {string} */
            kind: "file" | "dir" | "symlink";
            size?: number;
            modified_at?: components["schemas"]["Timestamp"];
            sha256?: components["schemas"]["Sha256Hex"];
        };
        FileList: {
            /** @enum {string} */
            object: "list";
            data: components["schemas"]["FileEntry"][];
            /**
             * Format: date-time
             * @description Time of the manifest this listing reflects; null when the workspace has never synced.
             */
            synced_at: string | null;
            /**
             * @description hand = live listing from a running hand; manifest = from the last sync (hand released).
             * @enum {string}
             */
            source: "hand" | "manifest";
        };
        PersistRequest: {
            name: string;
            /** @description Workspace path to persist as a named, downloadable artifact. */
            path: string;
            media_type?: string;
        };
        Artifact: {
            /** @enum {string} */
            object: "artifact";
            session_id: components["schemas"]["SessionId"];
            name: string;
            bytes: number;
            sha256: components["schemas"]["Sha256Hex"];
            media_type: string;
            created_at: components["schemas"]["Timestamp"];
            /**
             * Format: uri
             * @description Short-lived; present on GET of a single artifact.
             */
            download_url?: string;
            download_url_expires_at?: components["schemas"]["Timestamp"];
        };
        ArtifactList: {
            /** @enum {string} */
            object: "list";
            data: components["schemas"]["Artifact"][];
        };
    };
    responses: {
        /** @description Error */
        Error: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["ApiErrorResponse"];
            };
        };
    };
    parameters: {
        SessionId: components["schemas"]["SessionId"];
        /** @description Repeating a request with the same key within 24 h returns the original result. */
        IdempotencyKey: string;
    };
    requestBodies: never;
    headers: never;
    pathItems: never;
};
export type $defs = Record<string, never>;
export interface operations {
    listSessions: {
        parameters: {
            query?: {
                limit?: number;
                cursor?: string;
                state?: components["schemas"]["SessionState"];
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description OK */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SessionList"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    createSession: {
        parameters: {
            query?: never;
            header?: {
                /** @description Repeating a request with the same key within 24 h returns the original result. */
                "Idempotency-Key"?: components["parameters"]["IdempotencyKey"];
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateSessionRequest"];
            };
        };
        responses: {
            /** @description Created */
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Session"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    getSession: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description OK */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Session"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    deleteSession: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            default: components["responses"]["Error"];
        };
    };
    sendMessage: {
        parameters: {
            query?: never;
            header?: {
                /** @description Repeating a request with the same key within 24 h returns the original result. */
                "Idempotency-Key"?: components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["MessageRequest"];
            };
        };
        responses: {
            /** @description Accepted */
            202: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MessageAccepted"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    streamEvents: {
        parameters: {
            query?: {
                after?: number;
                follow?: boolean;
            };
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Event stream */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/event-stream": components["schemas"]["Event"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    cancelTurn: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description OK */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Session"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    endSession: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description OK */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Session"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    listFiles: {
        parameters: {
            query?: {
                path?: string;
                recursive?: boolean;
            };
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description OK */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["FileList"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    downloadFile: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                /** @description Absolute guest path, URL-encoded, e.g. `%2Fworkspace%2Fsrc%2Fmain.rs`. */
                path: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description File bytes */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/octet-stream": string;
                };
            };
            default: components["responses"]["Error"];
        };
    };
    uploadFile: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                /** @description Absolute guest path, URL-encoded, e.g. `%2Fworkspace%2Fsrc%2Fmain.rs`. */
                path: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/octet-stream": string;
            };
        };
        responses: {
            /** @description OK */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["FileEntry"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    persistArtifact: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PersistRequest"];
            };
        };
        responses: {
            /** @description Created */
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Artifact"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    listArtifacts: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description OK */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ArtifactList"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    getArtifact: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                name: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description OK */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Artifact"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
}
