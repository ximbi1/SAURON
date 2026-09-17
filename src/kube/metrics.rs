//! One bounded Metrics API snapshot. Scheduling/ownership belongs to Runtime.
use super::{Connection, discovery::Resource};
use crate::{
    evidence::{Coverage, Evidence, Observation, Origin, Unknown},
    filters,
    resources::Object,
};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

pub const POLL: Duration = Duration::from_secs(15);
pub const TTL: Duration = Duration::from_secs(60);
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SAMPLES: usize = 5000;
const MAX_CONTAINERS: usize = 128;

#[derive(Clone, Debug)]
pub struct Usage {
    pub cpu: Result<f64, Unknown>,
    pub memory: Result<f64, Unknown>,
}
#[derive(Clone, Debug)]
pub struct Sample {
    pub uid: Option<String>,
    pub window_start: DateTime<Utc>,
    pub node: Option<Usage>,
    pub containers: BTreeMap<String, Usage>,
}
#[derive(Clone, Debug)]
pub struct Batch {
    pub samples: BTreeMap<String, Evidence<Sample>>,
    pub coverage: Coverage,
}
#[derive(Clone, Debug)]
pub struct Pin {
    pub uid: String,
    pub created: Option<DateTime<Utc>>,
}
pub type Pins = BTreeMap<String, Pin>;

pub fn supported(resource: &Resource) -> bool {
    resource.api.group.is_empty()
        && resource.api.version == "v1"
        && matches!(resource.api.plural.as_str(), "pods" | "nodes")
}
fn quantity(value: &Value) -> Result<f64, Unknown> {
    let text = value.as_str().ok_or(if value.is_null() {
        Unknown::NotReported
    } else {
        Unknown::Malformed
    })?;
    if text.ends_with('%') {
        return Err(Unknown::Malformed);
    }
    filters::quantity(text).ok_or(Unknown::Malformed)
}
fn usage(value: &Value) -> Usage {
    Usage {
        cpu: quantity(&value["cpu"]),
        memory: quantity(&value["memory"]),
    }
}
pub fn decode(value: Value, node: bool) -> Result<Batch, Unknown> {
    let items = value["items"].as_array().ok_or(Unknown::Malformed)?;
    let mut batch = Batch {
        samples: BTreeMap::new(),
        coverage: Coverage {
            truncated: items.len() > MAX_SAMPLES
                || value
                    .pointer("/metadata/continue")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty()),
            ..Default::default()
        },
    };
    for item in items.iter().take(MAX_SAMPLES) {
        let Some(name) = item
            .pointer("/metadata/name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 253)
        else {
            batch.coverage.malformed += 1;
            continue;
        };
        let namespace = if node {
            Some("")
        } else {
            item.pointer("/metadata/namespace")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() <= 63)
        };
        let Some(namespace) = namespace else {
            batch.coverage.malformed += 1;
            continue;
        };
        let timestamp = item["timestamp"]
            .as_str()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok());
        let observation = Observation::new(Origin::MetricsApi, timestamp);
        let sample = (|| {
            let timestamp = timestamp.ok_or(Unknown::NotReported)?;
            let window = item["window"]
                .as_str()
                .and_then(filters::duration)
                .filter(|s| *s > 0.0 && *s <= 3600.0)
                .ok_or(Unknown::Malformed)?;
            let window_start = timestamp
                .checked_sub_signed(chrono::Duration::milliseconds(
                    (window * 1000.0).ceil() as i64
                ))
                .ok_or(Unknown::Malformed)?;
            let uid = item
                .pointer("/metadata/uid")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .map(str::to_owned);
            let mut containers = BTreeMap::new();
            if !node {
                let list = item["containers"].as_array().ok_or(Unknown::NotReported)?;
                if list.len() > MAX_CONTAINERS {
                    return Err(Unknown::Partial);
                }
                for container in list {
                    let name = container["name"]
                        .as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 253)
                        .ok_or(Unknown::Malformed)?;
                    if containers
                        .insert(name.to_owned(), usage(&container["usage"]))
                        .is_some()
                    {
                        return Err(Unknown::Malformed);
                    }
                }
                if containers.is_empty() {
                    return Err(Unknown::NotReported);
                }
            }
            Ok(Sample {
                uid,
                window_start,
                node: node.then(|| usage(&item["usage"])),
                containers,
            })
        })();
        if !sample.as_ref().is_ok_and(|s| {
            s.node
                .as_ref()
                .into_iter()
                .chain(s.containers.values())
                .all(|u| u.cpu.is_ok() && u.memory.is_ok())
        }) {
            batch.coverage.malformed += 1;
        }
        let slot = format!("{namespace}/{name}");
        if batch.samples.contains_key(&slot) {
            batch.coverage.malformed += 1;
            batch.samples.insert(
                slot,
                Evidence {
                    value: Err(Unknown::Malformed),
                    observation,
                },
            );
        } else {
            batch.samples.insert(
                slot,
                Evidence {
                    value: sample,
                    observation,
                },
            );
        }
    }
    Ok(batch)
}
fn api_error(error: kube::Error) -> Unknown {
    match error {
        kube::Error::Api(e) if e.code == 401 || e.code == 403 => Unknown::Forbidden,
        kube::Error::Api(e) if e.code == 404 => Unknown::Unavailable,
        _ => Unknown::TransportError,
    }
}
pub async fn fetch(
    connection: &Connection,
    resource: &Resource,
    namespace: Option<&str>,
) -> Result<Batch, Unknown> {
    if !supported(resource) {
        return Err(Unknown::Unsupported);
    }
    let node = resource.api.plural == "nodes";
    let version = connection
        .catalog
        .resources
        .iter()
        .find(|r| r.api.group == "metrics.k8s.io" && r.api.plural == resource.api.plural)
        .map(|r| r.api.version.as_str())
        .unwrap_or("v1beta1");
    if !matches!(version, "v1" | "v1beta1") {
        return Err(Unknown::Unsupported);
    }
    if namespace.is_some_and(|ns| {
        ns.is_empty()
            || ns.len() > 63
            || !ns
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }) {
        return Err(Unknown::Malformed);
    }
    let path = match namespace.filter(|_| !node) {
        Some(ns) => format!("/apis/metrics.k8s.io/{version}/namespaces/{ns}/pods"),
        None => format!("/apis/metrics.k8s.io/{version}/{}", resource.api.plural),
    };
    let request = kube::core::Request::new(path)
        .list(&kube::api::ListParams::default().limit(MAX_SAMPLES as u32))
        .map_err(|_| Unknown::Malformed)?;
    tokio::time::timeout(connection.timeout(), async {
        // request_stream() consumes unbounded API error bodies internally. Inspect
        // status ourselves and never read rejected bodies (they may hold secrets).
        let response = connection
            .client
            .send(request.map(kube::client::Body::from))
            .await
            .map_err(api_error)?;
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err(Unknown::Forbidden),
            404 => return Err(Unknown::Unavailable),
            _ => return Err(Unknown::TransportError),
        }
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| Unknown::TransportError)?;
            if let Some(data) = frame.data_ref() {
                if bytes.len().saturating_add(data.len()) > MAX_BYTES {
                    return Err(Unknown::Partial);
                }
                bytes.extend_from_slice(data);
            }
        }
        let value = serde_json::from_slice(&bytes).map_err(|_| Unknown::Malformed)?;
        decode(value, node)
    })
    .await
    .map_err(|_| Unknown::TimedOut)?
}

/// Match a sample against both the pre-request incarnation and the current row.
pub fn correlate(sample: &Evidence<Sample>, pin: &Pin, object: &Object) -> Result<(), Unknown> {
    if pin.uid.is_empty() || pin.uid != object.uid {
        return Err(Unknown::TargetReplaced);
    }
    let value = sample.current(Utc::now(), TTL)?;
    if value.uid.as_deref().is_some_and(|uid| uid != pin.uid) {
        return Err(Unknown::TargetReplaced);
    }
    let created = pin.created.ok_or(Unknown::NotReported)?;
    if Some(created) != object.created || value.window_start < created {
        return Err(Unknown::Stale);
    }
    Ok(())
}
