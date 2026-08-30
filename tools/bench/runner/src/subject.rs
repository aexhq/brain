//! Subject manifests: what a thing is, what it can answer, and what it needs to run.
//!
//! JSON rather than TOML so the runner needs no parser the workspace does not already
//! carry, and so a manifest sits next to `contracts/` in the same format.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::schema::{Class, Evidence, LimitSource, Probe, ResidentKind};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subject {
    pub name: String,
    pub class: Class,
    /// A pinned ref — commit, image tag, or SDK version. A moving target is not a subject.
    pub version: String,
    #[serde(default)]
    pub notes: Vec<String>,

    /// What must exist before this subject can run at all. Missing requirements produce a
    /// recorded skip with the reason, never a blank row.
    #[serde(default)]
    pub requires: Requires,

    /// How the runner brings the subject up, when it can. Absent for hosted services.
    #[serde(default)]
    pub launch: Option<Launch>,

    /// The probes this subject answers, and how each is defined in its own terms.
    pub probes: BTreeMap<String, ProbeSpec>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Requires {
    /// Environment variables that must be set — API keys, account ids.
    #[serde(default)]
    pub env: Vec<String>,
    /// Needs a `.metal` instance. EC2 offers nested KVM nowhere else, so Firecracker,
    /// forkd, and self-hosted E2B cannot run on ordinary capacity.
    #[serde(default)]
    pub metal: bool,
    /// Costs money per run. Never scheduled by CI.
    #[serde(default)]
    pub paid: bool,
    #[serde(default)]
    pub linux: bool,
}

/// How the runner starts a subject.
///
/// Every string field may interpolate `{port}`, `{model_base_url}`,
/// `{environment_base_url}` and `{data_dir}`. The port is chosen per run so two subjects
/// never collide, and the data directory is created empty and removed afterwards — a
/// subject inheriting a previous run's session log would carry state into both its memory
/// and its latency numbers, and would carry a different amount of it than the next
/// subject does.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Launch {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Where the driver talks to the subject once it is up.
    pub base_url: String,
    /// Polled until it answers, before any probe starts.
    pub ready_url: String,
    #[serde(default = "default_ready_timeout")]
    pub ready_timeout_secs: u64,
}

fn default_ready_timeout() -> u64 {
    60
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeSpec {
    /// Exactly what this probe measures for this subject. Required. This is the field
    /// that lets a Brain row and a Daytona row share a table honestly.
    pub definition: String,
    #[serde(default)]
    pub variants: Vec<String>,
    /// Set when the subject cannot be pointed at the scripted provider — no BYOK, or a
    /// fixed hosted model. Such a number carries real model latency and is kept out of
    /// engine comparisons.
    #[serde(default)]
    pub model_included: bool,
    #[serde(default)]
    pub limit_source: Option<LimitSource>,
    #[serde(default)]
    pub resident_kind: Option<ResidentKind>,
    /// Defaults to `measured`. A manifest may carry a published figure instead, which is
    /// then rendered with its grade rather than pretending we ran it.
    #[serde(default)]
    pub cited: Option<Cited>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cited {
    pub value: f64,
    pub unit: String,
    pub evidence: Evidence,
    pub source: String,
}

impl Subject {
    pub fn probe(&self, probe: Probe) -> Option<&ProbeSpec> {
        self.probes.get(probe.as_str())
    }

    /// Why this subject cannot be measured here, if it cannot.
    pub fn blocked(&self, metal: bool, allow_paid: bool) -> Option<String> {
        // "A moving target is not a subject", and a placeholder is the most moving target
        // there is. Nine manifests shipped saying `PIN A RELEASE`, and without this a run
        // would compare Brain against whatever those projects happened to publish that
        // morning and label the result with a version string that says nothing.
        let version = self.version.trim();
        if version.is_empty() || version.eq_ignore_ascii_case("PIN A RELEASE") {
            return Some(format!(
                "version is {:?}: pin a commit, image tag or release before measuring it",
                self.version
            ));
        }
        if self.requires.linux && !cfg!(target_os = "linux") {
            return Some("needs Linux".to_owned());
        }
        if self.requires.metal && !metal {
            return Some(
                "needs a .metal instance for nested KVM; ordinary EC2 capacity cannot run it"
                    .to_owned(),
            );
        }
        if self.requires.paid && !allow_paid {
            return Some("costs money to run; re-run with --allow-paid".to_owned());
        }
        let missing: Vec<&str> = self
            .requires
            .env
            .iter()
            .filter(|key| std::env::var(key).is_err())
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            return Some(format!("missing credentials: {}", missing.join(", ")));
        }
        None
    }
}

/// Loads every `subject.json` under `dir`.
pub fn load_all(dir: &Path) -> Result<Vec<Subject>> {
    let mut subjects = Vec::new();
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading subjects from {}", dir.display()))?;
    for entry in entries.flatten() {
        let manifest = entry.path().join("subject.json");
        if !manifest.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest)
            .with_context(|| format!("reading {}", manifest.display()))?;
        let subject: Subject = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", manifest.display()))?;
        for (name, spec) in &subject.probes {
            anyhow::ensure!(
                !spec.definition.trim().is_empty(),
                "{}: probe {name} has no definition; a number without one cannot be \
                 compared to anything",
                subject.name
            );
        }
        subjects.push(subject);
    }
    subjects.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(subjects)
}
