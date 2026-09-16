//! One-shot native Kubernetes exec: structured argv, explicit container, UID-pinned,
//! non-interactive (no stdin/TTY). Interactive shell is separate, later work -- it
//! needs a terminal-handoff mechanism this module does not attempt.
use super::Connection;
use crate::{
    app::{
        event::{Event, Payload},
        session::{Outcome, SessionId},
    },
    resources::SharedObject,
};
use anyhow::{Context, Result, ensure};
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, api::AttachParams};
use tokio::{io::AsyncReadExt, sync::mpsc};

#[derive(Clone)]
pub struct Request {
    pub object: SharedObject,
    pub container: Option<String>,
    pub command: Vec<String>,
}

/// The explicit container to exec into: the one named container if there is only
/// one, otherwise a required, explicit choice -- never guessed for a multi-container
/// Pod. Mirrors `kube::logs::sources`'s bare-`:logs` convention.
pub fn container(request: &Request) -> Result<String> {
    ensure!(
        request.object.api_version == "v1" && request.object.kind == "Pod",
        "Exec requires a Pod"
    );
    ensure!(
        !request.object.uid.is_empty() && !request.object.namespace.is_empty(),
        "Exec requires explicit Pod UID and namespace"
    );
    ensure!(
        !request.command.is_empty(),
        "Exec requires a command after --"
    );
    let names = request
        .object
        .value
        .pointer("/spec/containers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c["name"].as_str())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    match &request.container {
        Some(name) => {
            ensure!(
                names.iter().any(|n| n == name),
                "Unknown container {name}; available: {}",
                names.join(", ")
            );
            Ok(name.clone())
        }
        None => {
            ensure!(
                names.len() == 1,
                "Choose :exec CONTAINER -- ... (available: {})",
                names.join(", ")
            );
            Ok(names.into_iter().next().expect("checked len == 1"))
        }
    }
}

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
            .context("Exec view closed")
    }
    async fn line(&self, source: &str, bytes: &[u8]) -> Result<()> {
        self.send(Payload::ExecLine {
            request: self.request,
            session: self.session,
            line: format!(
                "[{source}] {}",
                crate::safety::text(&String::from_utf8_lossy(bytes))
            ),
        })
        .await
    }
}

/// Drains `reader` line by line (16 KiB clip, same bound as logs), tagging every
/// line with `source` ("stdout"/"stderr") so interleaved output stays attributable.
async fn drain(
    reader: impl tokio::io::AsyncRead + Unpin,
    source: &str,
    output: &Output,
) -> Result<()> {
    let mut reader = reader;
    let mut chunk = [0u8; 4096];
    let mut line = Vec::with_capacity(16_384);
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .context("Exec output stream interrupted")?;
        if count == 0 {
            if !line.is_empty() {
                output.line(source, &line).await?;
            }
            return Ok(());
        }
        for byte in &chunk[..count] {
            if *byte == b'\n' {
                output.line(source, &line).await?;
                line.clear();
            } else if line.len() < 16_384 {
                line.push(*byte);
            }
        }
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
    match run_inner(connection, request, epoch, request_id, tx, session).await {
        Ok(outcome) => outcome,
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

async fn run_inner(
    connection: Connection,
    request: Request,
    epoch: u64,
    request_id: u64,
    tx: mpsc::Sender<Event>,
    session: SessionId,
) -> Result<Outcome> {
    let container = container(&request)?;
    let api: Api<Pod> = Api::namespaced(connection.client.clone(), &request.object.namespace);
    super::check_pod_uid(&connection, &api, &request.object.name, &request.object.uid).await?;
    let params = AttachParams::default()
        .container(container)
        .stdin(false)
        .stdout(true)
        .stderr(true)
        .tty(false);
    let mut attached = tokio::time::timeout(
        connection.timeout(),
        api.exec(&request.object.name, &request.command, &params),
    )
    .await
    .context("Exec connection timed out")?
    .map_err(|e| anyhow::anyhow!(crate::safety::api_error(&e, "starting exec")))?;
    // Do not emit an "attached" status until a second identity read has closed the
    // startup race, exactly like logs' pre/post-connection check.
    super::check_pod_uid(&connection, &api, &request.object.name, &request.object.uid).await?;
    let output = Output {
        epoch,
        request: request_id,
        session,
        tx,
    };
    output
        .send(Payload::ExecStarted {
            request: request_id,
            session,
        })
        .await?;
    let mut status_rx = attached.take_status();
    let stdout = attached.stdout().context("No stdout stream")?;
    let stderr = attached.stderr().context("No stderr stream")?;
    // Both streams must be fully drained before `join()` -- an undrained duplex
    // buffer would otherwise deadlock the background message-loop task.
    let (stdout_result, stderr_result) = tokio::join!(
        drain(stdout, "stdout", &output),
        drain(stderr, "stderr", &output)
    );
    stdout_result?;
    stderr_result?;
    attached
        .join()
        .await
        .map_err(|e| anyhow::anyhow!("Exec stream error: {e}"))?;
    let status = match status_rx.take() {
        Some(recv) => recv.await,
        None => None,
    };
    match status {
        Some(status) if status.status.as_deref() == Some("Success") => Ok(Outcome::Completed),
        Some(status) => Ok(Outcome::Failed(
            status
                .message
                .or(status.reason)
                .unwrap_or_else(|| "Command exited non-zero".into()),
        )),
        None => Ok(Outcome::Completed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::Object;
    use serde_json::json;
    #[test]
    fn container_selection_requires_explicit_choice_for_multi_container_pods() {
        let object = std::sync::Arc::new(Object::new(json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"name":"p","namespace":"n","uid":"u"},
            "spec":{"containers":[{"name":"a"},{"name":"b"}]}
        })));
        let mut request = Request {
            object: object.clone(),
            container: None,
            command: vec!["echo".into(), "hi".into()],
        };
        assert!(container(&request).is_err());
        request.container = Some("a".into());
        assert_eq!(container(&request).expect("a"), "a");
        request.container = Some("missing".into());
        assert!(container(&request).is_err());
        request.container = None;
        request.command = vec![];
        assert!(container(&request).is_err());
    }
    #[test]
    fn single_container_pod_defaults_without_an_explicit_name() {
        let object = std::sync::Arc::new(Object::new(json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"name":"p","namespace":"n","uid":"u"},
            "spec":{"containers":[{"name":"only"}]}
        })));
        let request = Request {
            object,
            container: None,
            command: vec!["true".into()],
        };
        assert_eq!(container(&request).expect("only"), "only");
    }
}
