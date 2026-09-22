pub mod discovery;
pub mod drain;
pub mod evidence;
pub mod exec;
pub mod forward;
pub mod helm;
pub mod logs;
pub mod metrics;
pub mod mutation;
pub mod printer;
pub mod relationships;
pub mod watch;

use crate::config::{Config as AppConfig, Settings};
use ::kube::{
    Client, Config,
    config::{KubeConfigOptions, Kubeconfig},
};
use anyhow::{Context, Result, bail};
use discovery::Catalog;
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug, Default)]
pub struct ConnectOptions {
    pub kubeconfig: Option<PathBuf>,
    pub context: Option<String>,
    /// Unconditionally forces `Settings.readonly = true` on every resolve, regardless
    /// of what a cluster/context config layer requests. `--readonly` on the CLI sets
    /// this; it is a hard safety override, not just a default a config file can lift.
    pub force_readonly: bool,
    /// M8: an explicit, external attestation that the active cluster is the
    /// isolated, guarded test cluster -- set ONLY by `--mutation-test-cluster-verified`,
    /// which only `scripts/test-cluster.sh` is meant to pass, and only after that
    /// script has independently proven cluster identity via Docker/API introspection.
    /// This is deliberately a SEPARATE state from `force_readonly`/`Settings.readonly`:
    /// `readonly=false` means mutating operations are not globally disabled; this flag
    /// means the active context has passed strong verification as the one cluster
    /// authorized for real mutations. Neither implies the other. NEVER derive this
    /// from the context/cluster name, and never set it against a real cluster.
    pub mutation_test_cluster_verified: bool,
}
#[derive(Clone)]
pub struct Connection {
    pub client: Client,
    pub context: String,
    pub cluster: String,
    /// M13.5: the real API server URL (`kube::Config::cluster_url`,
    /// captured before `Client::try_from` consumes the config) -- `cluster`
    /// above is only the kubeconfig's own cluster *alias*, never the
    /// address actually being talked to.
    pub server: String,
    pub namespace: String,
    pub contexts: Vec<String>,
    pub catalog: Catalog,
    pub settings: Settings,
    pub version: String,
}

pub async fn connect(options: ConnectOptions, app_config: AppConfig) -> Result<Connection> {
    // Parsing paths/credential files runs on a Tokio worker; never on the render owner.
    let raw = match &options.kubeconfig {
        Some(path) => Kubeconfig::read_from(path),
        None => Kubeconfig::read(),
    }
    .map_err(|_| {
        anyhow::anyhow!(
            "Cannot read kubeconfig; check --kubeconfig, KUBECONFIG and file permissions"
        )
    })?;
    let context = options
        .context
        .or_else(|| raw.current_context.clone())
        .context("Kubeconfig has no current context; use --context")?;
    let named = raw
        .contexts
        .iter()
        .find(|c| c.name == context)
        .and_then(|c| c.context.as_ref())
        .context("Requested context is not present in kubeconfig")?;
    let cluster = named.cluster.clone();
    let contexts = raw.contexts.iter().map(|c| c.name.clone()).collect();
    let mut settings = app_config.resolve(&cluster, &context)?;
    if options.force_readonly {
        settings.readonly = true;
    }
    let deadline = Duration::from_secs(settings.request_timeout_secs);
    let mut config=tokio::time::timeout(deadline,Config::from_custom_kubeconfig(raw,&KubeConfigOptions{context:Some(context.clone()),..Default::default()})).await
        .map_err(|_|anyhow::anyhow!("Kubeconfig authentication timed out"))?
        .map_err(|_|anyhow::anyhow!("Cannot build Kubernetes configuration; check TLS files and credential plugin configuration"))?;
    if config.accept_invalid_certs {
        bail!("Kubeconfig disables TLS verification; use a context with valid certificate trust");
    }
    config.connect_timeout = Some(deadline);
    // M8B.4 finding: kube-rs's `Config::default_retry` defaults to `true`,
    // installing a transport-level `RetryLayer` that silently retries 429/
    // 503/504 responses (up to 15 attempts, exponential backoff up to
    // 1000s) BELOW this module entirely. That directly contradicts every
    // mutation outcome this codebase promises ("no automatic retry, ever";
    // an ambiguous/denied outcome is reported, never silently retried) --
    // it was invisible until M8B.4 introduced the first mutation outcome
    // (429 DisruptionBudgetDenied) mapped from a status this layer treats
    // as retryable. `kube::mutation`'s executor is the only place that
    // gets to decide whether to retry (it decides: never) -- transport-
    // level retry must be off, for every request, not just mutations.
    config.default_retry = false;
    config
        .headers
        .push(("user-agent".parse()?, crate::brand::USER_AGENT.parse()?));
    let namespace = settings
        .namespace
        .clone()
        .unwrap_or_else(|| config.default_namespace.clone());
    let server = config.cluster_url.to_string();
    let client = Client::try_from(config).map_err(|_| {
        anyhow::anyhow!("Cannot initialize Kubernetes client; check TLS and authentication")
    })?;
    let catalog = discovery::discover(&client, deadline).await?;
    let version = match tokio::time::timeout(deadline, client.apiserver_version()).await {
        Ok(Ok(v)) => v.git_version,
        _ => "unknown (version endpoint unavailable)".into(),
    };
    tracing::info!(
        event = "connected",
        resources = catalog.resources.len(),
        partial = catalog.warnings.len()
    );
    Ok(Connection {
        client,
        context,
        cluster,
        server,
        namespace,
        contexts,
        catalog,
        settings,
        version,
    })
}

impl Connection {
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.settings.request_timeout_secs)
    }
}

/// Re-reads `name`/`namespace` and rejects if its UID no longer matches: the one
/// Kubernetes-side precondition every session-owned Pod operation (logs, exec) can
/// give itself, since the log/exec/attach subresource APIs accept no UID precondition
/// of their own. Best-effort, not atomic with the operation it guards.
pub async fn check_pod_uid(
    connection: &Connection,
    api: &::kube::Api<k8s_openapi::api::core::v1::Pod>,
    name: &str,
    uid: &str,
) -> Result<()> {
    let pod = tokio::time::timeout(connection.timeout(), api.get(name))
        .await
        .context("Pod identity check timed out; operation stopped")?
        .map_err(|e| anyhow::anyhow!(crate::safety::api_error(&e, "checking Pod UID")))?;
    anyhow::ensure!(
        pod.metadata.uid.as_deref() == Some(uid),
        "Pod replaced; select its new incarnation"
    );
    Ok(())
}
