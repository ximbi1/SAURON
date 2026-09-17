//! Deterministic Pod/Node resource accounting from spec/status fields alone --
//! no Metrics API involved. A container with no request/limit for a resource
//! contributes nothing to that resource's sum (matching `kubectl describe`),
//! which is distinct from the object failing to report anything at all.
//!
//! Effective Pod request/limit follows the documented init-container formula
//! (https://kubernetes.io/docs/concepts/workloads/pods/init-containers/#resources):
//! restartable ("sidecar", `restartPolicy: Always`) init containers run for the
//! Pod's whole lifetime and add to every other container's total; a regular
//! (sequential) init container's own request/limit is only compared against the
//! running total at its own position, never summed with other regular inits.
//!
//! QoS is read from `status.qosClass`, which Kubernetes itself computes and
//! stores -- re-deriving it here would be exactly the speculative accounting
//! this milestone forbids.
use crate::evidence::Unknown;
use serde_json::Value;

fn amount(container: &Value, field: &str, cpu: bool) -> Result<Option<f64>, Unknown> {
    let key = if cpu { "cpu" } else { "memory" };
    match container.pointer(&format!("/resources/{field}/{key}")) {
        None => Ok(None),
        Some(v) => v
            .as_str()
            .and_then(crate::filters::quantity)
            .map(Some)
            .ok_or(Unknown::Malformed),
    }
}
fn sum(containers: &[Value], field: &str, cpu: bool) -> Result<f64, Unknown> {
    let mut total = 0.0;
    for c in containers {
        total += amount(c, field, cpu)?.unwrap_or(0.0);
    }
    Ok(total)
}
fn is_restartable(container: &Value) -> bool {
    container.get("restartPolicy").and_then(Value::as_str) == Some("Always")
}

/// The Pod's effective request/limit for one resource (`field` is "requests" or
/// "limits", `cpu` selects cpu vs memory). `Unknown::NotReported` only when the
/// Pod has no container spec at all (should not happen for a real Pod object).
pub fn pod_effective(pod: &Value, field: &str, cpu: bool) -> Result<f64, Unknown> {
    let Some(containers) = pod.pointer("/spec/containers").and_then(Value::as_array) else {
        return Err(Unknown::NotReported);
    };
    let inits = pod
        .pointer("/spec/initContainers")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    // One pass: `running` accumulates only sidecars encountered so far, so it
    // equals the total sidecar sum once the loop ends; a sequential init is
    // compared against that running total at its own position, never against
    // other sequential inits (they never run concurrently with each other).
    let mut running = 0.0_f64;
    let mut max_sequential = 0.0_f64;
    for c in inits {
        if is_restartable(c) {
            running += amount(c, field, cpu)?.unwrap_or(0.0);
        } else {
            let effective = running + amount(c, field, cpu)?.unwrap_or(0.0);
            max_sequential = max_sequential.max(effective);
        }
    }
    let app_total = sum(containers, field, cpu)? + running;
    Ok(app_total.max(max_sequential))
}

/// `status.qosClass` verbatim; Unknown if the API has not populated it yet.
pub fn qos_class(pod: &Value) -> Result<&str, Unknown> {
    pod.pointer("/status/qosClass")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(Unknown::NotReported)
}

/// Node capacity or allocatable for one resource ("capacity" or "allocatable").
pub fn node_amount(node: &Value, field: &str, cpu: bool) -> Result<f64, Unknown> {
    let key = if cpu { "cpu" } else { "memory" };
    node.pointer(&format!("/status/{field}/{key}"))
        .and_then(Value::as_str)
        .and_then(crate::filters::quantity)
        .ok_or(Unknown::NotReported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn app_containers_sum_and_missing_request_contributes_nothing() {
        let pod = json!({"spec":{"containers":[
            {"resources":{"requests":{"cpu":"250m","memory":"64Mi"}}},
            {"resources":{"requests":{"cpu":"100m"}}},
        ]}});
        assert_eq!(pod_effective(&pod, "requests", true), Ok(0.35));
        assert_eq!(
            pod_effective(&pod, "requests", false),
            Ok(64.0 * 1024.0 * 1024.0)
        );
    }
    #[test]
    fn sidecar_init_adds_to_every_total_but_sequential_init_only_competes_at_its_own_position() {
        let pod = json!({"spec":{
            "containers":[{"resources":{"requests":{"cpu":"100m"}}}],
            "initContainers":[
                {"restartPolicy":"Always","resources":{"requests":{"cpu":"50m"}}},
                {"resources":{"requests":{"cpu":"500m"}}},
            ],
        }});
        // app(100m) + sidecar(50m) = 150m; sequential init effective = sidecar(50m)+500m = 550m; max = 550m.
        assert_eq!(pod_effective(&pod, "requests", true), Ok(0.55));
    }
    #[test]
    fn malformed_quantity_is_distinct_from_absent() {
        let pod =
            json!({"spec":{"containers":[{"resources":{"requests":{"cpu":"not-a-quantity"}}}]}});
        assert_eq!(
            pod_effective(&pod, "requests", true),
            Err(Unknown::Malformed)
        );
        let pod = json!({"spec":{"containers":[{}]}});
        assert_eq!(pod_effective(&pod, "requests", true), Ok(0.0));
    }
    #[test]
    fn qos_and_node_amounts_are_read_not_rederived() {
        assert_eq!(
            qos_class(&json!({"status":{"qosClass":"Burstable"}})),
            Ok("Burstable")
        );
        assert_eq!(qos_class(&json!({})), Err(Unknown::NotReported));
        let node = json!({"status":{"allocatable":{"cpu":"3800m","memory":"7Gi"}}});
        assert!((node_amount(&node, "allocatable", true).expect("cpu") - 3.8).abs() < 1e-9);
    }
}
