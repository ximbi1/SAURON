pub mod accounting;
pub mod health;
pub mod priority;
pub mod sort;
pub mod store;
pub mod timeline;

use crate::{evidence::Unknown, safety};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

/// Live sampled usage lookup, implemented by `app::metrics::Cache`. Defined here
/// (not in `filters`) so `Field`/`Expr`/sort can depend on it without an upward
/// dependency from this crate's core resource types onto the `app` layer.
pub trait Metrics {
    fn usage(&self, object: &Object, cpu: bool) -> Result<f64, Unknown>;
    fn percentage(&self, object: &Object, cpu: bool, of_limit: bool) -> Result<f64, Unknown>;
}

#[derive(Clone, Debug)]
pub struct Object {
    pub value: Value,
    pub uid: String,
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub api_version: String,
    pub version: String,
    pub created: Option<DateTime<Utc>>,
    pub bytes: usize,
    pub health: health::Health,
    pub cells: Vec<(String, String)>,
}
impl Object {
    pub fn new(mut value: Value) -> Self {
        safety::redact(&mut value);
        let string = |p| {
            value
                .pointer(p)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let name = string("/metadata/name");
        let namespace = string("/metadata/namespace");
        let uid = string("/metadata/uid");
        let kind = string("/kind");
        let api_version = string("/apiVersion");
        let version = string("/metadata/resourceVersion");
        let created = string("/metadata/creationTimestamp").parse().ok();
        let health = health::derive(&value);
        let bytes = value.to_string().len();
        let cells = project(&value, &kind, &api_version, &health);
        Self {
            value,
            uid,
            name,
            namespace,
            kind,
            api_version,
            version,
            created,
            bytes,
            health,
            cells,
        }
    }
    pub fn slot(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
    pub fn age(&self, now: DateTime<Utc>) -> Option<f64> {
        self.created.map(|t| (now - t).num_seconds().max(0) as f64)
    }
    pub fn field(&self, key: &str, now: DateTime<Utc>) -> Option<String> {
        match key.to_ascii_lowercase().as_str() {
            "name" | "metadata.name" => (!self.name.is_empty()).then(|| self.name.clone()),
            "namespace" | "ns" | "metadata.namespace" => {
                (!self.namespace.is_empty()).then(|| self.namespace.clone())
            }
            "status" => (self.health.severity != health::Severity::Unknown)
                .then(|| self.health.status.clone()),
            "age" => self.age(now).map(|x| x.to_string()),
            "spec.nodename" => self
                .value
                .pointer("/spec/nodeName")
                .and_then(Value::as_str)
                .map(str::to_owned),
            "status.phase" => self
                .value
                .pointer("/status/phase")
                .and_then(Value::as_str)
                .map(str::to_owned),
            k => self
                .cells
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(k))
                .map(|(_, value)| value.clone())
                .filter(|v| v != "-"),
        }
    }
    pub fn search_text(&self) -> String {
        format!(
            "{} {} {} {}",
            self.namespace,
            self.name,
            self.health.status,
            self.cells
                .iter()
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

pub type SharedObject = Arc<Object>;

pub fn age_text(seconds: Option<f64>) -> String {
    match seconds {
        None => "-".into(),
        Some(s) if s < 60.0 => format!("{}s", s as u64),
        Some(s) if s < 3600.0 => format!("{}m", (s / 60.0) as u64),
        Some(s) if s < 86400.0 => format!("{}h", (s / 3600.0) as u64),
        Some(s) => format!("{}d", (s / 86400.0) as u64),
    }
}

pub fn scalar(v: &Value, path: &str) -> String {
    match v.pointer(path) {
        Some(Value::String(s)) => safety::text(s),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => "-".into(),
    }
}
pub fn number(v: &Value, path: &str) -> i64 {
    v.pointer(path).and_then(Value::as_i64).unwrap_or(0)
}

/// Round-trips through `filters::quantity`: a plain decimal below 1 core renders
/// with the conventional "m" (milli) suffix, matching kubectl's own convention.
pub fn cpu_text(cores: f64) -> String {
    if cores < 1.0 {
        format!("{}m", (cores * 1000.0).round() as i64)
    } else {
        let s = format!("{cores:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}
/// Round-trips through `filters::quantity`: largest binary unit that keeps the
/// value >= 1, matching kubectl's own binary-suffix convention.
pub fn memory_text(bytes: f64) -> String {
    const UNITS: [(&str, f64); 6] = [
        ("Ei", 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("Pi", 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("Ti", 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("Gi", 1024.0 * 1024.0 * 1024.0),
        ("Mi", 1024.0 * 1024.0),
        ("Ki", 1024.0),
    ];
    for (suffix, mult) in UNITS {
        if bytes >= mult {
            let s = format!("{:.2}", bytes / mult);
            let s = s.trim_end_matches('0').trim_end_matches('.');
            return format!("{s}{suffix}");
        }
    }
    (bytes.round() as i64).to_string()
}

fn project(v: &Value, kind: &str, api: &str, health: &health::Health) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut add = |name: &str, value: String| out.push((name.to_owned(), value));
    if api == "v1" && kind == "Pod" {
        let (ready, total, restarts) = health::pod_counts(v);
        let display =
            |value: Option<usize>| value.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
        add("READY", format!("{}/{}", display(ready), display(total)));
        add("STATUS", health.status.clone());
        add(
            "RESTARTS",
            restarts
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into()),
        );
        add("NODE", scalar(v, "/spec/nodeName"));
        let text = |r: Result<f64, crate::evidence::Unknown>, cpu: bool| match r {
            Ok(n) if cpu => cpu_text(n),
            Ok(n) => memory_text(n),
            Err(_) => "-".into(),
        };
        add(
            "CPU/R",
            text(accounting::pod_effective(v, "requests", true), true),
        );
        add(
            "MEM/R",
            text(accounting::pod_effective(v, "requests", false), false),
        );
        add(
            "CPU/L",
            text(accounting::pod_effective(v, "limits", true), true),
        );
        add(
            "MEM/L",
            text(accounting::pod_effective(v, "limits", false), false),
        );
        add("QOS", accounting::qos_class(v).unwrap_or("-").to_owned());
    } else if api == "apps/v1" && ["Deployment", "StatefulSet", "ReplicaSet"].contains(&kind) {
        add(
            "READY",
            format!(
                "{}/{}",
                scalar(v, "/status/readyReplicas"),
                scalar(v, "/spec/replicas")
            ),
        );
        add("STATUS", health.status.clone());
        add("UPDATED", scalar(v, "/status/updatedReplicas"));
        add("AVAILABLE", scalar(v, "/status/availableReplicas"));
    } else if api == "apps/v1" && kind == "DaemonSet" {
        add(
            "READY",
            format!(
                "{}/{}",
                scalar(v, "/status/numberReady"),
                scalar(v, "/status/desiredNumberScheduled")
            ),
        );
        add("STATUS", health.status.clone());
        add("AVAILABLE", scalar(v, "/status/numberAvailable"));
    } else if api == "v1" && kind == "Service" {
        add("TYPE", scalar(v, "/spec/type"));
        add("CLUSTER-IP", scalar(v, "/spec/clusterIP"));
        let ports = v
            .pointer("/spec/ports")
            .and_then(Value::as_array)
            .map(|ports| {
                ports
                    .iter()
                    .map(|p| {
                        let node = p
                            .get("nodePort")
                            .map(|n| format!(":{n}"))
                            .unwrap_or_default();
                        format!(
                            "{}{}{}{}",
                            scalar(p, "/port"),
                            node,
                            "/",
                            scalar(p, "/protocol")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        add("PORTS", ports);
    } else if api == "v1" && kind == "Node" {
        add("STATUS", health.status.clone());
        add("VERSION", scalar(v, "/status/nodeInfo/kubeletVersion"));
        add("CPU/A", scalar(v, "/status/allocatable/cpu"));
        add("MEM/A", scalar(v, "/status/allocatable/memory"));
        add("CPU/C", scalar(v, "/status/capacity/cpu"));
        add("MEM/C", scalar(v, "/status/capacity/memory"));
    } else if api == "v1" && ["ConfigMap", "Secret"].contains(&kind) {
        add(
            "DATA",
            v.get("data")
                .and_then(Value::as_object)
                .map(|m| m.len().to_string())
                .unwrap_or_else(|| "-".into()),
        );
        if kind == "Secret" {
            add("TYPE", scalar(v, "/type"));
        }
    } else if api == "batch/v1" && kind == "CronJob" {
        add("SCHEDULE", scalar(v, "/spec/schedule"));
        add("SUSPEND", scalar(v, "/spec/suspend"));
        add(
            "ACTIVE",
            v.pointer("/status/active")
                .and_then(Value::as_array)
                .map(|m| m.len().to_string())
                .unwrap_or_else(|| "-".into()),
        );
    } else if api == "batch/v1" && kind == "Job" {
        add("STATUS", health.status.clone());
        add("SUCCEEDED", scalar(v, "/status/succeeded"));
        add("FAILED", scalar(v, "/status/failed"));
    } else if api == "v1" && ["PersistentVolumeClaim", "PersistentVolume"].contains(&kind) {
        add("STATUS", health.status.clone());
        add(
            "CAPACITY",
            scalar(
                v,
                if kind == "PersistentVolume" {
                    "/spec/capacity/storage"
                } else {
                    "/status/capacity/storage"
                },
            ),
        );
        add("CLASS", scalar(v, "/spec/storageClassName"));
    } else if api == "networking.k8s.io/v1" && kind == "Ingress" {
        add("CLASS", scalar(v, "/spec/ingressClassName"));
        add(
            "HOSTS",
            v.pointer("/spec/rules")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|r| scalar(r, "/host"))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default(),
        );
    } else if api == "v1" && kind == "Endpoints" {
        add(
            "SUBSETS",
            v.get("subsets")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
                .to_string(),
        );
    } else if api == "apiextensions.k8s.io/v1" && kind == "CustomResourceDefinition" {
        add("GROUP", scalar(v, "/spec/group"));
        add("SCOPE", scalar(v, "/spec/scope"));
        add("STATUS", health.status.clone());
    } else if kind == "Event" && (api == "v1" || api.starts_with("events.k8s.io/")) {
        add("TYPE", scalar(v, "/type"));
        add("REASON", scalar(v, "/reason"));
        add(
            "MESSAGE",
            scalar(v, if api == "v1" { "/message" } else { "/note" }),
        );
    } else {
        add("STATUS", health.status.clone());
    }
    out
}
