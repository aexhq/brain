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
    "/v1/environments": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["listEnvironments"];
        put?: never;
        /** @description Creates an environment: Brain runs its setup and keeps what it declared it executes and offers. Sessions attach to it by id. A managed environment is closed by Brain once no session has been attached to it for its idle TTL; an unmanaged one lives until it is deleted. */
        post: operations["createEnvironment"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/environments/{environment_id}": {
        parameters: {
            query?: never;
            header?: never;
            path: {
                environment_id: components["schemas"]["Identifier"];
            };
            cookie?: never;
        };
        get: operations["getEnvironment"];
        put?: never;
        post?: never;
        /** @description Tears the environment down. Refused with `conflict` while a session is still attached; every session that was ever attached sees `environment_closed` on its events. */
        delete: operations["deleteEnvironment"];
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
        /** @description The serve feed: an SSE stream of this session's client-hosted `tool_call_started` and `tool_cancel_started` records, filtered to the tools named in `tools`, plus `session_ended`. It opens with the still-pending backlog (calls with no finished record) and then carries records as they are appended. Authorized by the session's share key as a bearer token (the API token also works). One live consumer per tool: a new connection claiming a tool displaces the stream that held it, so a reconnecting client replaces its own dead connection instead of racing it. */
        get: operations["serveSessionTools"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/v1/sessions/{session_id}/tool-results/{sequence}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** @description Answers a client-hosted tool call. The call is named by the sequence of its `tool_call_started` record on the event feed; the body is the call's outcome. Idempotent per call: a retry with the same key replays the first answer, and a call that is no longer pending is a conflict. Authorized by the API token or by the session's share key. */
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
        /** @enum {unknown} */
        EnvironmentStatus: "open" | "unreachable";
        SessionId: string;
        /** @enum {unknown} */
        Runtime: "esm" | "shell" | "http";
        Environment: {
            environment_id: components["schemas"]["Identifier"];
            status: components["schemas"]["EnvironmentStatus"];
            managed: boolean;
            idle_ttl_ms?: number;
            attached_sessions: components["schemas"]["SessionId"][];
            runtimes?: components["schemas"]["Runtime"][];
            resources?: Record<string, never>;
            created_at_ms: number;
        };
        EnvironmentList: {
            environments: components["schemas"]["Environment"][];
        };
        CreateEnvironmentRequest: {
            environment_id?: components["schemas"]["Identifier"];
            configuration: unknown;
            /** @description Brain closes the environment once no session has been attached to it for idle_ttl_ms. */
            managed?: boolean;
            idle_ttl_ms?: number;
        };
        ShareKey: string;
        Session: {
            session_id: components["schemas"]["SessionId"];
            /** @enum {unknown} */
            status: "creating" | "idle" | "running" | "ended" | "failed";
            last_sequence: number;
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
        ResourceName: string;
        HttpProgramRequest: {
            method: string;
            url: string;
            headers?: {
                [key: string]: string;
            };
        };
        Program: {
            /** @constant */
            kind: "esm";
            identity: components["schemas"]["Identity"];
        } | {
            /** @constant */
            kind: "shell";
            identity: components["schemas"]["Identity"];
            script: string;
        } | {
            /** @constant */
            kind: "http";
            identity: components["schemas"]["Identity"];
            request: components["schemas"]["HttpProgramRequest"];
        };
        BoundTool: {
            name: components["schemas"]["Identifier"];
            description: string;
            input_schema: Record<string, never>;
            output_schema?: Record<string, never>;
            needs: components["schemas"]["ResourceName"][];
            binding_names: components["schemas"]["Identifier"][];
            /** @enum {unknown} */
            hosting?: "provisioned" | "client";
            program?: components["schemas"]["Program"];
            environment_id?: components["schemas"]["Identifier"];
        } & (unknown & unknown);
        EnvironmentAttachRequest: {
            environment_id: components["schemas"]["Identifier"];
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
            response_format?: unknown;
            tools: components["schemas"]["BoundTool"][];
            environments: components["schemas"]["EnvironmentAttachRequest"][];
            history?: components["schemas"]["HistoryEvent"][];
            /** @description How long the session may sit idle before Brain suspends its task and memory to disk. Absent means the server default; zero means never. */
            idle_ttl_ms?: number;
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
        Sequence: number;
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
    listEnvironments: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Environments */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["EnvironmentList"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    createEnvironment: {
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
                "application/json": components["schemas"]["CreateEnvironmentRequest"];
            };
        };
        responses: {
            /** @description Created environment */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Environment"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    getEnvironment: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                environment_id: components["schemas"]["Identifier"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Environment state */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Environment"];
                };
            };
            default: components["responses"]["Error"];
        };
    };
    deleteEnvironment: {
        parameters: {
            query?: never;
            header: {
                "Idempotency-Key": components["parameters"]["IdempotencyKey"];
            };
            path: {
                environment_id: components["schemas"]["Identifier"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Closed */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
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
                sequence: components["schemas"]["Sequence"];
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
