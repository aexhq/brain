//! The vocabulary between a Tool and the Environment that executes it.
//!
//! An environment is a place that executes programs. It declares which program
//! [`Runtime`]s it can launch and which [`Resources`] a program finds there. A tool
//! declares its program and the resource names it needs. Brain checks the tool's
//! needs against the environment's declaration at session create, journals every
//! call, and never wraps the platform: inside the environment a program reaches
//! its resources through the platform's own APIs, and policy is enforced at the
//! platform boundary, not by Brain.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

/// The closed set of program kinds an environment can launch. Closed only because
/// Brain and the SDK must physically package and start the program.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    /// A self-contained JavaScript module (`brain build` output).
    Esm,
    /// A POSIX shell script.
    Shell,
    /// A request to an endpoint the environment fronts.
    Http,
}

impl fmt::Display for Runtime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Runtime::Esm => "esm",
            Runtime::Shell => "shell",
            Runtime::Http => "http",
        })
    }
}

/// The resources an environment declares, keyed by name. The contract fixes the
/// policy shape of the named resources (`fs`, `process`, `net`, `dom`, `secrets`);
/// vendor resources are namespaced (`aws:iam`) and opaque. Brain compares names
/// only and never interprets the policy blocks.
pub type Resources = BTreeMap<String, serde_json::Value>;

/// Whether `value` is a resource name the contract admits: a lowercase word,
/// optionally namespaced with one colon (`fs`, `bin:ffmpeg`).
pub fn resource_name_valid(value: &str) -> bool {
    let (head, tail) = match value.split_once(':') {
        Some((head, tail)) => (head, Some(tail)),
        None => (value, None),
    };
    let head_valid = !head.is_empty()
        && head.len() <= 64
        && head.bytes().enumerate().all(|(index, byte)| {
            (index == 0 && byte.is_ascii_lowercase())
                || (index > 0
                    && (byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'))
        });
    let tail_valid = tail.is_none_or(|tail| {
        !tail.is_empty()
            && tail.len() <= 64
            && tail
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    });
    head_valid && tail_valid
}

/// The one envelope every tool invocation resolves to.
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
    fn a_runtime_is_its_wire_form() {
        for runtime in [Runtime::Esm, Runtime::Shell, Runtime::Http] {
            let wire = serde_json::to_value(runtime).unwrap();
            assert_eq!(wire, serde_json::json!(runtime.to_string()));
        }
    }

    #[test]
    fn resource_names_are_words_with_one_optional_namespace() {
        for valid in [
            "fs",
            "process",
            "net",
            "dom",
            "secrets",
            "bin:ffmpeg",
            "aws:iam",
            "chrome:cdp-1.3",
        ] {
            assert!(resource_name_valid(valid), "{valid}");
        }
        for invalid in [
            "", "Fs", "fs:", ":x", "a:b:c", "../fs", "fs bin", "1fs", "-fs",
        ] {
            assert!(!resource_name_valid(invalid), "{invalid}");
        }
    }
}
