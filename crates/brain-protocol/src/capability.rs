//! The typed vocabulary between a Tool and the Environment that hosts it.
//!
//! Each capability is one request/response schema pair in `contracts/environment/v2`,
//! shared by every environment that provides it and every tool that requires it. All of
//! them resolve to the same [`Outcome`] envelope, so Brain journals, bounds, and cancels
//! every capability call the same way.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

/// The closed set of capability names. Growth is a contract release, not an author
/// convention.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Exec,
    Fs,
    Net,
    Js,
    Page,
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Capability::Exec => "exec",
            Capability::Fs => "fs",
            Capability::Net => "net",
            Capability::Js => "js",
            Capability::Page => "page",
        })
    }
}

/// The one envelope every capability call and tool invocation resolves to.
///
/// `timeout` is distinguished from `error` because the deadline is caller-owned: no
/// backend family can be trusted to enforce one remotely, so the caller kills and says
/// exactly what happened rather than encoding it as an exit code.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Ok { value: serde_json::Value },
    Error { error: OutcomeError },
    Timeout,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Per-capability grant policies, supplied at attach. Enforcement is environment-side;
/// a tool cannot observe or bypass policy, only hit it as an error.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrantSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec: Option<ExecGrant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fs: Option<FsGrant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net: Option<NetGrant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub js: Option<JsGrant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageGrant>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecGrant {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms_max: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_bytes_max: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsGrant {
    pub root: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetGrant {
    /// Host patterns the environment allows outbound fetches to.
    pub allow: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsGrant {}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageGrant {}

/// `exec`: run a shell string, buffered and bounded.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRequest {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecResult {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

/// `fs`: read/write/list under the granted workspace root; bytes travel as base64.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FsRequest {
    Read { path: String },
    Write { path: String, data: String },
    List { path: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FsResult {
    Read { data: String },
    Write {},
    List { entries: Vec<FsEntry> },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsEntry {
    pub name: String,
    pub kind: FsEntryKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FsEntryKind {
    File,
    Dir,
}

/// `net`: one outbound HTTP fetch under the granted allowlist.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetResult {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

/// `js`: evaluate a source string in the environment's realm. The result is an
/// arbitrary JSON value.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsRequest {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<serde_json::Value>>,
    pub timeout_ms: u64,
}

/// `page`: the non-eval browser residue — compositor and trusted-input work that
/// `js` cannot express, plus the pull-based console side channel.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PageRequest {
    Navigate {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wait: Option<PageWait>,
    },
    Screenshot {},
    Input {
        action: serde_json::Value,
    },
    ConsoleSince {
        cursor: u64,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PageWait {
    None,
    Interaction,
    Complete,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PageResult {
    Navigate {},
    /// A PNG, base64-encoded.
    Screenshot {
        data: String,
    },
    Input {},
    ConsoleSince {
        entries: Vec<ConsoleEntry>,
        cursor: u64,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleEntry {
    pub level: String,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outcome_carries_its_status_on_the_wire() {
        let ok = serde_json::to_value(&Outcome::Ok {
            value: serde_json::json!({"exit_code": 0}),
        })
        .unwrap();
        assert_eq!(ok["status"], "ok");
        let timeout: Outcome =
            serde_json::from_value(serde_json::json!({"status":"timeout"})).unwrap();
        assert_eq!(timeout, Outcome::Timeout);
    }

    #[test]
    fn a_capability_name_is_its_wire_form() {
        for capability in [
            Capability::Exec,
            Capability::Fs,
            Capability::Net,
            Capability::Js,
            Capability::Page,
        ] {
            let wire = serde_json::to_value(capability).unwrap();
            assert_eq!(wire, serde_json::json!(capability.to_string()));
        }
    }
}
