//! Deterministic health: a pure function over already-known fields. No AI, no
//! fuzzy scoring, no hidden weighting -- every result traces to concrete
//! evidence lines citing the fields that produced it. Absent/missing critical
//! evidence is `Unknown`, never `Healthy` by default.
//!
//! Precedence (the actual design question this module exists to answer):
//! deletion, then terminal failure, then container failure, then scheduling/
//! init problems, then readiness degradation, then progressing, then healthy,
//! then unknown. This governs every resource kind below, adapted to what
//! "terminal"/"progressing" mean for that kind (a Pod's terminal state is its
//! own phase; a workload's is a stalled/failed rollout condition; a Job's is
//! a Failed condition).
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
    /// Bounded, human-readable evidence lines citing the actual fields/values
    /// that produced this result. Empty for Healthy/Unknown, where there is
    /// nothing to explain. Never a diagnosis or inferred root cause -- only
    /// what was directly observed.
    pub evidence: Vec<String>,
}
impl Health {
    fn new(status: impl Into<String>, severity: Severity) -> Self {
        Self {
            status: status.into(),
            severity,
            evidence: Vec::new(),
        }
    }
    fn with(status: impl Into<String>, severity: Severity, evidence: Vec<String>) -> Self {
        Self {
            status: status.into(),
            severity,
            evidence,
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

/// A genuine container failure (crash loop, image pull error, nonzero exit).
/// `ContainerCreating`/`PodInitializing` are ordinary startup, not failures --
/// conflating them would let a transient "just starting" state preempt a real
/// failure elsewhere, or worse, be reported as Critical for doing nothing wrong.
fn container_failure(c: &Value) -> Option<(String, Severity, Vec<String>)> {
    let name = c["name"].as_str().unwrap_or("?");
    let restarts = c.get("restartCount").and_then(Value::as_i64);
    let last_terminated = c
        .pointer("/lastState/terminated/reason")
        .and_then(Value::as_str);
    if let Some(reason) = c.pointer("/state/waiting/reason").and_then(Value::as_str) {
        if matches!(reason, "ContainerCreating" | "PodInitializing") {
            return None;
        }
        let mut evidence = vec![format!("container {name} waiting: {reason}")];
        if let Some(message) = c.pointer("/state/waiting/message").and_then(Value::as_str) {
            evidence.push(format!("message: {message}"));
        }
        if let Some(n) = restarts {
            evidence.push(format!("restartCount: {n}"));
        }
        if let Some(last) = last_terminated {
            evidence.push(format!("last termination: {last}"));
        }
        return Some((reason.to_owned(), Severity::Critical, evidence));
    }
    if let Some(t) = c.pointer("/state/terminated") {
        let code = number(t, "/exitCode");
        if code != 0 {
            let reason = t["reason"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("ExitCode:{code}"));
            let mut evidence = vec![format!(
                "container {name} terminated: {reason} (exit code {code})"
            )];
            if let Some(n) = restarts {
                evidence.push(format!("restartCount: {n}"));
            }
            return Some((reason, Severity::Critical, evidence));
        }
    }
    None
}

fn pod_health(v: &Value) -> Health {
    use Severity::*;
    let phase = v
        .pointer("/status/phase")
        .and_then(Value::as_str)
        .unwrap_or("Unknown");
    if phase == "Failed" {
        // Terminal failure outranks container-level detail, but the specific
        // container evidence is still attached, not discarded.
        let mut evidence: Vec<String> = array(v, "/status/initContainerStatuses")
            .iter()
            .chain(array(v, "/status/containerStatuses"))
            .filter_map(|c| container_failure(c).map(|(_, _, ev)| ev))
            .flatten()
            .collect();
        if let Some(message) = v.pointer("/status/message").and_then(Value::as_str) {
            evidence.push(format!("message: {message}"));
        }
        return Health::with(
            v.pointer("/status/reason")
                .and_then(Value::as_str)
                .unwrap_or("Failed"),
            Critical,
            evidence,
        );
    }
    if phase == "Succeeded" {
        return Health::new("Completed", Healthy);
    }
    let initialized = cond(v, "Initialized", "True");
    // Container failure: regular containers first (most actionable), then any
    // init container that is still relevant right now -- either a genuinely
    // in-progress (non-restartable) init container, or a restartable sidecar,
    // which runs for the Pod's whole lifetime and can fail long after Initialized.
    for c in array(v, "/status/containerStatuses") {
        if let Some((reason, severity, evidence)) = container_failure(c) {
            return Health::with(reason, severity, evidence);
        }
    }
    for c in array(v, "/status/initContainerStatuses") {
        let name = c["name"].as_str().unwrap_or_default();
        if (!initialized || is_sidecar(v, name))
            && let Some((reason, severity, mut evidence)) = container_failure(c)
        {
            evidence.insert(0, format!("init container {name}"));
            return Health::with(format!("Init:{reason}"), severity, evidence);
        }
    }
    if cond(v, "PodScheduled", "False") {
        let evidence = condition(v, "PodScheduled")
            .and_then(|c| c["message"].as_str())
            .map(|m| vec![format!("message: {m}")])
            .unwrap_or_default();
        return Health::with(
            condition(v, "PodScheduled")
                .and_then(|c| c["reason"].as_str())
                .unwrap_or("Unschedulable"),
            Warning,
            evidence,
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
    if phase == "Running" {
        if cond(v, "Ready", "True") {
            return Health::new("Running", Healthy);
        }
        // Readiness degradation: name which conditions/containers are not
        // ready rather than a bare "NotReady" with nothing to point at.
        let mut evidence: Vec<String> = array(v, "/status/conditions")
            .iter()
            .filter(|c| c["status"] != "True")
            .filter_map(|c| {
                Some(format!(
                    "condition {}={}",
                    c["type"].as_str()?,
                    c["status"].as_str().unwrap_or("Unknown")
                ))
            })
            .collect();
        for c in array(v, "/status/containerStatuses") {
            if c["ready"] != Value::Bool(true) {
                evidence.push(format!(
                    "container {} not ready",
                    c["name"].as_str().unwrap_or("?")
                ));
            }
        }
        return Health::with("NotReady", Warning, evidence);
    }
    Health::new(phase, if phase == "Unknown" { Unknown } else { Warning })
}

fn workload_health(v: &Value, kind: &str) -> Health {
    use Severity::*;
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
        let evidence = condition(v, "Progressing")
            .and_then(|c| c["message"].as_str())
            .map(|m| vec![format!("message: {m}")])
            .unwrap_or_default();
        return Health::with("Stalled", Critical, evidence);
    }
    if cond(v, "ReplicaFailure", "True") {
        let evidence = condition(v, "ReplicaFailure")
            .and_then(|c| c["message"].as_str())
            .map(|m| vec![format!("message: {m}")])
            .unwrap_or_default();
        return Health::with("Degraded", Critical, evidence);
    }
    if ready == 0 {
        return Health::with(
            "Unavailable",
            Critical,
            vec![format!("readyReplicas: 0 (desired {desired})")],
        );
    }
    let generation = number(v, "/metadata/generation");
    let observed = number(v, "/status/observedGeneration");
    // A controller that has not observed the latest spec generation is never
    // "healthy" purely because old replicas are still running -- it has not
    // reconciled the current desired state yet.
    if observed < generation {
        return Health::with(
            "Progressing",
            Warning,
            vec![format!(
                "metadata.generation: {generation}; status.observedGeneration: {observed}"
            )],
        );
    }
    if ready < desired {
        return Health::with(
            "Progressing",
            Warning,
            vec![format!("readyReplicas: {ready}/{desired}")],
        );
    }
    if kind == "Deployment" {
        let updated = number(v, "/status/updatedReplicas");
        if updated < desired {
            return Health::with(
                "Progressing",
                Warning,
                vec![format!("updatedReplicas: {updated}/{desired}")],
            );
        }
    }
    if kind == "DaemonSet" {
        let updated = number(v, "/status/updatedNumberScheduled");
        if updated < desired {
            return Health::with(
                "Progressing",
                Warning,
                vec![format!("updatedNumberScheduled: {updated}/{desired}")],
            );
        }
        let available = number(v, "/status/numberAvailable");
        if available < desired {
            return Health::with(
                "Progressing",
                Warning,
                vec![format!("numberAvailable: {available}/{desired}")],
            );
        }
    }
    if kind == "StatefulSet" {
        // OnDelete never auto-rolls; partition holds back the lowest-ordinal
        // Pods below the partition index deliberately, not as a stall.
        if scalar(v, "/spec/updateStrategy/type") != "OnDelete" {
            let partition =
                number(v, "/spec/updateStrategy/rollingUpdate/partition").clamp(0, desired);
            let target = desired - partition;
            let updated = number(v, "/status/updatedReplicas");
            if updated < target {
                return Health::with(
                    "Progressing",
                    Warning,
                    vec![format!(
                        "updatedReplicas: {updated}/{target} (partition {partition})"
                    )],
                );
            }
        }
        let current_revision = scalar(v, "/status/currentRevision");
        let update_revision = scalar(v, "/status/updateRevision");
        if !current_revision.is_empty()
            && !update_revision.is_empty()
            && current_revision != update_revision
            && scalar(v, "/spec/updateStrategy/type") == "OnDelete"
        {
            // OnDelete: replicas will not move to updateRevision until manually
            // deleted. Not a failure, but worth surfacing as still-progressing
            // evidence rather than claiming full convergence.
            return Health::with(
                "Progressing",
                Warning,
                vec![format!(
                    "currentRevision: {current_revision}; updateRevision: {update_revision} (OnDelete: waiting for manual Pod deletion)"
                )],
            );
        }
    }
    Health::new("Ready", Healthy)
}

fn job_health(v: &Value) -> Health {
    use Severity::*;
    if cond(v, "Failed", "True") || cond(v, "FailureTarget", "True") {
        let evidence = condition(v, "Failed")
            .or_else(|| condition(v, "FailureTarget"))
            .and_then(|c| c["message"].as_str())
            .map(|m| vec![format!("message: {m}")])
            .unwrap_or_default();
        return Health::with("Failed", Critical, evidence);
    }
    // A completed Job is healthy regardless of how many attempts it took --
    // success is success, backoff history is not itself a fault once done.
    if cond(v, "Complete", "True") {
        return Health::new("Completed", Healthy);
    }
    if v.pointer("/spec/suspend") == Some(&Value::Bool(true)) {
        return Health::new("Suspended", Warning);
    }
    let failed = number(v, "/status/failed");
    if failed > 0 {
        return Health::with(
            "Retrying",
            Warning,
            vec![format!("status.failed: {failed} (backoff, not yet Failed)")],
        );
    }
    Health::new(
        if number(v, "/status/active") > 0 {
            "Running"
        } else {
            "Pending"
        },
        Warning,
    )
}

fn node_health(v: &Value) -> Health {
    use Severity::*;
    let cordoned = v.pointer("/spec/unschedulable") == Some(&Value::Bool(true));
    // Condition polarity is not uniform: Ready=True is good; the pressure/
    // network conditions are bad when True (and Unknown stays Unknown, never
    // silently treated as either good or bad).
    for name in [
        "MemoryPressure",
        "DiskPressure",
        "PIDPressure",
        "NetworkUnavailable",
    ] {
        match condition(v, name).and_then(|c| c["status"].as_str()) {
            Some("True") => {
                return Health::with(name, Warning, vec![format!("condition {name}=True")]);
            }
            Some("Unknown") => {
                return Health::with(name, Unknown, vec![format!("condition {name}=Unknown")]);
            }
            _ => {}
        }
    }
    match condition(v, "Ready").and_then(|c| c["status"].as_str()) {
        Some("True") => Health::new(
            if cordoned {
                "Ready,SchedulingDisabled"
            } else {
                "Ready"
            },
            Healthy,
        ),
        Some("Unknown") | None => Health::new("Unknown", Unknown),
        _ => {
            let evidence = condition(v, "Ready")
                .and_then(|c| c["message"].as_str())
                .map(|m| vec![format!("message: {m}")])
                .unwrap_or_default();
            Health::with(
                if cordoned {
                    "NotReady,SchedulingDisabled"
                } else {
                    "NotReady"
                },
                Critical,
                evidence,
            )
        }
    }
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
        return pod_health(v);
    }
    if api == "apps/v1" && ["Deployment", "StatefulSet", "ReplicaSet", "DaemonSet"].contains(&kind)
    {
        return workload_health(v, kind);
    }
    if api == "batch/v1" && kind == "Job" {
        return job_health(v);
    }
    if api == "v1" && kind == "Node" {
        return node_health(v);
    }
    if api == "v1" && ["PersistentVolumeClaim", "PersistentVolume", "Namespace"].contains(&kind) {
        let phase = scalar(v, "/status/phase");
        let severity = match phase.as_str() {
            "Bound" | "Active" | "Available" => Healthy,
            "Lost" | "Failed" => Critical,
            _ => Warning,
        };
        // Not diagnosing a cause here (that needs correlation work M6 owns) --
        // just citing the phase/message actually reported, for Lost/Failed only.
        let evidence = (severity == Critical)
            .then(|| {
                v.pointer("/status/message")
                    .and_then(Value::as_str)
                    .map(|m| vec![format!("status.phase: {phase}; message: {m}")])
            })
            .flatten()
            .unwrap_or_default();
        return Health::with(&phase, severity, evidence);
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
        assert!(
            derive(&p)
                .evidence
                .iter()
                .any(|e| e.contains("waiting: CrashLoopBackOff"))
        );
        p["status"]["containerStatuses"][0]["state"] = json!({"running":{}});
        assert_eq!(derive(&p).severity, Severity::Warning);
        p["status"]["conditions"] = json!([{"type":"Ready","status":"True"}]);
        assert_eq!(derive(&p).severity, Severity::Healthy);
        p["metadata"] = json!({"deletionTimestamp":"2026-01-01T00:00:00Z"});
        assert_eq!(derive(&p).status, "Terminating");
    }
    #[test]
    fn container_creating_is_not_a_failure_and_oom_is_evidence_from_last_state() {
        let p = json!({"apiVersion":"v1","kind":"Pod","spec":{"containers":[{"name":"c"}]},"status":{"phase":"Pending","containerStatuses":[{"name":"c","ready":false,"restartCount":3,"state":{"waiting":{"reason":"ContainerCreating"}},"lastState":{"terminated":{"reason":"OOMKilled","exitCode":137}}}]}});
        let h = derive(&p);
        assert_eq!(h.status, "Pending");
        assert_eq!(h.severity, Severity::Warning);
        let mut crashed = p.clone();
        crashed["status"]["containerStatuses"][0]["state"] =
            json!({"waiting":{"reason":"CrashLoopBackOff"}});
        let h = derive(&crashed);
        assert_eq!(h.status, "CrashLoopBackOff");
        assert!(
            h.evidence
                .iter()
                .any(|e| e.contains("last termination: OOMKilled"))
        );
        assert!(h.evidence.iter().any(|e| e.contains("restartCount: 3")));
    }
    #[test]
    fn init_container_failure_is_distinct_from_a_running_sidecar_crash() {
        let init_failing = json!({"apiVersion":"v1","kind":"Pod","spec":{"containers":[{"name":"app"}],"initContainers":[{"name":"setup"}]},"status":{"phase":"Pending","conditions":[{"type":"Initialized","status":"False"}],"initContainerStatuses":[{"name":"setup","state":{"waiting":{"reason":"CrashLoopBackOff"}}}]}});
        let h = derive(&init_failing);
        assert_eq!(h.status, "Init:CrashLoopBackOff");
        assert_eq!(h.severity, Severity::Critical);
        let sidecar_crash = json!({"apiVersion":"v1","kind":"Pod","spec":{"containers":[{"name":"app"}],"initContainers":[{"name":"proxy","restartPolicy":"Always"}]},"status":{"phase":"Running","conditions":[{"type":"Initialized","status":"True"},{"type":"Ready","status":"False"}],"containerStatuses":[{"name":"app","ready":true,"state":{"running":{}}}],"initContainerStatuses":[{"name":"proxy","state":{"waiting":{"reason":"CrashLoopBackOff"}}}]}});
        let h = derive(&sidecar_crash);
        assert_eq!(h.status, "Init:CrashLoopBackOff");
        assert_eq!(h.severity, Severity::Critical);
    }
    #[test]
    fn unschedulable_and_not_ready_carry_evidence() {
        let unschedulable = json!({"apiVersion":"v1","kind":"Pod","spec":{"containers":[{"name":"c"}]},"status":{"phase":"Pending","conditions":[{"type":"PodScheduled","status":"False","reason":"Unschedulable","message":"0/1 nodes are available"}]}});
        let h = derive(&unschedulable);
        assert_eq!(h.status, "Unschedulable");
        assert!(h.evidence.iter().any(|e| e.contains("0/1 nodes")));
        let not_ready = json!({"apiVersion":"v1","kind":"Pod","spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","conditions":[{"type":"Ready","status":"False"}],"containerStatuses":[{"name":"c","ready":false,"state":{"running":{}}}]}});
        let h = derive(&not_ready);
        assert_eq!(h.status, "NotReady");
        assert!(
            h.evidence
                .iter()
                .any(|e| e.contains("container c not ready"))
        );
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
    fn node_condition_polarity_and_unknown_status() {
        let pressure = json!({"apiVersion":"v1","kind":"Node","status":{"conditions":[{"type":"DiskPressure","status":"True"},{"type":"Ready","status":"True"}]}});
        let h = derive(&pressure);
        assert_eq!(h.status, "DiskPressure");
        assert_eq!(h.severity, Severity::Warning);
        let unknown_ready = json!({"apiVersion":"v1","kind":"Node","status":{"conditions":[{"type":"Ready","status":"Unknown"}]}});
        assert_eq!(derive(&unknown_ready).severity, Severity::Unknown);
    }
    #[test]
    fn rollout_not_observed_is_progressing() {
        let d = json!({"apiVersion":"apps/v1","kind":"Deployment","metadata":{"generation":3},"spec":{"replicas":2},"status":{"observedGeneration":2,"readyReplicas":2,"updatedReplicas":2}});
        let h = derive(&d);
        assert_eq!(h.status, "Progressing");
        assert!(h.evidence.iter().any(|e| e.contains("generation: 3")));
    }
    #[test]
    fn statefulset_partition_and_ondelete_are_not_stalls() {
        let partitioned = json!({"apiVersion":"apps/v1","kind":"StatefulSet","metadata":{"generation":1},"spec":{"replicas":3,"updateStrategy":{"type":"RollingUpdate","rollingUpdate":{"partition":2}}},"status":{"observedGeneration":1,"readyReplicas":3,"updatedReplicas":1}});
        assert_eq!(derive(&partitioned).status, "Ready");
        let ondelete = json!({"apiVersion":"apps/v1","kind":"StatefulSet","metadata":{"generation":1},"spec":{"replicas":2,"updateStrategy":{"type":"OnDelete"}},"status":{"observedGeneration":1,"readyReplicas":2,"updatedReplicas":2,"currentRevision":"r1","updateRevision":"r2"}});
        let h = derive(&ondelete);
        assert_eq!(h.status, "Progressing");
        assert!(h.evidence.iter().any(|e| e.contains("OnDelete")));
    }
    #[test]
    fn daemonset_uses_its_own_replica_fields() {
        let d = json!({"apiVersion":"apps/v1","kind":"DaemonSet","metadata":{"generation":1},"status":{"desiredNumberScheduled":3,"numberReady":3,"observedGeneration":1,"updatedNumberScheduled":3,"numberAvailable":2}});
        let h = derive(&d);
        assert_eq!(h.status, "Progressing");
        assert!(h.evidence.iter().any(|e| e.contains("numberAvailable")));
    }
    #[test]
    fn job_completion_is_healthy_and_backoff_is_distinct_from_failed() {
        let completed = json!({"apiVersion":"batch/v1","kind":"Job","status":{"conditions":[{"type":"Complete","status":"True"}],"failed":2}});
        assert_eq!(derive(&completed).status, "Completed");
        assert_eq!(derive(&completed).severity, Severity::Healthy);
        let retrying =
            json!({"apiVersion":"batch/v1","kind":"Job","status":{"failed":1,"active":1}});
        let h = derive(&retrying);
        assert_eq!(h.status, "Retrying");
        assert_eq!(h.severity, Severity::Warning);
        let failed = json!({"apiVersion":"batch/v1","kind":"Job","status":{"conditions":[{"type":"Failed","status":"True","message":"BackoffLimitExceeded"}]}});
        let h = derive(&failed);
        assert_eq!(h.status, "Failed");
        assert_eq!(h.severity, Severity::Critical);
        assert!(
            h.evidence
                .iter()
                .any(|e| e.contains("BackoffLimitExceeded"))
        );
    }
}
