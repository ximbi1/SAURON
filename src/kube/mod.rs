pub mod discovery;
pub mod evidence;
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
}
#[derive(Clone)]
pub struct Connection {
    pub client: Client,
    pub context: String,
    pub cluster: String,
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
    let settings = app_config.resolve(&cluster, &context)?;
    let deadline = Duration::from_secs(settings.request_timeout_secs);
    let mut config=tokio::time::timeout(deadline,Config::from_custom_kubeconfig(raw,&KubeConfigOptions{context:Some(context.clone()),..Default::default()})).await
        .map_err(|_|anyhow::anyhow!("Kubeconfig authentication timed out"))?
        .map_err(|_|anyhow::anyhow!("Cannot build Kubernetes configuration; check TLS files and credential plugin configuration"))?;
    if config.accept_invalid_certs {
        bail!("Kubeconfig disables TLS verification; use a context with valid certificate trust");
    }
    config.connect_timeout = Some(deadline);
    config
        .headers
        .push(("user-agent".parse()?, crate::brand::USER_AGENT.parse()?));
    let namespace = settings
        .namespace
        .clone()
        .unwrap_or_else(|| config.default_namespace.clone());
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
