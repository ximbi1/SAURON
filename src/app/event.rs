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
}
