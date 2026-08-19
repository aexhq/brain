//! Tool resolution and the brain-side tools.
//!
//! The seven hand tools come verbatim from the sealed ABI manifest (`aex-contracts`); their
//! schemas are rendered to the model exactly as the hand serves them, so the manifest digest
//! the brain seals at create is the digest the hand must answer in `hello` (I1).
//!
//! Brain-side tools (`todo`, and since slice 8 `task`) run in-process. Managed web tools run
//! through the guarded outbound seam and are available only when explicitly sealed.

use crate::config::{ToolDecl, ToolRoute};
use crate::{BrainError, Result};
use aex_contracts::session::BuiltinTool;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// The default tool set when `tools` is omitted at create: all hand tools + `task` + `todo`,
/// exactly as the contract documents.
pub fn default_builtins() -> Vec<BuiltinTool> {
    vec![
        BuiltinTool::Bash,
        BuiltinTool::Read,
        BuiltinTool::Write,
        BuiltinTool::Edit,
        BuiltinTool::Glob,
        BuiltinTool::Grep,
        BuiltinTool::Ls,
        BuiltinTool::Task,
        BuiltinTool::Todo,
    ]
}

fn builtin_name(t: &BuiltinTool) -> &'static str {
    match t {
        BuiltinTool::Bash => "bash",
        BuiltinTool::Read => "read",
        BuiltinTool::Write => "write",
        BuiltinTool::Edit => "edit",
        BuiltinTool::Glob => "glob",
        BuiltinTool::Grep => "grep",
        BuiltinTool::Ls => "ls",
        BuiltinTool::Task => "task",
        BuiltinTool::Todo => "todo",
        BuiltinTool::WebSearch => "web_search",
        BuiltinTool::WebFetch => "web_fetch",
    }
}

pub fn parse_builtin(name: &str) -> Option<BuiltinTool> {
    Some(match name {
        "bash" => BuiltinTool::Bash,
        "read" => BuiltinTool::Read,
        "write" => BuiltinTool::Write,
        "edit" => BuiltinTool::Edit,
        "glob" => BuiltinTool::Glob,
        "grep" => BuiltinTool::Grep,
        "ls" => BuiltinTool::Ls,
        "task" => BuiltinTool::Task,
        "todo" => BuiltinTool::Todo,
        "web_search" => BuiltinTool::WebSearch,
        "web_fetch" => BuiltinTool::WebFetch,
        _ => return None,
    })
}

/// Resolves the declared builtin tools into sealed `ToolDecl`s, in declaration order (order
/// is cache-visible).
pub fn resolve(builtins: &[BuiltinTool]) -> Result<Vec<ToolDecl>> {
    let manifest = aex_contracts::tools::manifest_v1();
    let mut decls = Vec::with_capacity(builtins.len());
    for b in builtins {
        let name = builtin_name(b);
        match b {
            BuiltinTool::WebSearch => decls.push(web_search_decl()),
            BuiltinTool::WebFetch => decls.push(web_fetch_decl()),
            BuiltinTool::Task => decls.push(task_decl()),
            BuiltinTool::Todo => decls.push(todo_decl()),
            _ => {
                let spec = manifest
                    .tools
                    .iter()
                    .find(|t| *t.name == name)
                    .ok_or_else(|| BrainError::UndeclaredTool { name: name.into() })?;
                decls.push(ToolDecl {
                    name: name.into(),
                    description: spec.description.clone(),
                    input_schema: serde_json::Value::Object(spec.input_schema.clone()),
                    route: ToolRoute::Hand,
                });
            }
        }
    }
    Ok(decls)
}

fn web_search_decl() -> ToolDecl {
    ToolDecl {
        name: "web_search".into(),
        description: "Search the public web using the managed search service. Returns ordered titles, URLs, snippets, and optional dates. Each successful call is billed at the published per-query rate.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type":"string", "minLength":1, "maxLength":500},
                "num": {"type":"integer", "minimum":1, "maximum":10, "default":5},
                "country": {"type":"string", "minLength":2, "maxLength":8},
                "language": {"type":"string", "minLength":2, "maxLength":16}
            },
            "required": ["query"],
            "additionalProperties": false
        }),
        route: ToolRoute::Web,
    }
}

fn web_fetch_decl() -> ToolDecl {
    ToolDecl {
        name: "web_fetch".into(),
        description: "Fetch one public HTTPS page through the SSRF guard and return readable text. Redirects are revalidated at every hop; private, local, metadata, and non-web destinations are refused.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "url": {"type":"string", "format":"uri", "minLength":1},
                "max_chars": {"type":"integer", "minimum":1, "maximum":50000, "default":50000}
            },
            "required": ["url"],
            "additionalProperties": false
        }),
        route: ToolRoute::Web,
    }
}

/// Names of the resolved tools, for the HEAD prefix doc.
pub fn names(decls: &[ToolDecl]) -> Vec<String> {
    decls.iter().map(|d| d.name.clone()).collect()
}

/// The digest the brain seals and sends in `hello`. Any hand that cannot serve exactly this
/// manifest fails the session (`tool_manifest_mismatch`).
pub fn manifest_digest() -> String {
    aex_contracts::tools::TOOL_MANIFEST_V1_DIGEST
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------------------------
// task -- brain-side: a self-similar child agent inside the parent's turn (slice-8 spec)
// ---------------------------------------------------------------------------------------------

/// The model-supplied input of one `task` call.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
    /// Short label for events and dashboards; never sent to the child.
    pub description: String,
    /// The child's seed user message.
    pub prompt: String,
}

pub fn task_decl() -> ToolDecl {
    ToolDecl {
        name: "task".into(),
        description: concat!(
            "Delegate a self-contained piece of work to a subagent. The subagent has the ",
            "same tools and workspace as you (except task-list state), works autonomously ",
            "from your prompt alone, and returns only its final report -- so the prompt ",
            "must carry every detail it needs."
        )
        .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "description": {"type": "string", "minLength": 1, "maxLength": 100,
                                "description": "A short (3-7 word) label for this delegation."},
                "prompt": {"type": "string", "minLength": 1,
                           "description": "The complete, self-contained task for the subagent."}
            },
            "required": ["description", "prompt"],
            "additionalProperties": false
        }),
        route: ToolRoute::Brain,
    }
}

// ---------------------------------------------------------------------------------------------
// todo -- brain-side, no side effects outside the session
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TodoInput {
    items: Vec<TodoItem>,
}

fn todo_decl() -> ToolDecl {
    ToolDecl {
        name: "todo".into(),
        description: "Replace the session's todo list. Send the complete list every time; \
                      the response echoes the stored list."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {"type": "string", "minLength": 1},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                        },
                        "required": ["content", "status"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["items"],
            "additionalProperties": false
        }),
        route: ToolRoute::Brain,
    }
}

/// The per-session todo state. Ephemeral by design: it is model working memory, not data.
#[derive(Debug, Default)]
pub struct TodoState {
    items: Mutex<Vec<TodoItem>>,
}

impl TodoState {
    /// Executes one `todo` call. Returns (content, is_error).
    pub fn execute(&self, input: &serde_json::Value) -> (String, bool) {
        match serde_json::from_value::<TodoInput>(input.clone()) {
            Ok(t) => {
                let mut items = self.items.lock().expect("todo lock");
                *items = t.items;
                let body = serde_json::json!({ "items": &*items });
                (body.to_string(), false)
            }
            Err(e) => (format!("invalid todo input: {e}"), true),
        }
    }
}

/// The per-call metadata the dispatcher carries; also what error text the model sees when a
/// tool cannot run at all.
pub fn undeclared(name: &str) -> String {
    format!("tool {name} is not declared in this session's sealed tool set")
}

/// Session-scoped mint for operation/batch ids: unique per session for the life of the
/// process, unique across rehydrations by the turn prefix.
#[derive(Debug, Default)]
pub struct Mint(std::sync::atomic::AtomicU64);

impl Mint {
    pub fn next(&self, prefix: &str) -> String {
        let n = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{prefix}_{n:08x}")
    }
}

/// Placeholder to keep `HashMap` import honest for env carriage helpers used by hand hello.
pub type Env = HashMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_serves_manifest_schemas_verbatim_in_order() {
        let decls = resolve(&default_builtins()).unwrap();
        assert_eq!(
            names(&decls),
            vec![
                "bash", "read", "write", "edit", "glob", "grep", "ls", "task", "todo"
            ]
        );
        let manifest = aex_contracts::tools::manifest_v1();
        let bash = manifest.tools.iter().find(|t| &**t.name == "bash").unwrap();
        assert_eq!(
            decls[0].input_schema,
            serde_json::Value::Object(bash.input_schema.clone()),
            "hand tool schemas must render exactly as the manifest serves them (I1)"
        );
        assert!(matches!(decls[0].route, ToolRoute::Hand));
        assert!(matches!(decls[7].route, ToolRoute::Brain));
        assert!(matches!(decls[8].route, ToolRoute::Brain));
    }

    #[test]
    fn managed_web_tools_have_sealed_schemas_and_routes() {
        let tools = resolve(&[BuiltinTool::WebSearch, BuiltinTool::WebFetch]).unwrap();
        assert_eq!(names(&tools), ["web_search", "web_fetch"]);
        assert!(tools.iter().all(|tool| tool.route == ToolRoute::Web));
    }

    #[test]
    fn todo_replaces_and_echoes() {
        let s = TodoState::default();
        let (out, err) = s.execute(&serde_json::json!({
            "items": [{"content": "write tests", "status": "in_progress"}]
        }));
        assert!(!err);
        assert!(out.contains("write tests"));
        let (out, err) = s.execute(&serde_json::json!({"bogus": true}));
        assert!(err, "invalid input is an error result, not a panic: {out}");
    }

    #[test]
    fn manifest_digest_matches_the_pin() {
        let m = aex_contracts::tools::manifest_v1();
        assert_eq!(*aex_contracts::tools::manifest_digest(m), manifest_digest());
    }
}
