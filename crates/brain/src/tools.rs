//! Tool resolution and the brain-side tools.
//!
//! The seven hand tools come verbatim from the sealed ABI manifest (`aex-contracts`); their
//! schemas are rendered to the model exactly as the hand serves them, so the manifest digest
//! the brain seals at create is the digest the hand must answer in `hello` (I1).
//!
//! Brain-side tools (`todo` in M0) run in-process. `task` subagents, MCP and the managed web
//! tools are M1: asking for them at create is a typed rejection, never a silent no-op.

use crate::config::{ToolDecl, ToolRoute};
use crate::{BrainError, Result};
use aex_contracts::session::BuiltinTool;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// The default tool set when `tools` is omitted at create. The contract's full default adds
/// `task`; that lands with subagents in M1 and is documented as such.
pub fn default_builtins() -> Vec<BuiltinTool> {
    vec![
        BuiltinTool::Bash,
        BuiltinTool::Read,
        BuiltinTool::Write,
        BuiltinTool::Edit,
        BuiltinTool::Glob,
        BuiltinTool::Grep,
        BuiltinTool::Ls,
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
/// is cache-visible). Unavailable tools are refused at create, loudly.
pub fn resolve(builtins: &[BuiltinTool]) -> Result<Vec<ToolDecl>> {
    let manifest = aex_contracts::tools::manifest_v1();
    let mut decls = Vec::with_capacity(builtins.len());
    for b in builtins {
        let name = builtin_name(b);
        match b {
            BuiltinTool::Task | BuiltinTool::WebSearch | BuiltinTool::WebFetch => {
                return Err(BrainError::Invalid(format!(
                    "tool {name} is not available yet (M1); remove it from tools.builtin"
                )));
            }
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
                "bash", "read", "write", "edit", "glob", "grep", "ls", "todo"
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
    }

    #[test]
    fn m1_tools_are_refused_loudly() {
        for t in [
            BuiltinTool::Task,
            BuiltinTool::WebSearch,
            BuiltinTool::WebFetch,
        ] {
            assert!(matches!(resolve(&[t]), Err(BrainError::Invalid(_))));
        }
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
