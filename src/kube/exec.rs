//! Native Kubernetes exec and attach: structured argv, explicit container,
//! UID-pinned. `run()` is one-shot and non-interactive (no stdin/TTY);
//! `interactive()` (exec a shell) and `attach()` (the already-running process)
//! both hand stdin/stdout to the remote over the shared `forward_interactive()`
//! loop and need a terminal-handoff guard from the caller (see
//! `app::terminal::TerminalHandoff`) -- neither manages the local terminal itself.
use super::Connection;
use crate::{
    app::{
        event::{Event, Payload},
        session::{Outcome, SessionId},
    },
    resources::SharedObject,
};
use anyhow::{Context, Result, ensure};
use futures_util::SinkExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{
    Api,
    api::{AttachParams, AttachedProcess, TerminalSize},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

#[derive(Clone)]
pub struct Request {
    pub object: SharedObject,
    pub container: Option<String>,
    pub command: Vec<String>,
}

/// The container to target: the one named container if there is only one,
/// otherwise a required, explicit choice -- never guessed for a multi-container
/// Pod. Shared by one-shot exec and interactive shell; mirrors
/// `kube::logs::sources`'s bare-`:logs` convention.
fn resolve_container(object: &SharedObject, requested: &Option<String>) -> Result<String> {
    ensure!(
        object.api_version == "v1" && object.kind == "Pod",
        "Exec requires a Pod"
    );
    ensure!(
        !object.uid.is_empty() && !object.namespace.is_empty(),
        "Exec requires explicit Pod UID and namespace"
    );
    let names = object
        .value
        .pointer("/spec/containers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c["name"].as_str())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    match requested {
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
                "Choose an explicit container (available: {})",
                names.join(", ")
            );
            Ok(names.into_iter().next().expect("checked len == 1"))
        }
    }
}

pub fn container(request: &Request) -> Result<String> {
    ensure!(
        !request.command.is_empty(),
        "Exec requires a command after --"
    );
    resolve_container(&request.object, &request.container)
}

/// A shell target: no argv (defaults to `sh`, present on essentially every real
/// container image; an explicit `shell` overrides it -- deliberately no bash-then-
/// sh auto-detection, which would need a wasted or ambiguous partial attach).
#[derive(Clone)]
pub struct ShellRequest {
    pub object: SharedObject,
    pub container: Option<String>,
    pub shell: Option<String>,
}

pub fn shell_container(request: &ShellRequest) -> Result<String> {
    resolve_container(&request.object, &request.container)
}

/// Runs one interactive TTY session, forwarding raw bytes between `stdin`/`stdout`
/// and the remote shell until either side closes. The caller owns the local
/// terminal's suspended state (`app::terminal::TerminalHandoff`); this function
/// only ever touches the streams it's given, never the real terminal directly.
pub async fn interactive(
    connection: &Connection,
    request: &ShellRequest,
    stdin: impl AsyncRead + Unpin,
    stdout: impl AsyncWrite + Unpin,
) -> Result<Outcome> {
    let container = shell_container(request)?;
    let api: Api<Pod> = Api::namespaced(connection.client.clone(), &request.object.namespace);
    super::check_pod_uid(connection, &api, &request.object.name, &request.object.uid).await?;
    let shell = request.shell.clone().unwrap_or_else(|| "sh".into());
    let params = AttachParams::interactive_tty().container(container);
    let attached = tokio::time::timeout(
        connection.timeout(),
        api.exec(&request.object.name, [shell.as_str()], &params),
    )
    .await
    .context("Exec connection timed out")?
    .map_err(|e| anyhow::anyhow!(crate::safety::api_error(&e, "starting interactive shell")))?;
    super::check_pod_uid(connection, &api, &request.object.name, &request.object.uid).await?;
    forward_interactive(attached, stdin, stdout, "Shell exited non-zero").await
}

/// An attach target: the already-running main process, never a new one. Unlike
/// exec/shell, attach has no argv -- there is nothing to start.
#[derive(Clone)]
pub struct AttachRequest {
    pub object: SharedObject,
    pub container: Option<String>,
}

pub fn attach_container(request: &AttachRequest) -> Result<String> {
    resolve_container(&request.object, &request.container)
}

/// Attach is only meaningfully interactive if the container's own spec was
/// created with both `stdin: true` and `tty: true` -- Kubernetes does not
/// allocate a PTY for the container's process otherwise, so a bare attach
/// would just be a one-way, no-signal-handling view of raw output with no
/// stdin channel to type into. Refusing this explicitly (pointing at `:logs`)
/// is more honest than silently degrading to a read-only mode nobody asked for.
pub fn container_supports_interactive_attach(object: &SharedObject, container: &str) -> bool {
    object
        .value
        .pointer("/spec/containers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|c| c["name"].as_str() == Some(container))
        .is_some_and(|c| {
            c["tty"].as_bool().unwrap_or(false) && c["stdin"].as_bool().unwrap_or(false)
        })
}

/// Attaches to the already-running process (`Api::attach`, distinct from
/// `Api::exec`: no command, no new process). Reuses the exact same terminal-
/// handoff contract and forwarding loop as `interactive()` -- the caller is
/// responsible for the same `TerminalHandoff` suspension.
pub async fn attach(
    connection: &Connection,
    request: &AttachRequest,
    stdin: impl AsyncRead + Unpin,
    stdout: impl AsyncWrite + Unpin,
) -> Result<Outcome> {
    let container = attach_container(request)?;
    ensure!(
        container_supports_interactive_attach(&request.object, &container),
        "Container {container} was not started with stdin+tty (required for an \
         interactive attach); use :logs to view its output instead"
    );
    let api: Api<Pod> = Api::namespaced(connection.client.clone(), &request.object.namespace);
    super::check_pod_uid(connection, &api, &request.object.name, &request.object.uid).await?;
    let params = AttachParams::interactive_tty().container(container);
    let attached = tokio::time::timeout(
        connection.timeout(),
        api.attach(&request.object.name, &params),
    )
    .await
    .context("Attach connection timed out")?
    .map_err(|e| anyhow::anyhow!(crate::safety::api_error(&e, "attaching to container")))?;
    super::check_pod_uid(connection, &api, &request.object.name, &request.object.uid).await?;
    forward_interactive(attached, stdin, stdout, "Attached process exited non-zero").await
}

/// The shared byte-forwarding loop behind both `interactive()` (exec) and
/// `attach()`: relays local stdin to the remote process and its stdout back to
/// local stdout, drives the remote resize channel from local terminal size, and
/// reports a final `Outcome` from the remote exit status. Neither caller reads
/// stderr separately -- an allocated PTY multiplexes both onto the one stdout
/// stream, matching how a real terminal session behaves.
async fn forward_interactive(
    mut attached: AttachedProcess,
    mut stdin: impl AsyncRead + Unpin,
    mut stdout: impl AsyncWrite + Unpin,
    failure_label: &str,
) -> Result<Outcome> {
    let mut status_rx = attached.take_status();
    let mut remote_stdin = attached.stdin().context("No stdin stream")?;
    let mut remote_stdout = attached.stdout().context("No stdout stream")?;
    let mut resize = attached.terminal_size();
    let mut last_size = None;
    let mut resize_poll = tokio::time::interval(std::time::Duration::from_millis(250));
    let mut input_chunk = [0u8; 4096];
    let mut output_chunk = [0u8; 4096];
    loop {
        tokio::select! {
            biased;
            _ = resize_poll.tick() => {
                if let Some(tx) = resize.as_mut()
                    && let Ok((width, height)) = crossterm::terminal::size()
                    && last_size != Some((width, height))
                {
                    last_size = Some((width, height));
                    // A closed receiver (tty=false on the remote end, or the
                    // session already ending) is not itself an error here.
                    let _ = tx.send(TerminalSize { width, height }).await;
                }
            }
            read = stdin.read(&mut input_chunk) => {
                let count = read.context("Local stdin read failed")?;
                if count == 0 {
                    // Ctrl-D / local EOF: tell the remote side and stop reading,
                    // but keep relaying its output until it actually closes.
                    let _ = remote_stdin.shutdown().await;
                    break;
                }
                // Ctrl-] (0x1D), alone: a local-only detach, independent of the
                // remote's cooperation. Ctrl-D only ends a session that reads
                // and reacts to it (a real shell does); attaching to an
                // already-running process that never reads stdin at all (the
                // common case -- it was not started expecting input) would
                // otherwise hang here forever waiting for output that never
                // stops. Detaching observes local-only semantics exactly like
                // ending a background session (see docs/SESSIONS.md): it stops
                // watching, never touches the remote process.
                if input_chunk[..count] == [0x1d] {
                    return Ok(Outcome::Cancelled);
                }
                remote_stdin.write_all(&input_chunk[..count]).await.context("Failed to forward stdin")?;
            }
            read = remote_stdout.read(&mut output_chunk) => {
                let count = read.context("Remote output stream interrupted")?;
                if count == 0 {
                    break;
                }
                stdout.write_all(&output_chunk[..count]).await.context("Failed to write local stdout")?;
                stdout.flush().await.context("Failed to flush local stdout")?;
            }
        }
    }
    // Keep draining any already-buffered remote output after our side of stdin
    // closed, so a process that prints a final message on EOF is not truncated.
    loop {
        let count = remote_stdout
            .read(&mut output_chunk)
            .await
            .context("Remote output stream interrupted")?;
        if count == 0 {
            break;
        }
        stdout
            .write_all(&output_chunk[..count])
            .await
            .context("Failed to write local stdout")?;
    }
    stdout
        .flush()
        .await
        .context("Failed to flush local stdout")?;
    drop(remote_stdin);
    drop(remote_stdout);
    attached
        .join()
        .await
        .map_err(|e| anyhow::anyhow!("Exec stream error: {e}"))?;
    let status = match status_rx.take() {
        Some(recv) => recv.await,
        None => None,
    };
    Ok(match status {
        Some(status) if status.status.as_deref() == Some("Success") => Outcome::Completed,
        Some(status) => Outcome::Failed(
            status
                .message
                .or(status.reason)
                .unwrap_or_else(|| failure_label.into()),
        ),
        None => Outcome::Completed,
    })
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
    #[test]
    fn shell_defaults_to_sh_selection_rules_match_exec() {
        let object = std::sync::Arc::new(Object::new(json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"name":"p","namespace":"n","uid":"u"},
            "spec":{"containers":[{"name":"a"},{"name":"b"}]}
        })));
        let mut request = ShellRequest {
            object: object.clone(),
            container: None,
            shell: None,
        };
        assert!(shell_container(&request).is_err(), "must require a choice");
        request.container = Some("b".into());
        assert_eq!(shell_container(&request).expect("b"), "b");
    }
    #[test]
    fn attach_requires_the_container_to_have_been_started_with_stdin_and_tty() {
        let object = std::sync::Arc::new(Object::new(json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"name":"p","namespace":"n","uid":"u"},
            "spec":{"containers":[
                {"name":"plain"},
                {"name":"stdin-only","stdin":true},
                {"name":"interactive","stdin":true,"tty":true}
            ]}
        })));
        assert!(!container_supports_interactive_attach(&object, "plain"));
        assert!(!container_supports_interactive_attach(
            &object,
            "stdin-only"
        ));
        assert!(container_supports_interactive_attach(
            &object,
            "interactive"
        ));
        assert!(!container_supports_interactive_attach(&object, "missing"));
    }
    #[test]
    fn attach_container_selection_matches_exec_and_shell() {
        let object = std::sync::Arc::new(Object::new(json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"name":"p","namespace":"n","uid":"u"},
            "spec":{"containers":[{"name":"only","stdin":true,"tty":true}]}
        })));
        let request = AttachRequest {
            object,
            container: None,
        };
        assert_eq!(attach_container(&request).expect("only"), "only");
    }
}
