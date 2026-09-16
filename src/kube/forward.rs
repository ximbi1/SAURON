//! A local listener belongs to one immutable Pod incarnation, never the UI view.
use super::Connection;
use crate::resources::SharedObject;
use futures_util::{StreamExt, stream::FuturesUnordered};
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, api::Portforwarder};
use std::{
    fmt,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::{
    io::copy_bidirectional,
    net::{TcpListener, TcpStream},
    sync::watch,
};

pub const MAX_FORWARDS: usize = 4;
pub const MAX_CLIENTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ports {
    pub local: u16,
    pub remote: u16,
}
impl Ports {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let (local, remote) = text.split_once(':').unwrap_or(("0", text));
        anyhow::ensure!(
            !local.is_empty()
                && !remote.is_empty()
                && local.bytes().all(|b| b.is_ascii_digit())
                && remote.bytes().all(|b| b.is_ascii_digit()),
            "Use REMOTE or LOCAL:REMOTE (TCP port numbers only)"
        );
        let ports = Self {
            local: local.parse()?,
            remote: remote.parse()?,
        };
        anyhow::ensure!(ports.remote != 0, "Remote port must be 1–65535");
        Ok(ports)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    PermissionDenied,
    TargetGone,
    TargetReplaced,
    TargetTerminating,
    TargetEnded,
    TimedOut,
    PortInUse,
    ConnectionFailed,
    ProtocolError,
    LocalIo,
}
#[derive(Clone, Debug)]
pub struct Failure {
    pub kind: ErrorKind,
    pub message: String,
}
impl Failure {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    fn api(error: kube::Error) -> Self {
        let kind = match &error {
            kube::Error::Api(e) if e.code == 403 || e.code == 401 => ErrorKind::PermissionDenied,
            kube::Error::Api(e) if e.code == 404 => ErrorKind::TargetGone,
            _ => ErrorKind::ConnectionFailed,
        };
        Self::new(
            kind,
            crate::safety::api_error(&error, "accessing pinned port-forward Pod"),
        )
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub local: Option<SocketAddr>,
    pub clients: usize,
    pub accepted: u64,
    pub rejected: u64,
    pub failures: u64,
    pub last_error: Option<Failure>,
    pub ended: bool,
}

/// kube-client 4.2's Portforwarder has no Drop implementation. Never expose an
/// unowned instance to a cancellation point, including the post-upgrade UID GET.
struct OwnedForward(Option<Portforwarder>);
impl Drop for OwnedForward {
    fn drop(&mut self) {
        if let Some(forward) = &self.0 {
            forward.abort();
        }
    }
}
impl OwnedForward {
    async fn close(mut self) {
        if let Some(forward) = self.0.take() {
            // Abort *before* moving into join: cancellation of join cannot detach
            // a still-running transport. Join's cancellation error is expected.
            forward.abort();
            let _ = forward.join().await;
        }
    }
}
pub fn validate_target(object: &SharedObject) -> anyhow::Result<()> {
    anyhow::ensure!(
        object.kind == "Pod" && object.api_version == "v1",
        "Port forwarding currently requires a Pod"
    );
    anyhow::ensure!(
        !object.uid.is_empty() && !object.namespace.is_empty(),
        "Port forwarding requires an explicit namespace and UID"
    );
    Ok(())
}
pub fn declared_ports(object: &SharedObject) -> Vec<u16> {
    let mut ports = object
        .value
        .pointer("/spec/containers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|c| c["ports"].as_array().into_iter().flatten())
        .filter(|p| p["protocol"].as_str().is_none_or(|p| p == "TCP"))
        .filter_map(|p| {
            p["containerPort"]
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|p| *p != 0)
        })
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports.dedup();
    ports
}
async fn check_target(
    connection: &Connection,
    api: &Api<Pod>,
    object: &SharedObject,
) -> Result<(), Failure> {
    let pod = tokio::time::timeout(connection.timeout(), api.get(&object.name))
        .await
        .map_err(|_| Failure::new(ErrorKind::TimedOut, "Pod identity verification timed out"))?
        .map_err(Failure::api)?;
    if pod.metadata.uid.as_deref() != Some(object.uid.as_str()) {
        return Err(Failure::new(
            ErrorKind::TargetReplaced,
            "Pod UID changed; forward stopped, reselect explicitly",
        ));
    }
    if pod.metadata.deletion_timestamp.is_some() {
        return Err(Failure::new(
            ErrorKind::TargetTerminating,
            "Pod is terminating; forward stopped",
        ));
    }
    if matches!(
        pod.status.as_ref().and_then(|s| s.phase.as_deref()),
        Some("Succeeded" | "Failed")
    ) {
        return Err(Failure::new(
            ErrorKind::TargetEnded,
            "Pod has terminated; forward stopped",
        ));
    }
    Ok(())
}
async fn client(
    connection: &Connection,
    api: &Api<Pod>,
    object: &SharedObject,
    remote: u16,
    mut local: TcpStream,
) -> Result<(), Failure> {
    check_target(connection, api, object).await?;
    let forward = tokio::time::timeout(
        connection.timeout(),
        api.portforward(&object.name, &[remote]),
    )
    .await
    .map_err(|_| Failure::new(ErrorKind::TimedOut, "Port-forward upgrade timed out"))?
    .map_err(Failure::api)?;
    let mut owned = OwnedForward(Some(forward));
    check_target(connection, api, object).await?;
    let forward = owned.0.as_mut().expect("owned until close");
    let mut stream = forward
        .take_stream(remote)
        .ok_or_else(|| Failure::new(ErrorKind::ProtocolError, "Missing port stream"))?;
    let error = forward
        .take_error(remote)
        .ok_or_else(|| Failure::new(ErrorKind::ProtocolError, "Missing port error channel"))?;
    let result = {
        let copy = copy_bidirectional(&mut local, &mut stream);
        tokio::pin!(copy);
        let transfer = tokio::select! {
            result = &mut copy => result,
            error = error => {
                if error.is_some() { return Err(Failure::new(ErrorKind::ProtocolError, "Remote port refused or forwarding protocol failed")); }
                // Transport EOF can precede draining the buffered final bytes.
                copy.await
            },
        };
        transfer
            .map(|_| ())
            .map_err(|_| Failure::new(ErrorKind::ConnectionFailed, "TCP stream interrupted"))
    };
    drop(stream);
    owned.close().await;
    result
}
pub async fn run(
    connection: Connection,
    object: SharedObject,
    ports: Ports,
    progress: watch::Sender<Progress>,
) -> Result<(), Failure> {
    // Defense in depth for direct transport callers, not only the dispatcher.
    if connection.settings.readonly {
        return Err(Failure::new(
            ErrorKind::PermissionDenied,
            "Port forwarding is unavailable in read-only mode",
        ));
    }
    validate_target(&object).map_err(|e| Failure::new(ErrorKind::TargetGone, e.to_string()))?;
    let api: Api<Pod> = Api::namespaced(connection.client.clone(), &object.namespace);
    check_target(&connection, &api, &object).await?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, ports.local))
        .await
        .map_err(|e| {
            Failure::new(
                if e.kind() == std::io::ErrorKind::AddrInUse {
                    ErrorKind::PortInUse
                } else {
                    ErrorKind::LocalIo
                },
                "Cannot bind requested loopback port; choose another local port",
            )
        })?;
    let local = listener
        .local_addr()
        .map_err(|_| Failure::new(ErrorKind::LocalIo, "Cannot inspect bound loopback address"))?;
    let mut status = Progress {
        local: Some(local),
        ..Default::default()
    };
    progress.send_replace(status.clone());
    let monitor = async {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            check_target(&connection, &api, &object).await?;
        }
        #[allow(unreachable_code)]
        Ok::<(), Failure>(())
    };
    tokio::pin!(monitor);
    let mut clients = FuturesUnordered::new();
    loop {
        tokio::select! {
            biased;
            result = &mut monitor => return result,
            result = clients.next(), if !clients.is_empty() => {
                let result: Option<Result<(), Failure>> = result;
                if let Some(Err(error)) = result {
                    if matches!(error.kind, ErrorKind::TargetGone | ErrorKind::TargetReplaced | ErrorKind::TargetTerminating | ErrorKind::TargetEnded | ErrorKind::PermissionDenied) { return Err(error); }
                    status.failures = status.failures.saturating_add(1);
                    status.last_error = Some(error);
                }
                status.clients = clients.len();
                progress.send_replace(status.clone());
            },
            accepted = listener.accept() => {
                let (socket, _) = accepted.map_err(|_| Failure::new(ErrorKind::LocalIo, "Local listener failed"))?;
                if clients.len() == MAX_CLIENTS {
                    status.rejected = status.rejected.saturating_add(1);
                    drop(socket);
                } else {
                    status.accepted = status.accepted.saturating_add(1);
                    clients.push(client(&connection, &api, &object, ports.remote, socket));
                }
                status.clients = clients.len();
                progress.send_replace(status.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ports_have_no_address_or_zero_remote_escape_hatch() {
        assert_eq!(
            Ports::parse("8080").expect("auto"),
            Ports {
                local: 0,
                remote: 8080
            }
        );
        assert_eq!(Ports::parse("9000:8080").expect("explicit").local, 9000);
        for bad in [
            "0",
            "1:0",
            "-1:80",
            "0.0.0.0:80",
            "127.0.0.1:0:80",
            "65536:80",
            "80:65536",
            ":80",
            "80:",
        ] {
            assert!(Ports::parse(bad).is_err(), "{bad}");
        }
    }
}
