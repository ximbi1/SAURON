use crate::{kube::Connection, resources::Object};
pub struct Event {
    pub epoch: u64,
    pub payload: Payload,
}
pub enum Payload {
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
        line: String,
    },
    LogEnd {
        request: u64,
        message: String,
    },
}
