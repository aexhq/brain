//! Incremental results, and resume.
//!
//! Spot capacity gives two minutes of notice and then takes the instance. A run that
//! assembled its results in memory and wrote them at the end would lose an hour of
//! completed probes to that. So every record is appended and flushed as it lands, and a
//! killed run is resumed by reading back which (subject, probe, variant) tuples already
//! have a line.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::schema::{Datapoint, Host, Outcome, Probe, Record, Run, Skipped};

pub struct Writer {
    path: PathBuf,
    file: std::fs::File,
    run_id: String,
    sync: Option<String>,
}

impl Writer {
    /// Opens `dir/<run_id>.jsonl` for append and writes a header. Called again on resume,
    /// which appends a second header — deliberately, because spot may have handed back a
    /// different instance type and the results file should show both.
    pub fn open(dir: &Path, run_id: &str, host: &Host, sync: Option<String>) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating results directory {}", dir.display()))?;
        let path = dir.join(format!("{run_id}.jsonl"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let mut writer = Self {
            path,
            file,
            run_id: run_id.to_owned(),
            sync,
        };
        writer.write(&Record::Header {
            run_id: run_id.to_owned(),
            started_at_ms: now_ms(),
            host: Box::new(host.clone()),
        })?;
        Ok(writer)
    }

    fn write(&mut self, record: &Record) -> Result<()> {
        serde_json::to_writer(&mut self.file, record)?;
        self.file.write_all(b"\n")?;
        // Flushed per record: the next line may never be written.
        self.file.flush()?;
        self.ship();
        Ok(())
    }

    /// Copies the results file somewhere that outlives the instance.
    ///
    /// Flushing per record survives the *process* dying. It does not survive the *machine*
    /// going away, which on spot capacity is the likelier ending — and a run whose results
    /// were reclaimed along with the box has produced nothing. The command runs after every
    /// record; records are minutes apart, so a copy per record is not a burden.
    ///
    /// A failure here is reported and never fatal: losing the copy is bad, and losing the
    /// run while it still holds data worth shipping would be worse.
    fn ship(&self) {
        let Some(template) = &self.sync else { return };
        let command = template.replace("{file}", &self.path.display().to_string());
        let outcome = if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/C", &command])
                .output()
        } else {
            std::process::Command::new("sh")
                .args(["-c", &command])
                .output()
        };
        match outcome {
            Ok(output) if output.status.success() => {}
            Ok(output) => eprintln!(
                "results sync failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(error) => eprintln!("results sync could not run: {error}"),
        }
    }

    pub fn datapoint(&mut self, datapoint: Datapoint) -> Result<()> {
        self.write(&Record::Datapoint(Box::new(datapoint)))
    }

    pub fn skipped(&mut self, skipped: Skipped) -> Result<()> {
        self.write(&Record::Skipped(skipped))
    }

    pub fn finish(mut self, outcome: Outcome) -> Result<()> {
        self.write(&Record::Footer { outcome })
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// What a previous invocation of this run already recorded, so `--resume` does not repeat
/// it. A partially written final line is dropped rather than failing the load: it is the
/// expected shape of a file whose process was killed mid-write.
pub fn completed(dir: &Path, run_id: &str) -> Result<BTreeSet<(String, Probe, Option<String>)>> {
    let path = dir.join(format!("{run_id}.jsonl"));
    let mut done = BTreeSet::new();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(done);
    };
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<Record>(line) else {
            continue;
        };
        match record {
            Record::Datapoint(point) => {
                done.insert((point.subject, point.probe, point.variant));
            }
            Record::Skipped(skip) => {
                done.insert((skip.subject, skip.probe, None));
            }
            _ => {}
        }
    }
    Ok(done)
}

/// Reassembles a run from its lines, for table generation.
pub fn load(path: &Path) -> Result<Run> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut run = Run {
        run_id: String::new(),
        started_at_ms: 0,
        host: crate::schema::Host {
            label: "unknown".to_owned(),
            os: String::new(),
            arch: String::new(),
            kernel: None,
            cpus: 0,
            ec2: None,
            tuning: Default::default(),
        },
        // A file with no footer was killed. That is an interruption, not a completion,
        // and it must not be reported as one.
        outcome: Outcome::Interrupted,
        datapoints: Vec::new(),
        skipped: Vec::new(),
    };
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<Record>(line) else {
            continue;
        };
        match record {
            Record::Header {
                run_id,
                started_at_ms,
                host,
            } => {
                if run.run_id.is_empty() {
                    run.run_id = run_id;
                    run.started_at_ms = started_at_ms;
                }
                // The last header wins: it describes the host that produced the most
                // recent datapoints.
                run.host = *host;
            }
            Record::Datapoint(point) => run.datapoints.push(*point),
            Record::Skipped(skip) => run.skipped.push(skip),
            Record::Footer { outcome } => run.outcome = outcome,
        }
    }
    Ok(run)
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

/// A run id that sorts chronologically and survives being read back off an instance that
/// no longer exists.
pub fn new_run_id() -> String {
    format!("run-{}", now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Class, Evidence, Probe, Record, Tuning};

    fn host() -> Host {
        Host {
            label: "c7g.xlarge".to_owned(),
            os: "linux".to_owned(),
            arch: "aarch64".to_owned(),
            kernel: Some("6.8.0".to_owned()),
            cpus: 4,
            ec2: None,
            tuning: Tuning::default(),
        }
    }

    /// Every record must survive a write and a read back.
    ///
    /// `load` skips a line it cannot parse, because a run killed mid-write leaves a
    /// partial final line and that is normal. The cost of that tolerance is that a record
    /// which stops deserializing disappears in silence — which is exactly what happened
    /// with a `u128` timestamp, a type serde cannot read back inside an internally tagged
    /// enum. The header vanished and every generated table said "unknown host".
    #[test]
    fn every_record_survives_a_round_trip() {
        let point = Datapoint {
            subject: "brain".to_owned(),
            subject_version: "HEAD".to_owned(),
            class: Class::SessionKernel,
            probe: Probe::Ttfb,
            definition: "send until the first assistant delta byte".to_owned(),
            variant: None,
            evidence: Evidence::Measured,
            value: 1.4,
            unit: "ms".to_owned(),
            n: 900,
            percentiles: Default::default(),
            model_included: false,
            limit_source: None,
            resident_kind: None,
            generator_floor_ms: Some(0.038),
            // A non-empty series and a fit, because both are skipped when empty and a
            // round trip that only ever saw the empty case would not prove they survive.
            memory_kib: vec![crate::schema::Reading {
                at_ms: 250,
                value: 41_200.0,
                units: Some(64),
            }],
            fit: crate::schema::Fit::least_squares(&[
                (0.0, 40_000.0),
                (64.0, 41_400.0),
                (128.0, 42_800.0),
            ]),
            steal: Default::default(),
            notes: Vec::new(),
        };
        let records = [
            Record::Header {
                run_id: "run-1".to_owned(),
                started_at_ms: 1_787_879_221_930,
                host: Box::new(host()),
            },
            Record::Datapoint(Box::new(point)),
            Record::Skipped(Skipped {
                subject: "openfang".to_owned(),
                probe: Probe::Resident,
                reason: "needs Linux".to_owned(),
            }),
            Record::Footer {
                outcome: Outcome::Complete,
            },
        ];
        for record in &records {
            let line = serde_json::to_string(record).expect("serializes");
            serde_json::from_str::<Record>(&line)
                .unwrap_or_else(|error| panic!("{line} does not read back: {error}"));
        }
    }

    #[test]
    fn a_file_without_a_footer_is_reported_as_interrupted() {
        let dir = std::env::temp_dir().join(format!("brain-bench-test-{}", now_ms()));
        let mut writer = Writer::open(&dir, "run-x", &host(), None).expect("opens");
        writer
            .skipped(Skipped {
                subject: "brain".to_owned(),
                probe: Probe::Create,
                reason: "never became ready".to_owned(),
            })
            .expect("writes");
        drop(writer);

        let run = load(&dir.join("run-x.jsonl")).expect("loads");
        assert_eq!(
            run.run_id, "run-x",
            "the header must survive the round trip"
        );
        assert_eq!(run.host.label, "c7g.xlarge");
        assert!(run.started_at_ms > 0);
        assert_eq!(run.skipped.len(), 1);
        assert_eq!(
            run.outcome,
            Outcome::Interrupted,
            "a run that never wrote a footer was killed, and must not read as complete"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
