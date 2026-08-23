/* eslint-disable */
/** GENERATED from Brain-owned contracts/session/v1. DO NOT EDIT. */
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
        /** Create a session (seals model, prompt, tools, and execution policy) */
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
        /** Start irreversible recursive deletion of the session tree */
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
         *     `GET /events?after=<seq-1>`. 409 `session_busy` while a turn is running. Host products may
         *     implement higher-level result semantics with ordinary sealed server tools.
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
        /** Durably fence the session subtree and accept asynchronous teardown */
        post: operations["endSession"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/deletion": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        /** Read the durable recursive-deletion tombstone or retry state */
        get: operations["getDeletionStatus"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        get: operations["getDefaultSandbox"];
        put?: never;
        /** Idempotently materialize the root default sandbox */
        post: operations["createDefaultSandbox"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox/files/list": {
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
        post: operations["listSandboxFiles"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox/files/stat": {
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
        post: operations["statSandboxFile"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox/files/read-inline": {
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
        post: operations["readSandboxFileInline"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox/files/write-inline": {
        parameters: {
            query?: never;
            header: {
                /** @description Required exact-replay identity for an effectful operation. */
                "Idempotency-Key": components["parameters"]["RequiredIdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["writeSandboxFileInline"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox/files/downloads": {
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
         * Prepare a short-lived generation-fenced direct download
         * @description Process-local happy-path convenience. Brain does not automatically retry or recover this transfer after restart, expiry, missing state, or an ambiguous response. Prepare a fresh transfer after inspecting the file; use session storage plus copy for recovery-safe bytes.
         */
        post: operations["prepareSandboxFileDownload"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox/files/uploads": {
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
         * Prepare a short-lived generation-fenced direct upload
         * @description Process-local happy-path convenience. Brain does not automatically retry or recover this transfer after restart, expiry, missing state, or an ambiguous response. Prepare a fresh transfer after inspecting the file; use session storage plus copy for recovery-safe bytes.
         */
        post: operations["prepareSandboxFileUpload"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox/files/uploads/{transfer_id}/complete": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                transfer_id: components["parameters"]["TransferId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * Import one prepared upload into its originally fenced generation and path
         * @description Completion is not automatically retried. Unknown, expired, or ambiguous process-local state returns a typed error and requires file inspection plus a fresh prepare.
         */
        post: operations["completeSandboxFileUpload"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox/files/find": {
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
        post: operations["findSandboxFiles"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/sandbox/files/grep": {
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
        post: operations["grepSandboxFiles"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/children": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        get: operations["listChildren"];
        put?: never;
        post: operations["createChild"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/children/{child_id}": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        get: operations["getChild"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/children/{child_id}/messages": {
        parameters: {
            query?: never;
            header?: {
                /** @description Repeating the same logical request reuses its durable operation identity. */
                "Idempotency-Key"?: components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["sendChildMessage"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/children/{child_id}/follow-up": {
        parameters: {
            query?: never;
            header?: {
                /** @description Repeating the same logical request reuses its durable operation identity. */
                "Idempotency-Key"?: components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["followUpChild"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/children/{child_id}/wait": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["waitForChild"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/children/{child_id}/interrupt": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["interruptChild"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/children/{child_id}/end": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Durably fence the child subtree and accept asynchronous teardown */
        post: operations["endChild"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/list": {
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
        post: operations["listStorage"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/stat": {
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
        post: operations["statStorageObject"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/read-inline": {
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
        post: operations["readStorageInline"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/write-inline": {
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
        post: operations["writeStorageInline"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/downloads": {
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
        post: operations["prepareStorageDownload"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/uploads": {
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
        post: operations["prepareStorageUpload"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/uploads/{transfer_id}/complete": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                transfer_id: components["parameters"]["TransferId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["completeStorageUpload"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/delete": {
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
        post: operations["deleteStorageObject"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/copy-from-sandbox": {
        parameters: {
            query?: never;
            header: {
                /** @description Required exact-replay identity for an effectful operation. */
                "Idempotency-Key": components["parameters"]["RequiredIdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["copyFromSandbox"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/storage/copy-to-sandbox": {
        parameters: {
            query?: never;
            header: {
                /** @description Required exact-replay identity for an effectful operation. */
                "Idempotency-Key": components["parameters"]["RequiredIdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["copyToSandbox"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/customer-hand/grants": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["createCustomerHandGrant"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/customer-hand/socket": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["connectCustomerHand"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/customer-hand/observations/{grant_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["observeCustomerHandOperation"];
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
        DeletionStatus: {
            /** @constant */
            object: "session.deletion";
            session_id: components["schemas"]["SessionId"];
            /** @enum {string} */
            state: "accepted" | "deleting" | "retrying" | "blocked" | "succeeded";
            requested_at_ms: number;
            updated_at_ms: number;
            completed_at_ms: number | null;
        };
        SandboxFilePathRequest: {
            path: string;
            generation: string;
        };
        SandboxFileListRequest: components["schemas"]["SandboxFilePathRequest"] & {
            cursor?: string;
            /** @default 100 */
            limit?: number;
        };
        SandboxFileReadRequest: components["schemas"]["SandboxFilePathRequest"] & {
            /** @default 1048576 */
            max_bytes?: number;
        };
        SandboxFileWriteRequest: {
            path: string;
            generation: string;
            content_base64: string;
            /** @default false */
            overwrite?: boolean;
        };
        SandboxFileUploadRequest: {
            path: string;
            generation: string;
            bytes: number;
            sha256: string;
            /** @default false */
            overwrite?: boolean;
        };
        SandboxFileFindRequest: {
            path: string;
            generation: string;
            glob: string;
            cursor?: string;
            /** @default 100 */
            limit?: number;
        };
        SandboxFileGrepRequest: {
            path: string;
            generation: string;
            query: string;
            cursor?: string;
            /** @default 100 */
            limit?: number;
        };
        SandboxFileList: {
            data: components["schemas"]["FileEntry"][];
            has_more: boolean;
            next_cursor?: string;
            generation: string;
        };
        SandboxFileContent: {
            entry: components["schemas"]["FileEntry"];
            content_base64: string;
        };
        CreateChildRequest: {
            prompt: string;
            name?: string;
            fork_turns?: ("all" | "none") | string;
        };
        ChildMessageRequest: {
            message: string;
        };
        WaitChildRequest: {
            /** @default 30000 */
            timeout_ms?: number;
        };
        StorageListRequest: {
            prefix?: string;
            cursor?: string;
            /** @default 100 */
            limit?: number;
        };
        StorageKeyRequest: {
            key: string;
        };
        StorageReadRequest: {
            key: string;
            /** @default 1048576 */
            max_bytes?: number;
        };
        StorageWriteRequest: {
            key: string;
            content_base64: string;
            content_type?: string;
            /** @default false */
            overwrite?: boolean;
        };
        StorageUploadRequest: {
            key: string;
            bytes: number;
            sha256: string;
            content_type?: string;
            /** @default false */
            overwrite?: boolean;
        };
        StorageSandboxCopyRequest: {
            key: string;
            path: string;
            sandbox_generation: string;
            /** @default false */
            overwrite?: boolean;
        };
        StorageObject: {
            key: string;
            bytes: number;
            sha256: string;
            content_type?: string;
            /** Format: date-time */
            created_at: string;
            /** Format: date-time */
            updated_at: string;
        };
        StorageList: {
            data: components["schemas"]["StorageObject"][];
            has_more: boolean;
            next_cursor?: string;
        };
        StorageContent: {
            object: components["schemas"]["StorageObject"];
            content_base64: string;
        };
        StorageTransfer: {
            transfer_id: string;
            /** @enum {string} */
            method: "GET" | "PUT";
            url: string;
            headers: {
                [key: string]: string;
            };
            /** Format: date-time */
            expires_at: string;
            max_bytes: number;
        };
        CustomerGrantRequest: {
            client_id: string;
        };
        CustomerGrant: {
            url: string;
            protocol: string;
            /** Format: date-time */
            expires_at: string;
            grant_id: string;
            observation_url: string;
            observation_token: string;
        };
        CustomerObservation: {
            /** @constant */
            type: "receipt";
            epoch: number;
            operation_id: string;
            request_digest: string;
            replayed: boolean;
        } | {
            /** @constant */
            type: "terminal";
            epoch: number;
            operation_id: string;
            request_digest: string;
            ok: boolean;
            output?: unknown;
            error?: string;
        };
        /**
         * @description Lifecycle only. Whether a turn is running is reported separately as turn_state.
         * @enum {string}
         */
        SessionState: "open" | "ending" | "ended" | "deleting" | "deleted" | "failed";
        /** @description Stable machine-readable code. Brain defines its core codes; a host executor may return its own code without teaching Brain product semantics. */
        ApiErrorCode: string;
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
        /** @description Immutable pointer to the exact bounded parent model projection inherited at child admission. It never embeds parent prompt bytes. */
        ContextFork: {
            source_session_id: components["schemas"]["SessionId"];
            source_context_generation: number;
            source_through_sequence: number;
            /** @enum {string} */
            mode: "all" | "none" | "last_n";
            last_n?: number;
            resolved_turns: number;
            source_projection_digest: string;
        };
        /**
         * @description Current-turn activity, independent from session lifecycle.
         * @enum {string}
         */
        SessionTurnState: "idle" | "running";
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
            /** @description Effective immutable context window used for request admission and semantic compaction. */
            context_window_tokens: number;
        };
        /** @description Billed storage, visible from day one. */
        StorageInfo: {
            /** @description Durable objects scoped to the session. */
            session_storage_bytes: number;
            /** @description Outstanding staged upload bytes held against the sealed session quota until staging cleanup completes. These bytes are not yet published session objects. */
            upload_reserved_bytes: number;
        };
        /**
         * Format: date-time
         * @description RFC 3339, UTC.
         */
        Timestamp: string;
        TurnId: string;
        SessionFailure: {
            /** @enum {string} */
            code: "binding_conflict" | "provider_unusable" | "hand_unavailable" | "internal";
            message: string;
            at: components["schemas"]["Timestamp"];
        };
        Sha256Hex: string;
        /** @description The sealed agentloop identity of a session. */
        AgentloopInfo: {
            /** @constant */
            kind: "official";
            name: string;
            version: string;
        } | {
            /** @constant */
            kind: "custom";
            source_bundle_sha256: components["schemas"]["Sha256Hex"];
            toolchain: string;
        };
        Session: {
            id: components["schemas"]["SessionId"];
            parent_id?: components["schemas"]["SessionId"];
            /** @description Optional customer-visible task name for a child session. */
            name?: string;
            context_fork?: components["schemas"]["ContextFork"];
            root_id: components["schemas"]["SessionId"];
            depth: number;
            /** @description Authoritative durable journal high-water mark used for tenant discovery and delta folding. */
            last_seq: number;
            /** @enum {string} */
            object: "session";
            state: components["schemas"]["SessionState"];
            turn_state: components["schemas"]["SessionTurnState"];
            /** @description Stable recovery/dispatch phase when a turn is running. Absent while idle. */
            turn_phase?: string;
            /**
             * @description Authoritative immutable execution shape inherited by every child. The hosted alpha supports only 1gb.
             * @constant
             */
            shape: "1gb";
            model: components["schemas"]["ModelInfo"];
            storage: components["schemas"]["StorageInfo"];
            created_at: components["schemas"]["Timestamp"];
            updated_at: components["schemas"]["Timestamp"];
            last_message_at?: components["schemas"]["Timestamp"];
            turns: number;
            current_turn?: components["schemas"]["TurnId"];
            failure?: components["schemas"]["SessionFailure"];
            agentloop?: components["schemas"]["AgentloopInfo"];
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
            /** @description Immutable model context window. Omission seals the conservative neutral default of 32768 tokens; custom model names are never guessed from a mutable catalog. */
            context_window_tokens?: number;
            temperature?: number;
            /**
             * @description Sealed into supported OpenAI-family Chat profiles. The Anthropic MVP profile rejects this field before any external effect instead of silently dropping it.
             * @enum {string}
             */
            reasoning_effort?: "low" | "medium" | "high";
        };
        ToolName: string;
        /** @description The model-visible half of one Tool. Array order is preserved exactly in the immutable model prefix. */
        ToolDefinition: {
            name: components["schemas"]["ToolName"];
            description?: string;
            input_schema: {
                [key: string]: unknown;
            };
            output_schema?: {
                [key: string]: unknown;
            };
            contract_digest: components["schemas"]["Sha256Hex"];
        };
        ToolExecutor: {
            /** @constant */
            kind: "aex_managed";
            bundle_digest: components["schemas"]["Sha256Hex"];
            /** @description Environment-key names only. Secret values never enter the seal. */
            required_env: string[];
        } | {
            /** @constant */
            kind: "customer_app";
            registration: string;
        } | {
            /** @constant */
            kind: "engine";
            capability: string;
        };
        NetworkDestination: {
            host: string;
            ports: [
                443
            ];
            /** @constant */
            protocol: "tls";
        } | {
            cidr: string;
            ports: number[];
            /** @constant */
            protocol: "tcp";
        };
        ToolConfig: {
            definition: components["schemas"]["ToolDefinition"];
            executor: components["schemas"]["ToolExecutor"];
            /** @description The tool's declared outbound needs. Merged at create: effective allowlist = (union of tool declarations and session allows) minus session denies; Aex infra is always denied. Declaration and merge only - no per-tool runtime isolation is claimed. */
            network?: {
                destinations: components["schemas"]["NetworkDestination"][];
            };
        };
        /** @description Sealed at create with the rest of the prefix. Omitted tools default to an empty set. */
        ToolsConfig: {
            /** @description The exact ordered native Tool grant. Omitted or empty means no native tools. */
            items?: components["schemas"]["ToolConfig"][];
        };
        /** @description Create-time-only bundle bytes. Brain stages these outside the journal, then discards this representation. */
        ToolBundle: {
            checksum: components["schemas"]["Sha256Hex"];
            content_base64: string;
            bytes: number;
            /** @constant */
            media_type: "application/javascript+esm";
        };
        /** @description Immutable direct outbound ceiling. Omission means none. */
        NetworkPolicy: {
            /** @constant */
            outbound: "none";
            /** @description Hosts the session explicitly refuses (exact, or "*.suffix"). Subtracted from the merged allowlist at create; incompatible with outbound "public" (nothing enforces a deny off the gateway path). */
            deny?: string[];
        } | {
            /** @constant */
            outbound: "public";
            /** @description Hosts the session explicitly refuses (exact, or "*.suffix"). Subtracted from the merged allowlist at create; incompatible with outbound "public" (nothing enforces a deny off the gateway path). */
            deny?: string[];
        } | {
            /** @constant */
            outbound: "allowlist";
            destinations: components["schemas"]["NetworkDestination"][];
            /** @description Hosts the session explicitly refuses (exact, or "*.suffix"). Subtracted from the merged allowlist at create; incompatible with outbound "public" (nothing enforces a deny off the gateway path). */
            deny?: string[];
        };
        CustomerClientConfig: {
            id: string;
            /** @default 1 */
            submit_retries?: number;
        };
        /** @description Which agentloop drives this session's turns. Sealed at create for the life of the session; children inherit the parent's loop. Omission seals the official aex loop. */
        AgentloopConfig: string | {
            source_bundle_sha256: components["schemas"]["Sha256Hex"];
            /** @description The pinned loop-toolchain identity the bundle was built for. */
            toolchain: string;
            /** @description The deterministic esbuild source bundle, base64 (8 MiB decoded maximum). Create-time-only: staged outside the journal, never part of the model prefix. */
            bundle_base64: string;
        };
        ChildLimits: {
            /** @default 4 */
            max_depth?: number;
            /** @default 32 */
            max_direct_children?: number;
            /** @default 256 */
            max_descendants?: number;
        };
        /** @description Everything here except metadata is part of the immutable prefix: it cannot change for the life of the session. */
        CreateSessionRequest: {
            model: components["schemas"]["ModelConfig"];
            system_prompt?: string;
            tools?: components["schemas"]["ToolsConfig"];
            /** @description Bounded bundle payloads referenced by tools.items. Never part of the model prefix or journal. */
            tool_bundles?: components["schemas"]["ToolBundle"][];
            /** @description Write-only values for required managed Tool environment names; encrypted in custody. */
            secrets?: {
                [key: string]: string;
            };
            network?: components["schemas"]["NetworkPolicy"];
            /** @default 1 */
            provider_recovery_retries?: number;
            client?: components["schemas"]["CustomerClientConfig"];
            agentloop?: components["schemas"]["AgentloopConfig"];
            children?: components["schemas"]["ChildLimits"];
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
        MessageRequest: {
            content: string | components["schemas"]["ContentPart"][];
            metadata?: {
                [key: string]: string;
            };
        };
        /** @description The turn was admitted and journaled. Follow it on GET /events?after=<seq-1>. */
        MessageAccepted: {
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            /** @description Journal sequence of the turn.started event. */
            seq: number;
        };
        /** @description "root" for the session's root agent; subagents get brain-minted ids. */
        AgentId: string;
        /** @description Brain-minted identity of one provisional provider attempt. */
        ModelAttemptId: string;
        /** @description Brain-minted id of one durable Tool operation. Managed Hands receive the same operation_id. */
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
        StopReason: "end_turn" | "refusal" | "max_rounds" | "cancelled" | "error";
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
            attempt_id: components["schemas"]["ModelAttemptId"];
            /**
             * @description Deltas are provisional until the matching assistant.message wins.
             * @constant
             */
            provisional: true;
            text: string;
        } | {
            /** @enum {string} */
            type: "assistant.message";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            agent_id: components["schemas"]["AgentId"];
            attempt_id: components["schemas"]["ModelAttemptId"];
            /** @description The complete assistant text of one model round. */
            text: string;
        } | {
            /** @enum {string} */
            type: "replay.complete";
            session_id: components["schemas"]["SessionId"];
            /** @description Strong durable HEAD high-water captured after subscription and reached by every replay page before this proof was emitted. This control event has no SSE id and is never journaled. */
            through_seq: number;
        } | {
            /** @enum {string} */
            type: "model.attempt_superseded";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            logical_operation_id: string;
            superseded_attempt_id: components["schemas"]["ModelAttemptId"];
            replacement_attempt_id: components["schemas"]["ModelAttemptId"];
            /** @enum {string} */
            reason: "unknown";
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
            type: "storage.usage";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            storage: components["schemas"]["StorageInfo"];
        } | {
            /** @enum {string} */
            type: "session.updated";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id?: components["schemas"]["TurnId"];
            state: components["schemas"]["SessionState"];
            turn_state: components["schemas"]["SessionTurnState"];
            turn_phase?: string;
        } | {
            /** @enum {string} */
            type: "hand.lost";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id?: components["schemas"]["TurnId"];
            /** @description Calls whose outcome is unknown; they are reported to the model as interrupted and never replayed. */
            interrupted_calls: components["schemas"]["CallId"][];
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
        } | {
            /** @enum {string} */
            type: "loop.event";
            seq: number;
            at: components["schemas"]["Timestamp"];
            session_id: components["schemas"]["SessionId"];
            turn_id: components["schemas"]["TurnId"];
            /** @description Loop-chosen event name. */
            name: string;
            /** @description The loop-authored event payload, journaled as a loop `event` entry before it is delivered. */
            data: {
                [key: string]: unknown;
            };
        };
        /** @enum {string} */
        TargetKind: "default" | "additional";
        Identifier: string;
        SandboxTarget: {
            kind: components["schemas"]["TargetKind"];
            session_id: components["schemas"]["Identifier"];
            root_id: components["schemas"]["Identifier"];
            binding_ref: components["schemas"]["Identifier"];
            sandbox_id?: components["schemas"]["Identifier"] | null;
        };
        /** @enum {string} */
        SandboxState: "never_materialized" | "creating" | "running" | "suspended" | "gone" | "terminated";
        SandboxStatus: {
            target: components["schemas"]["SandboxTarget"];
            state: components["schemas"]["SandboxState"];
            target_ref?: components["schemas"]["Identifier"] | null;
            generation?: string | null;
            reason?: string | null;
            changed_at_ms?: number | null;
            expires_at_ms: number | null;
        };
        Digest: string;
        FileEntry: {
            path: string;
            /** @enum {string} */
            kind: "file" | "directory" | "symlink";
            bytes: number;
            sha256?: components["schemas"]["Digest"] | null;
            modified_at_ms: number;
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
        /** @description Ordinary Session projection */
        Session: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["Session"];
            };
        };
        /** @description Ordinary Session page */
        SessionList: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["SessionList"];
            };
        };
        /** @description Turn admitted */
        MessageAccepted: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["MessageAccepted"];
            };
        };
        /** @description Durable deletion status */
        DeletionStatus: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["DeletionStatus"];
            };
        };
        /** @description Generation-fenced default sandbox lifecycle */
        SandboxStatus: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["SandboxStatus"];
            };
        };
        /** @description Sandbox file metadata */
        SandboxFile: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["FileEntry"];
            };
        };
        /** @description Bounded inline sandbox file */
        SandboxFileContent: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["SandboxFileContent"];
            };
        };
        /** @description Sandbox file page */
        SandboxFileList: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["SandboxFileList"];
            };
        };
        /** @description Durable session-storage object */
        StorageObject: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["StorageObject"];
            };
        };
        /** @description Durable storage page */
        StorageList: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["StorageList"];
            };
        };
        /** @description Bounded inline durable object */
        StorageContent: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["StorageContent"];
            };
        };
        /** @description Short-lived one-purpose transfer authority */
        StorageTransfer: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["StorageTransfer"];
            };
        };
        /** @description Customer-Hand WebSocket and observation grants */
        CustomerGrant: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["CustomerGrant"];
            };
        };
    };
    parameters: {
        SessionId: components["schemas"]["SessionId"];
        /** @description Repeating the same logical request reuses its durable operation identity. */
        IdempotencyKey: string;
        /** @description Required exact-replay identity for an effectful operation. */
        RequiredIdempotencyKey: string;
        ChildId: components["schemas"]["SessionId"];
        TransferId: string;
        GrantId: string;
    };
    requestBodies: {
        SandboxFilePath: {
            content: {
                "application/json": components["schemas"]["SandboxFilePathRequest"];
            };
        };
        SandboxFileList: {
            content: {
                "application/json": components["schemas"]["SandboxFileListRequest"];
            };
        };
        SandboxFileRead: {
            content: {
                "application/json": components["schemas"]["SandboxFileReadRequest"];
            };
        };
        SandboxFileWrite: {
            content: {
                "application/json": components["schemas"]["SandboxFileWriteRequest"];
            };
        };
        SandboxFileUpload: {
            content: {
                "application/json": components["schemas"]["SandboxFileUploadRequest"];
            };
        };
        SandboxFileFind: {
            content: {
                "application/json": components["schemas"]["SandboxFileFindRequest"];
            };
        };
        SandboxFileGrep: {
            content: {
                "application/json": components["schemas"]["SandboxFileGrepRequest"];
            };
        };
        CreateChild: {
            content: {
                "application/json": components["schemas"]["CreateChildRequest"];
            };
        };
        ChildMessage: {
            content: {
                "application/json": components["schemas"]["ChildMessageRequest"];
            };
        };
        WaitChild: {
            content: {
                "application/json": components["schemas"]["WaitChildRequest"];
            };
        };
        StorageList: {
            content: {
                "application/json": components["schemas"]["StorageListRequest"];
            };
        };
        StorageKey: {
            content: {
                "application/json": components["schemas"]["StorageKeyRequest"];
            };
        };
        StorageRead: {
            content: {
                "application/json": components["schemas"]["StorageReadRequest"];
            };
        };
        StorageWrite: {
            content: {
                "application/json": components["schemas"]["StorageWriteRequest"];
            };
        };
        StorageUpload: {
            content: {
                "application/json": components["schemas"]["StorageUploadRequest"];
            };
        };
        StorageSandboxCopy: {
            content: {
                "application/json": components["schemas"]["StorageSandboxCopyRequest"];
            };
        };
        CustomerGrant: {
            content: {
                "application/json": components["schemas"]["CustomerGrantRequest"];
            };
        };
        CustomerObservation: {
            content: {
                "application/json": components["schemas"]["CustomerObservation"];
            };
        };
    };
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
                /** @description Repeating the same logical request reuses its durable operation identity. */
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
            /** @description Deletion accepted; poll the Location response header */
            202: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description An earlier request already completed physical deletion */
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
                /** @description Repeating the same logical request reuses its durable operation identity. */
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
                /** @description Exact strong high-water for a finite replay; valid only with follow=false */
                through?: number;
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
            /** @description The subtree admission fence is durable; the returned Session is ending */
            202: {
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
    getDeletionStatus: {
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
            200: components["responses"]["DeletionStatus"];
            default: components["responses"]["Error"];
        };
    };
    getDefaultSandbox: {
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
            200: components["responses"]["SandboxStatus"];
            default: components["responses"]["Error"];
        };
    };
    createDefaultSandbox: {
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
            200: components["responses"]["SandboxStatus"];
            default: components["responses"]["Error"];
        };
    };
    listSandboxFiles: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["SandboxFileList"];
        responses: {
            200: components["responses"]["SandboxFileList"];
            default: components["responses"]["Error"];
        };
    };
    statSandboxFile: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["SandboxFilePath"];
        responses: {
            200: components["responses"]["SandboxFile"];
            default: components["responses"]["Error"];
        };
    };
    readSandboxFileInline: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["SandboxFileRead"];
        responses: {
            200: components["responses"]["SandboxFileContent"];
            default: components["responses"]["Error"];
        };
    };
    writeSandboxFileInline: {
        parameters: {
            query?: never;
            header: {
                /** @description Required exact-replay identity for an effectful operation. */
                "Idempotency-Key": components["parameters"]["RequiredIdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["SandboxFileWrite"];
        responses: {
            200: components["responses"]["SandboxFile"];
            default: components["responses"]["Error"];
        };
    };
    prepareSandboxFileDownload: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["SandboxFilePath"];
        responses: {
            200: components["responses"]["StorageTransfer"];
            default: components["responses"]["Error"];
        };
    };
    prepareSandboxFileUpload: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["SandboxFileUpload"];
        responses: {
            200: components["responses"]["StorageTransfer"];
            default: components["responses"]["Error"];
        };
    };
    completeSandboxFileUpload: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                transfer_id: components["parameters"]["TransferId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: components["responses"]["SandboxFile"];
            default: components["responses"]["Error"];
        };
    };
    findSandboxFiles: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["SandboxFileFind"];
        responses: {
            200: components["responses"]["SandboxFileList"];
            default: components["responses"]["Error"];
        };
    };
    grepSandboxFiles: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["SandboxFileGrep"];
        responses: {
            200: components["responses"]["SandboxFileList"];
            default: components["responses"]["Error"];
        };
    };
    listChildren: {
        parameters: {
            query?: {
                cursor?: string;
                limit?: number;
            };
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: components["responses"]["SessionList"];
            default: components["responses"]["Error"];
        };
    };
    createChild: {
        parameters: {
            query?: never;
            header?: {
                /** @description Repeating the same logical request reuses its durable operation identity. */
                "Idempotency-Key"?: components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["CreateChild"];
        responses: {
            201: components["responses"]["Session"];
            default: components["responses"]["Error"];
        };
    };
    getChild: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: components["responses"]["Session"];
            default: components["responses"]["Error"];
        };
    };
    sendChildMessage: {
        parameters: {
            query?: never;
            header?: {
                /** @description Repeating the same logical request reuses its durable operation identity. */
                "Idempotency-Key"?: components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["ChildMessage"];
        responses: {
            202: components["responses"]["MessageAccepted"];
            default: components["responses"]["Error"];
        };
    };
    followUpChild: {
        parameters: {
            query?: never;
            header?: {
                /** @description Repeating the same logical request reuses its durable operation identity. */
                "Idempotency-Key"?: components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["ChildMessage"];
        responses: {
            200: components["responses"]["Session"];
            default: components["responses"]["Error"];
        };
    };
    waitForChild: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["WaitChild"];
        responses: {
            200: components["responses"]["Session"];
            default: components["responses"]["Error"];
        };
    };
    interruptChild: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: components["responses"]["Session"];
            default: components["responses"]["Error"];
        };
    };
    endChild: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                child_id: components["parameters"]["ChildId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            202: components["responses"]["Session"];
            default: components["responses"]["Error"];
        };
    };
    listStorage: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["StorageList"];
        responses: {
            200: components["responses"]["StorageList"];
            default: components["responses"]["Error"];
        };
    };
    statStorageObject: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["StorageKey"];
        responses: {
            200: components["responses"]["StorageObject"];
            default: components["responses"]["Error"];
        };
    };
    readStorageInline: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["StorageRead"];
        responses: {
            200: components["responses"]["StorageContent"];
            default: components["responses"]["Error"];
        };
    };
    writeStorageInline: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["StorageWrite"];
        responses: {
            200: components["responses"]["StorageObject"];
            default: components["responses"]["Error"];
        };
    };
    prepareStorageDownload: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["StorageKey"];
        responses: {
            200: components["responses"]["StorageTransfer"];
            default: components["responses"]["Error"];
        };
    };
    prepareStorageUpload: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["StorageUpload"];
        responses: {
            200: components["responses"]["StorageTransfer"];
            default: components["responses"]["Error"];
        };
    };
    completeStorageUpload: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
                transfer_id: components["parameters"]["TransferId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: components["responses"]["StorageObject"];
            default: components["responses"]["Error"];
        };
    };
    deleteStorageObject: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["StorageKey"];
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
    copyFromSandbox: {
        parameters: {
            query?: never;
            header: {
                /** @description Required exact-replay identity for an effectful operation. */
                "Idempotency-Key": components["parameters"]["RequiredIdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["StorageSandboxCopy"];
        responses: {
            200: components["responses"]["StorageObject"];
            default: components["responses"]["Error"];
        };
    };
    copyToSandbox: {
        parameters: {
            query?: never;
            header: {
                /** @description Required exact-replay identity for an effectful operation. */
                "Idempotency-Key": components["parameters"]["RequiredIdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["StorageSandboxCopy"];
        responses: {
            200: components["responses"]["SandboxFile"];
            default: components["responses"]["Error"];
        };
    };
    createCustomerHandGrant: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: components["requestBodies"]["CustomerGrant"];
        responses: {
            200: components["responses"]["CustomerGrant"];
            default: components["responses"]["Error"];
        };
    };
    connectCustomerHand: {
        parameters: {
            query?: never;
            header: {
                "Sec-WebSocket-Protocol": string;
            };
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description WebSocket upgraded */
            101: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            default: components["responses"]["Error"];
        };
    };
    observeCustomerHandOperation: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                grant_id: components["parameters"]["GrantId"];
            };
            cookie?: never;
        };
        requestBody: components["requestBodies"]["CustomerObservation"];
        responses: {
            /** @description Observation applied */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            default: components["responses"]["Error"];
        };
    };
}
