use super::{Connection, discovery::Resource};
use crate::app::session::{Outcome, SessionId};
use crate::{app::event::Event, command::Action, resources::Object};
use ::kube::api::ListParams;
use anyhow::{Context, Result, ensure};
use k8s_openapi::api::core::v1::Event as KubeEvent;
use tokio::sync::mpsc;

pub async fn document(
    connection: &Connection,
    resource: &Resource,
    selected: &Object,
    action: Action,
    warning_only: bool,
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
    let mut value = serde_json::to_value(fresh)?;
    value["kind"] = resource.api.kind.clone().into();
    value["apiVersion"] = resource.api.api_version.clone().into();
    let object = Object::new(value);
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
        let total = events.len();
        let shown: Vec<&serde_json::Value> = if warning_only {
            events.iter().filter(|e| e["type"] == "Warning").collect()
        } else {
            events.iter().collect()
        };
        let mut text = format!(
            "Events for {}/{}; UID={}{}\n{}\n\n",
            object.kind,
            object.name,
            object.uid,
            if warning_only { " (Warning only)" } else { "" },
            warnings.join("\n")
        );
        for event in &shown {
            // Present-but-null fields (very common: core v1 Events rarely set every
            // timestamp field) must not win over a real value later in the chain --
            // `Option::or_else` alone stops at the first `Some`, even `Some(Value::Null)`.
            let time = [
                event.pointer("/series/lastObservedTime"),
                event.get("eventTime"),
                event.get("lastTimestamp"),
                event.pointer("/metadata/creationTimestamp"),
            ]
            .into_iter()
            .flatten()
            .find(|v| !v.is_null());
            let field_path = event
                .pointer("/involvedObject/fieldPath")
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())
                .map(|s| format!(" ({s})"))
                .unwrap_or_default();
            text.push_str(&format!(
                "{} {} {} count={}{field_path}\n{}\n\n",
                time.map(ToString::to_string)
                    .unwrap_or_else(|| "unknown time".into()),
                event["type"].as_str().unwrap_or("?"),
                event["reason"].as_str().unwrap_or("?"),
                event["count"],
                event["message"].as_str().unwrap_or("")
            ));
        }
        if shown.is_empty() {
            if warning_only && total > 0 {
                text.push_str(&format!(
                    "No Warning Events among {total} related Event(s). Event retention is limited; absence is not proof of health.\n"
                ));
            } else {
                text.push_str(
                    "No Events returned. Event retention is limited; absence is not proof of health.\n",
                );
            }
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

pub use super::logs::LogOptions;

pub async fn logs(
    connection: Connection,
    object: std::sync::Arc<Object>,
    options: LogOptions,
    epoch: u64,
    request: u64,
    tx: mpsc::Sender<Event>,
    session: SessionId,
) -> Outcome {
    super::logs::run(
        connection,
        super::logs::Request {
            objects: vec![object],
            options,
        },
        epoch,
        request,
        tx,
        session,
    )
    .await
}
