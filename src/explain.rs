//! Pure deterministic findings. Every claim carries a path or Event reference.
use crate::resources::{Object, health::Severity};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Finding {
    pub severity: Severity,
    pub finding: String,
    pub evidence: String,
    pub affected: String,
    pub next: String,
}

pub fn explain(object: &Object, events: &[Value]) -> Vec<Finding> {
    let mut out = Vec::new();
    let affected = format!("{}/{} ({})", object.kind, object.name, object.uid);
    if object.health.severity >= Severity::Warning {
        out.push(Finding {
            severity: object.health.severity,
            finding: format!("Observed status: {}", object.health.status),
            evidence: format!(
                "apiVersion={} resourceVersion={}; status derived from resource status/conditions",
                object.api_version, object.version
            ),
            affected: affected.clone(),
            next: "Inspect conditions, container state and related Warning events below".into(),
        });
    }
    for array in [
        "containerStatuses",
        "initContainerStatuses",
        "ephemeralContainerStatuses",
    ] {
        if let Some(containers) = object
            .value
            .pointer(&format!("/status/{array}"))
            .and_then(Value::as_array)
        {
            for (index, c) in containers.iter().enumerate() {
                if c.pointer("/state/waiting").is_some()
                    || c.pointer("/lastState/terminated").is_some()
                    || c.pointer("/state/terminated/exitCode")
                        .and_then(Value::as_i64)
                        .is_some_and(|c| c != 0)
                {
                    let name = c["name"].as_str().unwrap_or("unknown");
                    let evidence = serde_json::json!({"state":c["state"],"lastState":c["lastState"],"restarts":c["restartCount"]});
                    out.push(Finding{severity:Severity::Warning,finding:format!("Container {name} has waiting or termination evidence"),evidence:format!("/status/{array}/{index}: {evidence}"),affected:affected.clone(),next:format!("Inspect logs for container {name}; previous logs may explain the last exit")});
                }
            }
        }
    }
    if let Some(conditions) = object
        .value
        .pointer("/status/conditions")
        .and_then(Value::as_array)
    {
        for (index, c) in conditions.iter().enumerate() {
            let name = c["type"].as_str().unwrap_or("Unknown");
            let positive = [
                "Ready",
                "Available",
                "Initialized",
                "PodScheduled",
                "ContainersReady",
                "Established",
                "Complete",
                "Progressing",
                "ResourcesUpToDate",
            ]
            .contains(&name);
            let negative = [
                "MemoryPressure",
                "DiskPressure",
                "PIDPressure",
                "NetworkUnavailable",
                "ReplicaFailure",
                "Failed",
                "Degraded",
                "Stalled",
                "ErrorOccurred",
            ]
            .contains(&name);
            let status = c["status"].as_str().unwrap_or("Unknown");
            if status == "Unknown"
                || (positive && status == "False")
                || (negative && status == "True")
            {
                out.push(Finding {
                    severity: Severity::Warning,
                    finding: format!("Condition {name}={status}"),
                    evidence: format!("/status/conditions/{index}: {c}"),
                    affected: affected.clone(),
                    next: "Inspect the condition reason/message and its last transition time"
                        .into(),
                });
            }
        }
    }
    for event in events.iter().filter(|e| e["type"] == "Warning") {
        out.push(Finding{severity:Severity::Warning,finding:format!("Warning Event: {}",event["reason"].as_str().unwrap_or("Unknown")),evidence:format!("Event/{}: {}",event.pointer("/metadata/name").and_then(Value::as_str).unwrap_or("?"),event["message"].as_str().unwrap_or("No message")),affected:affected.clone(),next:"Inspect the Event timestamp/count; event text is evidence, not proof of a current cause".into()});
    }
    if out.is_empty() {
        out.push(Finding{severity:Severity::Unknown,finding:"No failure evidence found in the collected resource and Events".into(),evidence:format!("resourceVersion={}; {} related Events read",object.version,events.len()),affected,next:"This does not prove the resource is healthy; inspect logs, dependencies and controller state".into()});
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.severity));
    out
}

pub fn report(object: &Object, events: &[Value], warnings: &[String]) -> String {
    let mut out = format!(
        "WHY: {}/{}\nObserved resourceVersion: {}\nEvidence collected at {}\n\n",
        object.kind,
        object.name,
        object.version,
        chrono::Utc::now().to_rfc3339()
    );
    if !warnings.is_empty() {
        out.push_str(&format!("PARTIAL EVIDENCE\n{}\n\n", warnings.join("\n")));
    }
    for f in explain(object, events) {
        out.push_str(&format!(
            "{:?}: {}\nEvidence: {}\nAffected: {}\nNext inspection: {}\n\n",
            f.severity, f.finding, f.evidence, f.affected, f.next
        ));
    }
    out.push_str("Scope: selected object and its UID-related Events. Child Pods, nodes and storage are not correlated yet.\n");
    crate::safety::text(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn healthy_node_pressure_false_produces_no_fault_finding() {
        let object = Object::new(
            serde_json::json!({"kind":"Node","apiVersion":"v1","status":{"conditions":[{"type":"Ready","status":"True"},{"type":"MemoryPressure","status":"False"}]}}),
        );
        let findings = explain(&object, &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Unknown);
    }
}
