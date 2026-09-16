//! Bounded native log fan-in. All stream futures live inside their owning session.
use super::Connection;
use crate::{
    app::{
        event::{Event, Payload},
        session::{Outcome, SessionId},
    },
    resources::SharedObject,
};
use anyhow::{Context, Result, ensure};
use futures_util::{AsyncReadExt, StreamExt, stream::FuturesUnordered};
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, api::LogParams};
use std::time::Duration;
use tokio::sync::mpsc;

pub const MAX_SOURCES: usize = 8;
#[derive(Clone)]
pub struct LogOptions {
    pub container: Option<String>,
    pub previous: bool,
}
#[derive(Clone)]
pub struct Request {
    pub objects: Vec<SharedObject>,
    pub options: LogOptions,
}
#[derive(Clone)]
pub struct Source {
    pub object: SharedObject,
    pub container: String,
}
impl Source {
    fn label(&self) -> String {
        format!(
            "{}/{}:{}",
            self.object.namespace, self.object.name, self.container
        )
    }
}
pub fn sources(request: &Request) -> Result<Vec<Source>> {
    let mut result = Vec::new();
    ensure!(!request.objects.is_empty(), "No visible Pods to stream");
    for object in &request.objects {
        ensure!(
            object.api_version == "v1" && object.kind == "Pod",
            "Logs require Pods"
        );
        ensure!(
            !object.uid.is_empty() && !object.namespace.is_empty(),
            "Logs require explicit Pod UID and namespace"
        );
        let containers = |field| {
            object
                .value
                .pointer(field)
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|c| c["name"].as_str())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let regular = containers("/spec/containers");
        let mut all = regular.clone();
        all.extend(containers("/spec/initContainers"));
        all.extend(containers("/spec/ephemeralContainers"));
        let names = match request.options.container.as_deref() {
            Some("*") => all,
            Some(name) => {
                ensure!(
                    all.iter().any(|n| n == name),
                    "Unknown container {name}; available: {}",
                    all.join(", ")
                );
                vec![name.into()]
            }
            None => {
                ensure!(
                    regular.len() == 1,
                    "Choose :logs NAME or :logs * (available: {})",
                    all.join(", ")
                );
                regular
            }
        };
        for container in names {
            ensure!(
                result.len() < MAX_SOURCES,
                "More than {MAX_SOURCES} log sources; narrow the visible Pods or choose one container"
            );
            result.push(Source {
                object: object.clone(),
                container,
            });
        }
    }
    ensure!(!result.is_empty(), "No containers available for logs");
    Ok(result)
}
#[derive(Clone)]
struct Output {
    epoch: u64,
    request: u64,
    session: SessionId,
    tx: mpsc::Sender<Event>,
}
impl Output {
    async fn send(&self, payload: Payload) -> Result<()> {
        self.tx
            .send(Event {
                epoch: self.epoch,
                payload,
            })
            .await
            .context("Log view closed")
    }
    async fn line(&self, source: &str, bytes: &[u8], clipped: bool) -> Result<()> {
        let mut line = format!(
            "[{source}] {}",
            crate::safety::text(&String::from_utf8_lossy(bytes))
        );
        if clipped {
            line.push_str(" [line truncated at 16 KiB]");
        }
        self.send(Payload::LogLine {
            request: self.request,
            session: self.session,
            line,
        })
        .await
    }
}
pub async fn run(
    connection: Connection,
    request: Request,
    epoch: u64,
    request_id: u64,
    tx: mpsc::Sender<Event>,
    session: SessionId,
) -> Outcome {
    let sources = match sources(&request) {
        Ok(s) => s,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let output = Output {
        epoch,
        request: request_id,
        session,
        tx,
    };
    let mut streams = FuturesUnordered::new();
    for source in sources {
        let connection = connection.clone();
        let output = output.clone();
        let previous = request.options.previous;
        streams.push(async move {
            let label = source.label();
            match stream(&connection, &source, previous, &output).await {
                Ok(()) => None,
                Err(e) => {
                    let message = crate::safety::text(&format!("{label}: {e}"));
                    let _ = output
                        .send(Payload::LogSourceError {
                            request: output.request,
                            session,
                            message: message.clone(),
                        })
                        .await;
                    Some(message)
                }
            }
        });
    }
    let mut errors = Vec::new();
    while let Some(error) = streams.next().await {
        if let Some(error) = error {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Outcome::Completed
    } else {
        Outcome::Failed(errors.join("; "))
    }
}
async fn check_uid(connection: &Connection, api: &Api<Pod>, source: &Source) -> Result<()> {
    super::check_pod_uid(connection, api, &source.object.name, &source.object.uid).await
}
async fn stream(
    connection: &Connection,
    source: &Source,
    previous: bool,
    output: &Output,
) -> Result<()> {
    let api: Api<Pod> = Api::namespaced(connection.client.clone(), &source.object.namespace);
    check_uid(connection, &api, source).await?;
    let params = LogParams {
        container: Some(source.container.clone()),
        follow: !previous,
        previous,
        timestamps: true,
        tail_lines: Some(300),
        ..Default::default()
    };
    let mut stream = tokio::time::timeout(
        connection.timeout(),
        api.log_stream(&source.object.name, &params),
    )
    .await
    .context("Log connection timed out")?
    .map_err(|e| anyhow::anyhow!(crate::safety::api_error(&e, "opening Pod logs")))?;
    // Do not emit bytes until a second identity read has closed the startup race.
    check_uid(connection, &api, source).await?;
    output
        .send(Payload::LogStarted {
            request: output.request,
            session: output.session,
        })
        .await?;
    let label = source.label();
    let mut chunk = [0; 4096];
    let mut line = Vec::with_capacity(16_384);
    let mut clipped = false;
    let mut identity = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    identity.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let count = tokio::select! {
            biased;
            _ = identity.tick() => { check_uid(connection, &api, source).await?; continue; },
            count = stream.read(&mut chunk) => count.context("Log stream interrupted")?,
        };
        if count == 0 {
            if !line.is_empty() {
                output.line(&label, &line, clipped).await?;
            }
            return Ok(());
        }
        for byte in &chunk[..count] {
            if *byte == b'\n' {
                output.line(&label, &line, clipped).await?;
                line.clear();
                clipped = false;
            } else if line.len() < 16_384 {
                line.push(*byte);
            } else {
                clipped = true;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::Object;
    use serde_json::json;
    #[test]
    fn source_selection_is_explicit_bounded_and_includes_init_ephemeral() {
        let object = std::sync::Arc::new(Object::new(
            json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"p","namespace":"n","uid":"u"},"spec":{"containers":[{"name":"a"},{"name":"b"}],"initContainers":[{"name":"init"}],"ephemeralContainers":[{"name":"debug"}]}}),
        ));
        let mut request = Request {
            objects: vec![object.clone()],
            options: LogOptions {
                container: None,
                previous: false,
            },
        };
        assert!(sources(&request).is_err());
        for name in ["init", "debug"] {
            request.options.container = Some(name.into());
            assert_eq!(sources(&request).expect("source")[0].container, name);
        }
        request.options.container = Some("*".into());
        assert_eq!(sources(&request).expect("all").len(), 4);
        request.objects = vec![object; 3];
        assert!(sources(&request).is_err());
    }
}
