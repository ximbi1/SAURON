//! Durable, local, append-oriented mutation journal. Not a Kubernetes Event
//! replacement -- this is SAURON's own evidence trail. NEVER records Secret
//! data/stringData/tokens/credentials/exec stdin: only redacted summaries and
//! payload hashes. A malformed prior line must never crash SAURON. A
//! pre-commit journal-write failure must fail the mutation closed; a
//! post-commit failure must never be reported as "mutation did not happen."
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

pub const SCHEMA_VERSION: u32 = 1;
const MAX_FIELD_BYTES: usize = 2048;
const MAX_RECENT: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    IntentCreated,
    PolicyEvaluated,
    Previewed,
    PreflightStarted,
    PreflightResult,
    ConfirmationSatisfied,
    CommitStarted,
    CommitResult,
    /// M8.5: a fresh post-commit observation, correlated by `request_id` to
    /// the `CommitResult` it follows -- never implies the commit itself was
    /// re-evaluated or retried.
    VerificationResult,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub schema_version: u32,
    pub timestamp: String,
    pub request_id: u64,
    pub phase: Phase,
    pub context: String,
    pub cluster: String,
    pub resource: String,
    pub namespace: String,
    pub name: String,
    pub uid: String,
    pub effect: String,
    /// Redacted, field-path-only description -- never a value.
    pub summary: String,
    pub payload_sha256: Option<String>,
    pub policy_decision: Option<String>,
    pub policy_reasons: Vec<String>,
    pub outcome: Option<String>,
    /// Bounded, redacted detail (e.g. an HTTP status classification). Never
    /// a raw server response body.
    pub detail: Option<String>,
}
impl Record {
    fn bound(mut self) -> Self {
        let clip = |s: &mut String| {
            if s.len() > MAX_FIELD_BYTES {
                s.truncate(MAX_FIELD_BYTES);
                s.push_str("...<truncated>");
            }
        };
        clip(&mut self.summary);
        if let Some(d) = &mut self.detail {
            clip(d);
        }
        self.policy_reasons.truncate(32);
        self
    }
}

pub struct Journal {
    path: PathBuf,
}
impl Journal {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Fails loudly on any I/O or serialization error -- callers on the
    /// pre-commit path must treat this as fail-closed, never mutate on error.
    pub fn append(&self, record: Record) -> std::io::Result<()> {
        let record = record.bound();
        let line = serde_json::to_string(&record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")?;
        file.flush()
    }
    /// Bounded, deterministic (file order), tail-most-recent read. A
    /// malformed line is skipped, never a crash and never an I/O error.
    pub fn recent(&self, limit: usize) -> Vec<Record> {
        let limit = limit.min(MAX_RECENT);
        let Ok(contents) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        let mut records: Vec<Record> = contents
            .lines()
            .rev()
            .filter_map(|line| serde_json::from_str(line).ok())
            .take(limit)
            .collect();
        records.reverse();
        records
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(request_id: u64, phase: Phase) -> Record {
        Record {
            schema_version: SCHEMA_VERSION,
            timestamp: "2026-09-18T00:00:00Z".into(),
            request_id,
            phase,
            context: "kind-sauron-test".into(),
            cluster: "kind-sauron-test".into(),
            resource: "v1/configmaps".into(),
            namespace: "sauron-m7".into(),
            name: "m7-target".into(),
            uid: "uid-1".into(),
            effect: "Modify".into(),
            summary: "metadata.annotations[\"m7-proof\"]".into(),
            payload_sha256: Some("hash".into()),
            policy_decision: Some("RequireConfirmation".into()),
            policy_reasons: vec!["ConfirmationRequired".into()],
            outcome: None,
            detail: None,
        }
    }

    #[test]
    fn append_and_recent_round_trip_in_order() {
        let dir = std::env::temp_dir().join(format!("sauron-journal-test-{}", std::process::id()));
        let path = dir.join("journal.jsonl");
        let journal = Journal::new(&path);
        for (i, phase) in [
            Phase::IntentCreated,
            Phase::PolicyEvaluated,
            Phase::CommitResult,
        ]
        .into_iter()
        .enumerate()
        {
            journal.append(record(i as u64, phase)).expect("append");
        }
        let recent = journal.recent(10);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].phase, Phase::IntentCreated);
        assert_eq!(recent[2].phase, Phase::CommitResult);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_prior_line_is_skipped_not_a_crash() {
        let dir =
            std::env::temp_dir().join(format!("sauron-journal-malformed-{}", std::process::id()));
        let path = dir.join("journal.jsonl");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "not json at all\n").unwrap();
        let journal = Journal::new(&path);
        journal
            .append(record(1, Phase::IntentCreated))
            .expect("append");
        let recent = journal.recent(10);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].request_id, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_journal_file_reads_as_empty_not_an_error() {
        let dir =
            std::env::temp_dir().join(format!("sauron-journal-missing-{}", std::process::id()));
        let path = dir.join("does-not-exist.jsonl");
        let journal = Journal::new(&path);
        assert!(journal.recent(10).is_empty());
    }

    #[test]
    fn oversized_fields_are_bounded_not_unbounded() {
        let dir =
            std::env::temp_dir().join(format!("sauron-journal-bounds-{}", std::process::id()));
        let path = dir.join("journal.jsonl");
        let journal = Journal::new(&path);
        let mut r = record(1, Phase::CommitResult);
        r.summary = "x".repeat(10_000);
        journal.append(r).expect("append");
        let recent = journal.recent(1);
        assert!(recent[0].summary.len() < 10_000);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn never_carries_secret_style_fields_by_construction() {
        // The type itself has no field capable of holding raw payload/value
        // content -- summary/detail are the only free-text fields and both
        // are documented and bounded as redacted-only.
        let r = record(1, Phase::CommitResult);
        assert!(serde_json::to_value(&r).unwrap().get("data").is_none());
        assert!(
            serde_json::to_value(&r)
                .unwrap()
                .get("stringData")
                .is_none()
        );
    }
}
