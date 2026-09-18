use crate::{kube::Connection, kube::printer::PrinterColumn, resources::Object};
pub struct Event {
    pub epoch: u64,
    pub payload: Payload,
}
pub enum Payload {
    Metrics {
        generation: u64,
        pins: crate::kube::metrics::Pins,
        result: Result<crate::kube::metrics::Batch, crate::evidence::Unknown>,
    },
    Connected(Box<Connection>),
    ConnectError(String),
    Begin,
    Apply(Object, bool),
    Delete(Object),
    Ready,
    WatchError(String),
    Document {
        request: u64,
        title: String,
        text: String,
        adjacent: Vec<crate::adjacent::Target>,
    },
    DocumentError {
        request: u64,
        error: String,
    },
    LogLine {
        request: u64,
        session: super::session::SessionId,
        line: String,
    },
    LogStarted {
        request: u64,
        session: super::session::SessionId,
    },
    LogSourceError {
        request: u64,
        session: super::session::SessionId,
        message: String,
    },
    ExecLine {
        request: u64,
        session: super::session::SessionId,
        line: String,
    },
    ExecStarted {
        request: u64,
        session: super::session::SessionId,
    },
    NamespaceList {
        request: u64,
        names: Vec<String>,
        truncated: bool,
    },
    /// Best-effort enrichment for the currently active watch (see `kube::printer`).
    /// Absence of this event (fetch failed or found no CRD) leaves generic/curated
    /// columns exactly as they already were -- there is no error variant to show,
    /// since this is enrichment, not a required part of showing the table at all.
    PrinterColumns(Vec<PrinterColumn>),
    /// M8.0: a server dry-run attempt finished -- never a real commit.
    MutationDryRun {
        request: u64,
        outcome: crate::mutation::MutationOutcome,
    },
    /// M8.0: a real commit attempt finished; `verification` is a distinct,
    /// separately-labeled fact from `outcome` itself (a fresh post-commit
    /// GET/observation), never collapsed into one "Success".
    MutationCommit {
        request: u64,
        outcome: crate::mutation::MutationOutcome,
        verification: Option<crate::mutation::Verification>,
    },
    /// M8B.5: the truthful, freshly-listed Drain preview finished loading
    /// (or failed to). Never synthesized -- `Err` means the list itself
    /// failed, not that the Node has zero Pods.
    DrainPlanned {
        request: u64,
        planned: Result<Vec<crate::mutation::drain::PlannedPod>, String>,
    },
    /// M8B.5: the orchestrated cordon-then-evict run finished (fully or
    /// partially -- `DrainReport` itself carries the per-step detail).
    DrainCommit {
        request: u64,
        report: crate::mutation::drain::DrainReport,
    },
}
