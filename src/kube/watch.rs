use super::{Connection, discovery::Resource};
use crate::{
    app::event::{Event, Payload},
    resources::Object,
};
use ::kube::{
    core::DynamicObject,
    runtime::{WatchStreamExt, watcher},
};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub resource: String,
    pub namespace: Option<String>,
    pub labels: Option<String>,
    pub fields: Option<String>,
}

pub async fn run(
    connection: Connection,
    resource: Resource,
    query: Query,
    epoch: u64,
    tx: mpsc::Sender<Event>,
    cancel: CancellationToken,
) {
    let api = resource.api(connection.client.clone(), query.namespace.as_deref());
    let mut config = watcher::Config::default().page_size(200).timeout(30);
    if let Some(labels) = &query.labels {
        config = config.labels(labels);
    }
    if let Some(fields) = &query.fields {
        config = config.fields(fields);
    }
    let mut stream = watcher(api, config).default_backoff().boxed();
    loop {
        let event =
            tokio::select! {biased; _=cancel.cancelled()=>break, event=stream.next()=>event};
        let Some(event) = event else {
            break;
        };
        let payload = match event {
            Ok(watcher::Event::Init) => Payload::Begin,
            Ok(watcher::Event::InitDone) => Payload::Ready,
            Ok(watcher::Event::InitApply(object)) => match convert(object, &resource) {
                Some(o) => Payload::Apply(o, true),
                None => {
                    Payload::WatchError("Object serialization failed; results incomplete".into())
                }
            },
            Ok(watcher::Event::Apply(object)) => match convert(object, &resource) {
                Some(o) => Payload::Apply(o, false),
                None => {
                    Payload::WatchError("Object serialization failed; results incomplete".into())
                }
            },
            Ok(watcher::Event::Delete(object)) => match convert(object, &resource) {
                Some(o) => Payload::Delete(o),
                None => Payload::WatchError("Delete serialization failed; refresh required".into()),
            },
            Err(error) => {
                tracing::warn!(event = "watch_error");
                // Runtime variants can contain arbitrary server text; don't format the body.
                let operation = format!(
                    "list/watch {} in {}",
                    resource.qualified(),
                    query.namespace.as_deref().unwrap_or("all namespaces")
                );
                let msg = match error {
                    watcher::Error::InitialListFailed(e)
                    | watcher::Error::WatchStartFailed(e)
                    | watcher::Error::WatchFailed(e) => crate::safety::api_error(&e, &operation),
                    _ => format!("Watch interrupted while {operation}; reconnecting with backoff"),
                };
                Payload::WatchError(msg)
            }
        };
        tokio::select! {biased;_=cancel.cancelled()=>break,result=tx.send(Event{epoch,payload})=>if result.is_err(){break;}}
    }
}

fn convert(object: DynamicObject, resource: &Resource) -> Option<Object> {
    let mut v = serde_json::to_value(object).ok()?;
    v["kind"] = resource.api.kind.clone().into();
    v["apiVersion"] = resource.api.api_version.clone().into();
    Some(Object::new(v))
}
