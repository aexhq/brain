//! Contract declarations for the Go HTTP implementation. These functions return
//! route metadata; they are not Rust HTTP handlers. Go checks its registered
//! routes against the rendered document at startup and in its journey suite.

use crate::openapi::Security;
use serde_json::Value;
use utoipa::OpenApi;

mod contract {
    use schemars::JsonSchema;
    use std::borrow::Cow;
    use utoipa::{
        PartialSchema, ToSchema,
        openapi::{Ref, RefOr, Schema},
    };

    macro_rules! reference {
        ($($name:ident),* $(,)?) => {$(
            pub(super) struct $name;
            impl PartialSchema for $name {
                fn schema() -> RefOr<Schema> {
                    RefOr::Ref(Ref::from_schema_name(
                        brain_protocol::session_v2::$name::schema_name()
                    ))
                }
            }
            impl ToSchema for $name {
                fn name() -> Cow<'static, str> {
                    brain_protocol::session_v2::$name::schema_name()
                }
            }
        )*};
    }
    reference!(
        SessionCreate,
        SessionView,
        SessionPage,
        Input,
        CommandReceipt,
        RecordPage,
        Record,
        Run,
        ChildResults,
        Operation,
        Claim,
        Execution,
        Receipt,
        Retry,
        InteractionCreate,
        Interaction,
        InteractionAnswer,
        ConnectionAttach,
        Connection,
        DocumentUpdate,
        Document,
        FileInfo,
        ScheduleCreate,
        Schedule,
        Delegation,
        Handoff,
        NodeInfo,
        TransferCreate,
        Transfer,
        ParticipantGrant,
        ParticipantToken,
        PackageAdmission,
        Package,
        CatalogEntry,
        SessionBudget,
        StorageReport,
        ExportManifest
    );
}

macro_rules! read {
    ($name:ident, $method:ident, $path:literal, $response:ty) => {
        #[utoipa::path($method, path = $path,
                                                    responses((status = 200, body = $response)),
                                                    security(("serverToken" = [])))]
        fn $name() -> (&'static str, &'static str) {
            (stringify!($method), $path)
        }
    };
}
macro_rules! paged {
    ($name:ident, $method:ident, $path:literal, $response:ty) => {
        #[utoipa::path($method, path = $path,
            params(
                ("after" = Option<String>, Query, description = "Exclusive last id (key for documents, name for files). Repeat until the returned collection is empty; byte bounds can shorten a page."),
                ("limit" = Option<u32>, Query, minimum = 1, maximum = 128)
            ),
            responses((status = 200, body = $response)),
            security(("serverToken" = [])))]
        fn $name() -> (&'static str, &'static str) {
            (stringify!($method), $path)
        }
    };
}
macro_rules! write {
    ($name:ident, $method:ident, $path:literal, $request:ty, $response:ty) => {
        #[utoipa::path($method, path = $path, request_body = $request,
                                                    responses((status = 200, body = $response)),
                                                    security(("serverToken" = [])))]
        fn $name() -> (&'static str, &'static str) {
            (stringify!($method), $path)
        }
    };
}

write!(
    create,
    post,
    "/v2/sessions",
    contract::SessionCreate,
    contract::SessionView
);
paged!(list, get, "/v2/sessions", contract::SessionPage);
read!(
    session,
    get,
    "/v2/sessions/{session}",
    contract::SessionView
);
read!(
    delete,
    delete,
    "/v2/sessions/{session}",
    contract::SessionView
);
write!(
    input,
    post,
    "/v2/sessions/{session}/inputs",
    contract::Input,
    contract::CommandReceipt
);
read!(
    command,
    get,
    "/v2/sessions/{session}/commands/{command}",
    contract::CommandReceipt
);
read!(
    records,
    get,
    "/v2/sessions/{session}/records",
    contract::RecordPage
);
#[utoipa::path(
    get,
    path = "/internal/v2/records/{session}/{sequence}",
    description = "Node-local history routing. Requires a fresh cluster HMAC signature; public bearer tokens do not authorize this route.",
    params(
        ("session" = String, Path),
        ("sequence" = String, Path),
        ("digest" = String, Query),
        ("X-Brain-Request-Time" = String, Header),
        ("X-Brain-Node-Signature" = String, Header)
    ),
    responses((status = 200, body = contract::Record))
)]
fn internal_record() -> (&'static str, &'static str) {
    ("get", "/internal/v2/records/{session}/{sequence}")
}
read!(stream, get, "/v2/sessions/{session}/events", String);
read!(
    budget,
    get,
    "/v2/sessions/{session}/budget",
    contract::SessionBudget
);
paged!(runs, get, "/v2/sessions/{session}/runs", Vec<contract::Run>);
#[utoipa::path(
    get,
    path = "/v2/sessions/{session}/runs/{run}/children",
    params(("step" = u32, Query, description = "The saved delegation decision's zero-based step.")),
    responses((status = 200, body = contract::ChildResults)),
    security(("serverToken" = []))
)]
fn children() -> (&'static str, &'static str) {
    ("get", "/v2/sessions/{session}/runs/{run}/children")
}
#[utoipa::path(
    get,
    path = "/v2/sessions/{session}/runs/{run}/children/{child}/result",
    params(
        ("step" = u32, Query, description = "The saved delegation decision's zero-based step."),
        ("child" = u32, Path, minimum = 0, maximum = 7)
    ),
    responses((status = 200, body = contract::Record)),
    security(("serverToken" = []))
)]
fn child_result() -> (&'static str, &'static str) {
    (
        "get",
        "/v2/sessions/{session}/runs/{run}/children/{child}/result",
    )
}
paged!(
    operations,
    get,
    "/v2/sessions/{session}/operations",
    Vec<contract::Operation>
);
read!(
    operation,
    get,
    "/v2/sessions/{session}/operations/{operation}",
    contract::Operation
);
write!(
    claim,
    post,
    "/v2/sessions/{session}/operations/{operation}/claim",
    contract::Claim,
    contract::Execution
);
write!(
    receipt,
    post,
    "/v2/sessions/{session}/operations/{operation}/receipt",
    contract::Receipt,
    contract::Operation
);
write!(
    retry,
    post,
    "/v2/sessions/{session}/operations/{operation}/retry",
    contract::Retry,
    contract::CommandReceipt
);
paged!(
    interactions,
    get,
    "/v2/sessions/{session}/interactions",
    Vec<contract::Interaction>
);
write!(
    interact,
    post,
    "/v2/sessions/{session}/interactions",
    contract::InteractionCreate,
    contract::Interaction
);
write!(
    answer,
    post,
    "/v2/sessions/{session}/interactions/{interaction}",
    contract::InteractionAnswer,
    contract::Interaction
);
paged!(
    connections,
    get,
    "/v2/sessions/{session}/connections",
    Vec<contract::Connection>
);
write!(
    attach,
    post,
    "/v2/sessions/{session}/connections",
    contract::ConnectionAttach,
    contract::Connection
);
read!(
    revoke,
    delete,
    "/v2/sessions/{session}/connections/{connection}",
    contract::Connection
);
paged!(
    state,
    get,
    "/v2/sessions/{session}/state",
    Vec<contract::Document>
);
write!(
    state_put,
    put,
    "/v2/sessions/{session}/state/{key}",
    contract::DocumentUpdate,
    contract::Document
);
read!(
    state_delete,
    delete,
    "/v2/sessions/{session}/state/{key}",
    contract::Document
);
paged!(
    memory,
    get,
    "/v2/sessions/{session}/memory",
    Vec<contract::Document>
);
write!(
    memory_put,
    put,
    "/v2/sessions/{session}/memory/{key}",
    contract::DocumentUpdate,
    contract::Document
);
read!(
    memory_delete,
    delete,
    "/v2/sessions/{session}/memory/{key}",
    contract::Document
);
paged!(
    files,
    get,
    "/v2/sessions/{session}/files",
    Vec<contract::FileInfo>
);
write!(
    file_put,
    put,
    "/v2/sessions/{session}/files/{file}",
    Vec<u8>,
    contract::FileInfo
);
read!(
    file_get,
    get,
    "/v2/sessions/{session}/files/{file}",
    Vec<u8>
);
read!(
    file_delete,
    delete,
    "/v2/sessions/{session}/files/{file}",
    contract::FileInfo
);
paged!(
    schedules,
    get,
    "/v2/sessions/{session}/schedules",
    Vec<contract::Schedule>
);
write!(
    schedule,
    post,
    "/v2/sessions/{session}/schedules",
    contract::ScheduleCreate,
    contract::Schedule
);
read!(
    unschedule,
    delete,
    "/v2/sessions/{session}/schedules/{schedule}",
    contract::Schedule
);
write!(
    webhook,
    post,
    "/v2/sessions/{session}/webhooks",
    contract::Input,
    contract::CommandReceipt
);
write!(
    delegate,
    post,
    "/v2/sessions/{session}/delegations",
    contract::Delegation,
    contract::SessionView
);
write!(
    handoff,
    post,
    "/v2/sessions/{session}/handoff",
    contract::Handoff,
    contract::SessionView
);
write!(
    token,
    post,
    "/v2/sessions/{session}/participants",
    contract::ParticipantGrant,
    contract::ParticipantToken
);
read!(
    token_revoke,
    delete,
    "/v2/sessions/{session}/participants/{participant}",
    String
);
#[utoipa::path(
    get,
    path = "/v2/sessions/{session}/export",
    description = "Bounded manifest pages covering all named sections. Echo continuation as after until empty. This is not a stopped-writer backup or a transactional snapshot.",
    params(("after" = Option<String>, Query, description = "Opaque continuation; omit or use 0 to begin.")),
    responses((status = 200, body = contract::ExportManifest)),
    security(("serverToken" = []))
)]
fn export() -> (&'static str, &'static str) {
    ("get", "/v2/sessions/{session}/export")
}
read!(catalog, get, "/v2/catalog", Vec<contract::CatalogEntry>);
write!(
    package,
    post,
    "/v2/packages",
    contract::PackageAdmission,
    contract::Package
);
read!(
    package_get,
    get,
    "/v2/packages/{package}",
    contract::Package
);
read!(live, get, "/health/live", String);
read!(ready, get, "/health/ready", String);
read!(drain, post, "/operator/drain", contract::StorageReport);
read!(storage, get, "/operator/storage", contract::StorageReport);
paged!(nodes, get, "/operator/nodes", Vec<contract::NodeInfo>);
write!(
    transfer_create,
    post,
    "/operator/sessions/{session}/transfers",
    contract::TransferCreate,
    contract::Transfer
);
read!(
    transfer,
    get,
    "/operator/sessions/{session}/transfers/{transfer}",
    contract::Transfer
);
read!(
    transfer_cancel,
    delete,
    "/operator/sessions/{session}/transfers/{transfer}",
    contract::Transfer
);

#[derive(OpenApi)]
#[openapi(paths(
    create, list, session, delete, input, command, records, internal_record, stream, budget, runs, children, child_result, operations,
    operation, claim, receipt, retry, interactions, interact, answer, connections,
    attach, revoke, state, state_put, state_delete, memory, memory_put, memory_delete,
    files, file_put, file_get, file_delete, schedules, schedule, unschedule, webhook,
    delegate, handoff, token, token_revoke, export, catalog, package, package_get,
    live, ready, drain, storage, nodes, transfer_create, transfer, transfer_cancel
), modifiers(&Security))]
struct Api;

pub fn openapi() -> Value {
    let mut document = serde_json::to_value(Api::openapi()).expect("OpenAPI serializes");
    let mut schemas = brain_protocol::contract::session_v2()["$defs"].take();
    crate::openapi::rewrite_references(&mut schemas);
    document["components"]["schemas"] = schemas;
    let routes = [
        create(),
        list(),
        session(),
        delete(),
        input(),
        command(),
        records(),
        internal_record(),
        stream(),
        budget(),
        runs(),
        children(),
        child_result(),
        operations(),
        operation(),
        claim(),
        receipt(),
        retry(),
        interactions(),
        interact(),
        answer(),
        connections(),
        attach(),
        revoke(),
        state(),
        state_put(),
        state_delete(),
        memory(),
        memory_put(),
        memory_delete(),
        files(),
        file_put(),
        file_get(),
        file_delete(),
        schedules(),
        schedule(),
        unschedule(),
        webhook(),
        delegate(),
        handoff(),
        token(),
        token_revoke(),
        export(),
        catalog(),
        package(),
        package_get(),
        live(),
        ready(),
        drain(),
        storage(),
        nodes(),
        transfer_create(),
        transfer(),
        transfer_cancel(),
    ];
    assert_eq!(
        routes.len(),
        crate::openapi::operations(&Api::openapi()).len()
    );
    document
}
