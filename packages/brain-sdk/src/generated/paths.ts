/* eslint-disable */
/** Generated from Brain-owned v1 contracts. Do not edit. */
export type paths = {
    "/v1/agentloops": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["admitAgentloop"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/agentloops/{identity}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getAgentloop"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["listSessions"];
        put?: never;
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
        get: operations["getSession"];
        put?: never;
        post?: never;
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
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["sendMessage"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/environments/{environment_id}/calls/{name}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["callEnvironment"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/serve": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** @description The serve feed: an SSE stream of this session's client-hosted `tool_intent` and `tool_cancel_intent` records, filtered to the tools named in `tools`, plus `session_ended`. It opens with the still-pending backlog (intents with no recorded result) and then carries records as they are appended. Authorized by the session's share key as a bearer token (the API token also works). One live consumer per tool: a new connection claiming a tool displaces the stream that held it, so a reconnecting client replaces its own dead connection instead of racing it. */
        get: operations["serveSessionTools"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/tool-results/{operation_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** @description Answers a client-hosted tool call. The `tool_intent` record on the event feed carries the operation id; the body is the call's outcome. Idempotent per operation: a retry with the same key replays the first answer, and a call that is no longer pending is a conflict. Authorized by the API token or by the session's share key. */
        post: operations["resolveToolCall"];
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
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["cancelSession"];
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
            path?: never;
            cookie?: never;
        };
        get: operations["readSessionEvents"];
        put?: never;
        post?: never;
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
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["endSession"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/health/live": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["live"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/health/ready": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["ready"];
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
        Identifier: string;
        ApiError: {
            code: components["schemas"]["Identifier"];
            message: string;
            retryable: boolean;
            details?: unknown;
        };
        Identity: string;
        AgentloopAdmission: {
            identity: components["schemas"]["Identity"];
            /** @enum {unknown} */
            status: "admitted" | "rejected";
            error?: components["schemas"]["ApiError"];
        };
        SessionId: string;
        ShareKey: string;
        Session: {
            session_id: components["schemas"]["SessionId"];
            journal_id: components["schemas"]["Identifier"];
            /** @enum {unknown} */
            status: "creating" | "idle" | "running" | "ended" | "failed";
            last_sequence: number;
            config_hash: components["schemas"]["Identity"];
            share_key: components["schemas"]["ShareKey"];
        };
        SessionList: {
            sessions: components["schemas"]["Session"][];
        };
        AgentloopRef: {
            identity: components["schemas"]["Identity"];
            configuration: unknown;
        };
        ModelSelection: {
            provider: components["schemas"]["Identifier"];
            name: string;
            api_key: string;
        };
        /** @enum {unknown} */
        CapabilityName: "exec" | "fs" | "net" | "js" | "page";
        ToolPayload: {
            /** @enum {unknown} */
            kind: "esm" | "component";
            identity: components["schemas"]["Identity"];
        };
        BoundTool: {
            name: components["schemas"]["Identifier"];
            description: string;
            input_schema: Record<string, never>;
            output_schema?: Record<string, never>;
            requires: components["schemas"]["CapabilityName"][];
            binding_names: components["schemas"]["Identifier"][];
            /** @enum {unknown} */
            hosting?: "provisioned" | "client";
            payload?: components["schemas"]["ToolPayload"];
            environment_id?: components["schemas"]["Identifier"];
        } & (unknown & unknown);
        ExecGrant: {
            timeout_ms_max?: number;
            output_bytes_max?: number;
        };
        FsGrant: {
            root: string;
        };
        NetGrant: {
            allow: string[];
        };
        JsGrant: Record<string, never>;
        PageGrant: Record<string, never>;
        GrantSet: {
            exec?: components["schemas"]["ExecGrant"];
            fs?: components["schemas"]["FsGrant"];
            net?: components["schemas"]["NetGrant"];
            js?: components["schemas"]["JsGrant"];
            page?: components["schemas"]["PageGrant"];
        };
        EnvironmentRequirement: {
            environment_id: components["schemas"]["Identifier"];
            configuration: unknown;
            /** @enum {unknown} */
            lifecycle_policy: "session" | "shared" | "external";
            grants?: components["schemas"]["GrantSet"];
            bindings?: {
                [key: string]: string;
            };
        };
        HistoryEvent: {
            sequence: number;
            recorded_at_ms?: number;
            event_type: components["schemas"]["Identifier"];
            data: unknown;
        };
        CreateSessionRequest: {
            agentloop: components["schemas"]["AgentloopRef"];
            model: components["schemas"]["ModelSelection"];
            system?: string;
            tools: components["schemas"]["BoundTool"][];
            response_format?: unknown;
            environments: components["schemas"]["EnvironmentRequirement"][];
            history?: components["schemas"]["HistoryEvent"][];
        };
        UserInput: {
            message: string;
        };
        MessageRequest: {
            input: components["schemas"]["UserInput"];
        };
        EnvironmentCallRequest: {
            input: unknown;
        };
        EnvironmentCallResult: {
            output: unknown;
        };
        OperationId: string;
        OutcomeError: {
            code: components["schemas"]["Identifier"];
            message: string;
            details?: unknown;
        };
        Outcome: {
            /** @constant */
            status: "ok";
            value: unknown;
        } | {
            /** @constant */
            status: "error";
            error: components["schemas"]["OutcomeError"];
        } | {
            /** @constant */
            status: "timeout";
        } | {
            /** @constant */
            status: "cancelled";
        };
        Event: {
            event_id: components["schemas"]["Identifier"];
            sequence: number;
            recorded_at_ms: number;
            event_type: components["schemas"]["Identifier"];
            data: unknown;
        };
        EventPage: {
            events: components["schemas"]["Event"][];
            next_cursor: number;
        };
    };
    responses: {
        /** @description Structured error */
        Error: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/json": components["schemas"]["ApiError"];
            };
        };
    };
    parameters: {
        IdempotencyKey: string;
        SessionId: components["schemas"]["SessionId"];
    };
    requestBodies: never;
    headers: never;
    pathItems: never;
};
export type $defs = Record<string, never>;
export interface operations {
    admitAgentloop: {
        parameters: {
            query?: never;
            header: {
                "Idempotency-Key": components["parameters"]["IdempotencyKey"];
            };
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/octet-stream": string;
            };
        };
        responses: {
            /** @description Agentloop admitted */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AgentloopAdmission"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    getAgentloop: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                identity: components["schemas"]["Identity"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Admission status */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AgentloopAdmission"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    listSessions: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Sessions */
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
            header: {
                "Idempotency-Key": components["parameters"]["IdempotencyKey"];
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
            /** @description Created session */
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
            /** @description Session state */
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
            header: {
                "Idempotency-Key": components["parameters"]["IdempotencyKey"];
            };
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
            header: {
                "Idempotency-Key": components["parameters"]["IdempotencyKey"];
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
            /** @description Updated session */
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
    callEnvironment: {
        parameters: {
            query?: never;
            header: {
                "Idempotency-Key": components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
                environment_id: components["schemas"]["Identifier"];
                name: components["schemas"]["Identifier"];
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["EnvironmentCallRequest"];
            };
        };
        responses: {
            /** @description Environment method result */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["EnvironmentCallResult"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    serveSessionTools: {
        parameters: {
            query: {
                /** @description Comma-separated client-hosted tool names this connection serves. */
                tools: string;
                /** @description Resume cursor. Absent, the stream opens with the pending backlog; set, it replays every matching record after this sequence instead. */
                after?: number;
            };
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Live serve stream */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/event-stream": string;
                };
            };
            default: components["responses"]["Error"];
        };
    };
    resolveToolCall: {
        parameters: {
            query?: never;
            header: {
                "Idempotency-Key": components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
                operation_id: components["schemas"]["OperationId"];
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["Outcome"];
            };
        };
        responses: {
            /** @description Outcome recorded */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            default: components["responses"]["Error"];
        };
    };
    cancelSession: {
        parameters: {
            query?: never;
            header: {
                "Idempotency-Key": components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Cancellation requested */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            default: components["responses"]["Error"];
        };
    };
    readSessionEvents: {
        parameters: {
            query?: {
                after?: number;
            };
            header?: never;
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description A finite event page for application/json, or a live SSE stream for text/event-stream. The stream begins with the page `after` names and then carries records as they are appended, so a client that opens it before sending a message sees that turn. It ends if the subscriber falls too far behind: reconnect with `after` set to the last id seen, and the journal hands back exactly what was missed. */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["EventPage"];
                    "text/event-stream": string;
                };
            };
            default: components["responses"]["Error"];
        };
    };
    endSession: {
        parameters: {
            query?: never;
            header: {
                "Idempotency-Key": components["parameters"]["IdempotencyKey"];
            };
            path: {
                session_id: components["parameters"]["SessionId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Ended session */
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
    live: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Process is live */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
    ready: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Process is ready */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description Required dependency is unavailable */
            503: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
        };
    };
}
