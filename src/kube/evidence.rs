use super::{Connection, discovery::Resource};
use crate::{
    app::event::{Event, Payload},
    command::Action,
    resources::Object,
};
use ::kube::api::{ListParams, LogParams};
use anyhow::{Context, Result, ensure};
use futures_util::AsyncReadExt;
use k8s_openapi::api::core::v1::{Event as KubeEvent, Pod};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub async fn document(
    connection: &Connection,
    resource: &Resource,
    selected: &Object,
    action: Action,
) -> Result<String> {
    let api = resource.api(connection.client.clone(), Some(&selected.namespace));
    let fresh = tokio::time::timeout(connection.timeout(), api.get(&selected.name))
        .await
        .context("Resource read timed out")?
        .map_err(|e| {
            anyhow::anyhow!(crate::safety::api_error(
                &e,
                &format!("reading {}/{}", resource.qualified(), selected.name)
            ))
        })?;
    let object = Object::new(serde_json::to_value(fresh)?);
    ensure!(
        !selected.uid.is_empty() && object.uid == selected.uid,
        "Object was replaced or has no UID; return to the table and select it again"
    );
    if action == Action::Yaml {
        return Ok(crate::safety::text(&serde_yaml_ng::to_string(
            &object.value,
        )?));
    }
    let (events, warnings) = events(connection, &object).await;
    if action == Action::Explain {
        return Ok(crate::explain::report(&object, &events, &warnings));
    }
    if action == Action::Events {
        let mut text = format!(
            "Events for {}/{}; UID={}\n{}\n\n",
            object.kind,
            object.name,
            object.uid,
            warnings.join("\n")
        );
        for event in &events {
            let time = event
                .pointer("/series/lastObservedTime")
                .or_else(|| event.get("lastTimestamp"))
                .or_else(|| event.get("eventTime"))
                .or_else(|| event.pointer("/metadata/creationTimestamp"));
            text.push_str(&format!(
                "{} {} {} count={}\n{}\n\n",
                time.map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown time".into()),
                event["type"].as_str().unwrap_or("?"),
                event["reason"].as_str().unwrap_or("?"),
                event["count"],
                event["message"].as_str().unwrap_or("")
            ));
        }
        if events.is_empty() {
            text.push_str(
                "No Events returned. Event retention is limited; absence is not proof of health.\n",
            );
        }
        return Ok(crate::safety::text(&text));
    }
    let mut text = format!(
        "{}/{}\nNamespace: {}\nUID: {}\nResource version: {}\nStatus: {}\n\n",
        object.kind,
        object.name,
        object.namespace,
        object.uid,
        object.version,
        object.health.status
    );
    for section in ["metadata", "spec", "status"] {
        text.push_str(&format!(
            "{}\n{}\n",
            section.to_uppercase(),
            serde_yaml_ng::to_string(&object.value[section])?
        ));
    }
    text.push_str(&crate::explain::report(&object, &events, &warnings));
    Ok(crate::safety::text(&text))
}

pub async fn events(
    connection: &Connection,
    object: &Object,
) -> (Vec<serde_json::Value>, Vec<String>) {
    let api: ::kube::Api<KubeEvent> = if object.namespace.is_empty() {
        ::kube::Api::all(connection.client.clone())
    } else {
        ::kube::Api::namespaced(connection.client.clone(), &object.namespace)
    };
    let params = ListParams::default()
        .fields(&format!("involvedObject.uid={}", object.uid))
        .limit(200);
    match tokio::time::timeout(connection.timeout(), api.list(&params)).await {
        Ok(Ok(list)) => {
            let warnings = if list
                .metadata
                .continue_
                .as_deref()
                .is_some_and(|s| !s.is_empty())
            {
                vec!["PARTIAL: related Events limited to 200".into()]
            } else {
                vec![]
            };
            (
                list.items
                    .into_iter()
                    .filter_map(|e| serde_json::to_value(e).ok())
                    .collect(),
                warnings,
            )
        }
        Ok(Err(e)) => (
            vec![],
            vec![crate::safety::api_error(&e, "listing UID-related Events")],
        ),
        Err(_) => (vec![], vec!["Related Event request timed out".into()]),
    }
}

pub struct LogOptions {
    pub container: Option<String>,
    pub previous: bool,
}

pub async fn logs(
    connection: Connection,
    object: std::sync::Arc<Object>,
    options: LogOptions,
    epoch: u64,
    request: u64,
    tx: mpsc::Sender<Event>,
    cancel: CancellationToken,
) {
    let LogOptions {
        container,
        previous,
    } = options;
    let job = async {
        ensure!(
            object.kind == "Pod" && object.api_version == "v1",
            "Logs currently support Pods; choose a Pod first"
        );
        let names = object
            .value
            .pointer("/spec/containers")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| c["name"].as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        ensure!(
            container.is_some() || names.len() == 1,
            "Choose a container with :logs NAME (available: {})",
            names.join(", ")
        );
        let api: ::kube::Api<Pod> =
            ::kube::Api::namespaced(connection.client.clone(), &object.namespace);
        let fresh = tokio::time::timeout(connection.timeout(), api.get(&object.name))
            .await
            .context("Pod identity check timed out")?
            .map_err(|e| {
                anyhow::anyhow!(crate::safety::api_error(&e, "checking Pod UID before logs"))
            })?;
        ensure!(
            fresh.metadata.uid.as_deref() == Some(object.uid.as_str()),
            "Pod replaced; select its new incarnation"
        );
        let params = LogParams {
            container: container.or_else(|| names.first().map(|s| s.to_string())),
            follow: !previous,
            previous,
            timestamps: true,
            tail_lines: Some(300),
            ..Default::default()
        };
        let mut stream =
            tokio::time::timeout(connection.timeout(), api.log_stream(&object.name, &params))
                .await
                .context("Log connection timed out")?
                .map_err(|e| anyhow::anyhow!(crate::safety::api_error(&e, "opening Pod logs")))?;
        let mut chunk = [0u8; 4096];
        let mut line = Vec::with_capacity(16_384);
        let mut truncated = false;
        loop {
            let count = stream
                .read(&mut chunk)
                .await
                .map_err(|_| anyhow::anyhow!("Log stream interrupted"))?;
            if count == 0 {
                if !line.is_empty() {
                    send_line(&tx, epoch, request, &line, truncated).await?;
                }
                break;
            }
            for byte in &chunk[..count] {
                if *byte == b'\n' {
                    send_line(&tx, epoch, request, &line, truncated).await?;
                    line.clear();
                    truncated = false;
                } else if line.len() < 16_384 {
                    line.push(*byte);
                } else {
                    truncated = true;
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    };
    let result = tokio::select! {biased;_=cancel.cancelled()=>return,result=job=>result};
    let message = match result {
        Ok(()) => "Log stream ended".into(),
        Err(e) => e.to_string(),
    };
    tokio::select! {_=cancel.cancelled()=>{},_=tx.send(Event{epoch,payload:Payload::LogEnd{request,message}})=>{}}
}

async fn send_line(
    tx: &mpsc::Sender<Event>,
    epoch: u64,
    request: u64,
    bytes: &[u8],
    truncated: bool,
) -> Result<()> {
    let mut line = crate::safety::text(&String::from_utf8_lossy(bytes));
    if truncated {
        line.push_str(" [line truncated at 16 KiB]");
    }
    tx.send(Event {
        epoch,
        payload: Payload::LogLine { request, line },
    })
    .await
    .map_err(|_| anyhow::anyhow!("Log view closed"))
}
