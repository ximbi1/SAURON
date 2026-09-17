//! Explain = health findings + fresh evidence + bounded correlation. This
//! reuses `resources::health`'s deterministic rules and their cited evidence
//! directly -- it is not a second, parallel diagnosis. Metrics are attached
//! as descriptive evidence, never a claimed cause. Related Pods come only
//! from verified ownership, never a name/label heuristic.
use crate::evidence::Unknown;
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

fn health_finding(object: &Object, affected: &str, prefix: &str) -> Option<Finding> {
    (object.health.severity >= Severity::Warning).then(|| Finding {
        severity: object.health.severity,
        finding: format!("{prefix}observed status: {}", object.health.status),
        evidence: if object.health.evidence.is_empty() {
            format!(
                "apiVersion={} resourceVersion={}",
                object.api_version, object.version
            )
        } else {
            object.health.evidence.join("; ")
        },
        affected: affected.to_owned(),
        next: "Inspect the cited fields directly; this reuses the same deterministic health \
               rules shown in the table, not a separate diagnosis"
            .into(),
    })
}

pub fn explain(
    object: &Object,
    events: &[Value],
    children: &[Object],
    metrics: Option<(Result<f64, Unknown>, Result<f64, Unknown>)>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut has_fault = false;
    let affected = format!("{}/{} ({})", object.kind, object.name, object.uid);
    if let Some(finding) = health_finding(object, &affected, "") {
        has_fault = true;
        out.push(finding);
    }
    let mut unhealthy_children = 0;
    for child in children {
        let child_affected = format!("Pod/{} ({})", child.name, child.uid);
        if let Some(mut finding) = health_finding(child, &child_affected, "owned Pod ") {
            finding.next = "Inspect this Pod directly (logs, YAML) for more detail".into();
            out.push(finding);
            has_fault = true;
            unhealthy_children += 1;
        }
    }
    // Purely descriptive -- never counts toward "fault found", so a Pod with
    // real usage but no failure evidence still gets the honest "not proven
    // healthy" disclaimer below rather than looking like a closed case.
    if let Some((cpu, memory)) = metrics.filter(|_| matches!(object.kind.as_str(), "Pod" | "Node"))
    {
        let text = |v: Result<f64, Unknown>, unit: &str| match v {
            Ok(n) => format!("{n} {unit}"),
            Err(reason) => format!("UNKNOWN ({reason})"),
        };
        out.push(Finding {
            severity: Severity::Healthy,
            finding: "Metrics (descriptive evidence, not a diagnosed cause)".into(),
            evidence: format!(
                "CPU: {}; Memory: {}",
                text(cpu, "cores"),
                text(memory, "bytes")
            ),
            affected: affected.clone(),
            next: "Correlate with the table's CPU/MEM/%R/%L columns if this seems disproportionate"
                .into(),
        });
    }
    let warning_events: Vec<_> = events.iter().filter(|e| e["type"] == "Warning").collect();
    has_fault |= !warning_events.is_empty();
    for event in warning_events {
        out.push(Finding{severity:Severity::Warning,finding:format!("Warning Event: {}",event["reason"].as_str().unwrap_or("Unknown")),evidence:format!("Event/{}: {}",event.pointer("/metadata/name").and_then(Value::as_str).unwrap_or("?"),event["message"].as_str().unwrap_or("No message")),affected:affected.clone(),next:"Inspect the Event timestamp/count; event text is evidence, not proof of a current cause".into()});
    }
    if !has_fault {
        let checked = if children.is_empty() {
            String::new()
        } else {
            format!("; {} owned Pod(s) checked, none unhealthy", children.len())
        };
        out.push(Finding{severity:Severity::Unknown,finding:"No failure evidence found in the collected resource and Events".into(),evidence:format!("resourceVersion={}; {} related Event(s) read{checked}",object.version,events.len()),affected,next:"This does not prove the resource is healthy; inspect logs, dependencies and controller state".into()});
    } else if !children.is_empty() && unhealthy_children == 0 {
        out.push(Finding {
            severity: Severity::Unknown,
            finding: format!("{} owned Pod(s) checked, none unhealthy", children.len()),
            evidence: "Ownership verified via ownerReferences, not name/label heuristics".into(),
            affected,
            next: "Not proof the workload is otherwise fine -- see the findings above".into(),
        });
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.severity));
    out
}

pub fn report(
    object: &Object,
    events: &[Value],
    warnings: &[String],
    children: &[Object],
    metrics: Option<(Result<f64, Unknown>, Result<f64, Unknown>)>,
) -> String {
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
    for f in explain(object, events, children, metrics) {
        out.push_str(&format!(
            "{:?}: {}\nEvidence: {}\nAffected: {}\nNext inspection: {}\n\n",
            f.severity, f.finding, f.evidence, f.affected, f.next
        ));
    }
    out.push_str(
        "Scope: selected object, verified-ownership child Pods (workloads only), and \
         UID-related Events. Nodes, storage and cross-kind relationships are not \
         correlated yet -- see M6.\n",
    );
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
        let findings = explain(&object, &[], &[], None);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Unknown);
    }
    #[test]
    fn health_evidence_is_reused_verbatim_not_rederived() {
        let object = Object::new(
            serde_json::json!({"kind":"Pod","apiVersion":"v1","spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","containerStatuses":[{"name":"c","ready":false,"restartCount":3,"state":{"waiting":{"reason":"CrashLoopBackOff"}},"lastState":{"terminated":{"reason":"OOMKilled","exitCode":137}}}]}}),
        );
        let findings = explain(&object, &[], &[], None);
        let top = &findings[0];
        assert_eq!(top.severity, Severity::Critical);
        assert!(top.evidence.contains("last termination: OOMKilled"));
        assert!(top.evidence.contains("restartCount: 3"));
    }
    #[test]
    fn unhealthy_child_pod_surfaces_and_healthy_children_are_noted_not_hidden() {
        let workload = Object::new(
            serde_json::json!({"kind":"StatefulSet","apiVersion":"apps/v1","metadata":{"generation":1},"spec":{"replicas":1},"status":{"observedGeneration":1,"readyReplicas":1,"updatedReplicas":1}}),
        );
        let crashing_child = Object::new(
            serde_json::json!({"kind":"Pod","apiVersion":"v1","metadata":{"name":"p-0","uid":"pod-uid"},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","containerStatuses":[{"name":"c","ready":false,"state":{"waiting":{"reason":"CrashLoopBackOff"}}}]}}),
        );
        let findings = explain(&workload, &[], &[crashing_child], None);
        assert!(
            findings
                .iter()
                .any(|f| f.finding.contains("owned Pod") && f.severity == Severity::Critical)
        );
        // A workload that is itself unhealthy (Progressing: 1/2 ready) but whose
        // one checked Pod looks fine must still note that the child was checked,
        // not silently omit it just because there was nothing wrong to report.
        let progressing = Object::new(
            serde_json::json!({"kind":"StatefulSet","apiVersion":"apps/v1","metadata":{"generation":1},"spec":{"replicas":2},"status":{"observedGeneration":1,"readyReplicas":1,"updatedReplicas":1}}),
        );
        let healthy_child = Object::new(
            serde_json::json!({"kind":"Pod","apiVersion":"v1","metadata":{"name":"p-0","uid":"pod-uid"},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","conditions":[{"type":"Ready","status":"True"}],"containerStatuses":[{"name":"c","ready":true,"state":{"running":{}}}]}}),
        );
        let findings = explain(&progressing, &[], &[healthy_child], None);
        assert!(
            findings
                .iter()
                .any(|f| f.finding.contains("owned Pod(s) checked, none unhealthy"))
        );
    }
    #[test]
    fn metrics_are_descriptive_not_a_severity_signal() {
        let object = Object::new(
            serde_json::json!({"kind":"Pod","apiVersion":"v1","status":{"phase":"Running","conditions":[{"type":"Ready","status":"True"}]}}),
        );
        let findings = explain(&object, &[], &[], Some((Ok(0.5), Err(Unknown::Forbidden))));
        let metrics_finding = findings
            .iter()
            .find(|f| f.finding.starts_with("Metrics"))
            .expect("metrics finding present");
        assert_eq!(metrics_finding.severity, Severity::Healthy);
        assert!(metrics_finding.evidence.contains("0.5 cores"));
        assert!(metrics_finding.evidence.contains("UNKNOWN (Forbidden)"));
    }
}
