use super::{number, scalar};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Severity {
    Healthy,
    Unknown,
    Warning,
    Critical,
}
#[derive(Clone, Debug)]
pub struct Health {
    pub status: String,
    pub severity: Severity,
}
impl Health {
    fn new(status: impl Into<String>, severity: Severity) -> Self {
        Self {
            status: status.into(),
            severity,
        }
    }
}
fn array<'a>(v: &'a Value, path: &str) -> &'a [Value] {
    v.pointer(path)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}
fn condition<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    array(v, "/status/conditions")
        .iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some(name))
}
fn cond(v: &Value, name: &str, status: &str) -> bool {
    condition(v, name).is_some_and(|c| c.get("status").and_then(Value::as_str) == Some(status))
}
fn is_sidecar(v: &Value, name: &str) -> bool {
    array(v, "/spec/initContainers")
        .iter()
        .any(|c| c["name"] == name && c["restartPolicy"] == "Always")
}

pub fn pod_counts(v: &Value) -> (Option<usize>, Option<usize>, Option<i64>) {
    let regular = array(v, "/status/containerStatuses");
    let init = array(v, "/status/initContainerStatuses");
    let Some(containers) = v.pointer("/spec/containers").and_then(Value::as_array) else {
        return (None, None, None);
    };
    let total = containers.len()
        + array(v, "/spec/initContainers")
            .iter()
            .filter(|c| c["restartPolicy"] == "Always")
            .count();
    let statuses = containers
        .iter()
        .map(|c| (c, regular))
        .chain(array(v, "/spec/initContainers").iter().map(|c| (c, init)))
        .map(|(spec, observed)| {
            let name = spec.get("name")?.as_str()?;
            let status = observed
                .iter()
                .find(|s| s.get("name").and_then(Value::as_str) == Some(name))?;
            Some((spec, status))
        })
        .collect::<Option<Vec<_>>>();
    let Some(statuses) = statuses else {
        return (None, Some(total), None);
    };
    let ready = statuses
        .iter()
        .filter(|(spec, _)| {
            containers.iter().any(|c| c["name"] == spec["name"])
                || spec["restartPolicy"] == "Always"
        })
        .try_fold(0usize, |n, (_, s)| {
            s.get("ready")?.as_bool().map(|r| n + usize::from(r))
        });
    let restarts = statuses.iter().try_fold(0i64, |n, (_, s)| {
        let count = s.get("restartCount")?.as_i64().filter(|n| *n >= 0)?;
        n.checked_add(count)
    });
    (ready, Some(total), restarts)
}

fn failure(c: &Value) -> Option<Health> {
    if let Some(reason) = c.pointer("/state/waiting/reason").and_then(Value::as_str) {
        let severity = if ["ContainerCreating", "PodInitializing"].contains(&reason) {
            Severity::Warning
        } else {
            Severity::Critical
        };
        return Some(Health::new(reason, severity));
    }
    if let Some(t) = c.pointer("/state/terminated")
        && number(t, "/exitCode") != 0
    {
        return Some(Health::new(
            t["reason"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("ExitCode:{}", number(t, "/exitCode"))),
            Severity::Critical,
        ));
    }
    None
}

pub fn derive(v: &Value) -> Health {
    use Severity::*;
    if v.pointer("/metadata/deletionTimestamp")
        .is_some_and(|x| !x.is_null())
    {
        return Health::new("Terminating", Warning);
    }
    let kind = v["kind"].as_str().unwrap_or_default();
    let api = v["apiVersion"].as_str().unwrap_or_default();
    if kind == "Pod" && api == "v1" {
        let phase = v
            .pointer("/status/phase")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        if phase == "Succeeded" {
            return Health::new("Completed", Healthy);
        }
        if phase == "Failed" {
            return Health::new(
                v.pointer("/status/reason")
                    .and_then(Value::as_str)
                    .unwrap_or("Failed"),
                Critical,
            );
        }
        let initialized = cond(v, "Initialized", "True");
        for c in array(v, "/status/initContainerStatuses") {
            if (!initialized || is_sidecar(v, c["name"].as_str().unwrap_or_default()))
                && let Some(mut h) = failure(c)
            {
                h.status = format!("Init:{}", h.status);
                return h;
            }
        }
        for c in array(v, "/status/containerStatuses") {
            if let Some(h) = failure(c) {
                return h;
            }
        }
        if cond(v, "PodScheduled", "False") {
            return Health::new(
                condition(v, "PodScheduled")
                    .and_then(|c| c["reason"].as_str())
                    .unwrap_or("Unschedulable"),
                Warning,
            );
        }
        if !initialized && !array(v, "/status/initContainerStatuses").is_empty() {
            let all = array(v, "/spec/initContainers")
                .iter()
                .filter(|c| c["restartPolicy"] != "Always")
                .count();
            let complete = array(v, "/status/initContainerStatuses")
                .iter()
                .filter(|c| c.pointer("/state/terminated/exitCode") == Some(&Value::from(0)))
                .count();
            return Health::new(format!("Init:{complete}/{all}"), Warning);
        }
        return Health::new(
            phase,
            if phase == "Running" && cond(v, "Ready", "True") {
                Healthy
            } else if phase == "Unknown" {
                Unknown
            } else {
                Warning
            },
        );
    }
    if api == "apps/v1" && ["Deployment", "StatefulSet", "ReplicaSet", "DaemonSet"].contains(&kind)
    {
        let daemon = kind == "DaemonSet";
        let desired = if daemon {
            number(v, "/status/desiredNumberScheduled")
        } else {
            v.pointer("/spec/replicas")
                .and_then(Value::as_i64)
                .unwrap_or(1)
        };
        let ready = number(
            v,
            if daemon {
                "/status/numberReady"
            } else {
                "/status/readyReplicas"
            },
        );
        if desired == 0 {
            return Health::new(
                if daemon {
                    "NoEligibleNodes"
                } else {
                    "ScaledDown"
                },
                if daemon { Unknown } else { Healthy },
            );
        }
        if cond(v, "Progressing", "False") {
            return Health::new("Stalled", Critical);
        }
        if cond(v, "ReplicaFailure", "True") {
            return Health::new("Degraded", Critical);
        }
        if ready == 0 {
            return Health::new("Unavailable", Critical);
        }
        if number(v, "/status/observedGeneration") < number(v, "/metadata/generation")
            || ready < desired
        {
            return Health::new("Progressing", Warning);
        }
        if kind == "Deployment" && number(v, "/status/updatedReplicas") < desired {
            return Health::new("Progressing", Warning);
        }
        if kind == "DaemonSet" && number(v, "/status/updatedNumberScheduled") < desired {
            return Health::new("Progressing", Warning);
        }
        if kind == "StatefulSet" && scalar(v, "/spec/updateStrategy/type") != "OnDelete" {
            let target =
                desired - number(v, "/spec/updateStrategy/rollingUpdate/partition").min(desired);
            if number(v, "/status/updatedReplicas") < target {
                return Health::new("Progressing", Warning);
            }
        }
        return Health::new("Ready", Healthy);
    }
    if api == "batch/v1" && kind == "Job" {
        if cond(v, "Failed", "True") || cond(v, "FailureTarget", "True") {
            return Health::new("Failed", Critical);
        }
        if cond(v, "Complete", "True") {
            return Health::new("Completed", Healthy);
        }
        if v.pointer("/spec/suspend") == Some(&Value::Bool(true)) {
            return Health::new("Suspended", Warning);
        }
        return Health::new(
            if number(v, "/status/active") > 0 {
                "Running"
            } else {
                "Pending"
            },
            Warning,
        );
    }
    if api == "v1" && kind == "Node" {
        let cordoned = v.pointer("/spec/unschedulable") == Some(&Value::Bool(true));
        for name in [
            "MemoryPressure",
            "DiskPressure",
            "PIDPressure",
            "NetworkUnavailable",
        ] {
            if cond(v, name, "True") || cond(v, name, "Unknown") {
                return Health::new(name, Warning);
            }
        }
        let ready = cond(v, "Ready", "True");
        return Health::new(
            format!(
                "{}{}",
                if ready { "Ready" } else { "NotReady" },
                if cordoned { ",SchedulingDisabled" } else { "" }
            ),
            if ready { Healthy } else { Critical },
        );
    }
    if api == "v1" && ["PersistentVolumeClaim", "PersistentVolume", "Namespace"].contains(&kind) {
        let phase = scalar(v, "/status/phase");
        return Health::new(
            &phase,
            match phase.as_str() {
                "Bound" | "Active" | "Available" => Healthy,
                "Lost" | "Failed" => Critical,
                _ => Warning,
            },
        );
    }
    for name in ["Ready", "Available", "Established"] {
        if let Some(c) = condition(v, name) {
            return Health::new(
                format!("{name}={}", scalar(c, "/status")),
                match c["status"].as_str() {
                    Some("True") => Healthy,
                    Some("False") => Warning,
                    _ => Unknown,
                },
            );
        }
    }
    Health::new("Unknown", Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn pod_reason_and_gate_precedence() {
        let mut p = json!({"apiVersion":"v1","kind":"Pod","spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","containerStatuses":[{"name":"c","ready":true,"state":{"waiting":{"reason":"CrashLoopBackOff"}}}]}});
        assert_eq!(derive(&p).status, "CrashLoopBackOff");
        p["status"]["containerStatuses"][0]["state"] = json!({"running":{}});
        assert_eq!(derive(&p).severity, Severity::Warning);
        p["status"]["conditions"] = json!([{"type":"Ready","status":"True"}]);
        assert_eq!(derive(&p).severity, Severity::Healthy);
        p["metadata"] = json!({"deletionTimestamp":"2026-01-01T00:00:00Z"});
        assert_eq!(derive(&p).status, "Terminating");
    }
    #[test]
    fn pressure_false_is_not_a_fault_and_unknown_is_unknown() {
        let n = json!({"apiVersion":"v1","kind":"Node","status":{"conditions":[{"type":"MemoryPressure","status":"False"},{"type":"Ready","status":"True"}]}});
        assert_eq!(derive(&n).severity, Severity::Healthy);
        assert_eq!(
            derive(&json!({"apiVersion":"other/v1","kind":"Pod"})).severity,
            Severity::Unknown
        );
    }
    #[test]
    fn rollout_not_observed_is_progressing() {
        let d = json!({"apiVersion":"apps/v1","kind":"Deployment","metadata":{"generation":3},"spec":{"replicas":2},"status":{"observedGeneration":2,"readyReplicas":2,"updatedReplicas":2}});
        assert_eq!(derive(&d).status, "Progressing");
    }
}
