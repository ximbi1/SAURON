//! Meaningful field-delta extraction shared by both the incremental watch
//! path and the relist-diff path in `store.rs`. Never records every
//! `resourceVersion` change -- only fields that actually carry meaning for
//! understanding what happened to an object.
use super::{Object, health};
use serde_json::Value;

fn display(v: Option<&Value>) -> String {
    match v {
        None => "absent".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn container_reason(c: &Value) -> Option<String> {
    c.pointer("/state/waiting/reason")
        .or_else(|| c.pointer("/state/terminated/reason"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn container_changes(old: &Value, new: &Value, array: &str) -> Vec<String> {
    let list = |v: &Value| {
        v.pointer(&format!("/status/{array}"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let (old_list, new_list) = (list(old), list(new));
    let mut out = Vec::new();
    for new_c in &new_list {
        let Some(name) = new_c["name"].as_str() else {
            continue;
        };
        let old_c = old_list.iter().find(|c| c["name"] == new_c["name"]);
        let old_reason = old_c.and_then(container_reason);
        let new_reason = container_reason(new_c);
        if old_reason != new_reason {
            out.push(format!(
                "container {name}: {} → {}",
                old_reason.as_deref().unwrap_or("running/ready"),
                new_reason.as_deref().unwrap_or("running/ready")
            ));
        }
    }
    out
}

fn image_changes(old: &Value, new: &Value) -> Vec<String> {
    let images = |v: &Value| -> Vec<(String, String)> {
        v.pointer("/spec/containers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|c| {
                Some((
                    c["name"].as_str()?.to_owned(),
                    c["image"].as_str().unwrap_or("").to_owned(),
                ))
            })
            .collect()
    };
    let (old_images, new_images) = (images(old), images(new));
    new_images
        .iter()
        .filter_map(|(name, new_image)| {
            let old_image = old_images.iter().find(|(n, _)| n == name)?;
            (old_image.1 != *new_image)
                .then(|| format!("image {name}: {} → {new_image}", old_image.1))
        })
        .collect()
}

/// Every field compared here is deliberately curated, not a generic deep
/// diff -- resourceVersion churn with no meaningful change produces nothing.
pub fn meaningful_diff(old: &Object, new: &Object) -> Vec<String> {
    let mut changes = Vec::new();
    if old.health.status != new.health.status {
        changes.push(format!(
            "health: {} → {}",
            old.health.status, new.health.status
        ));
    }
    for path in [
        "/status/phase",
        "/metadata/generation",
        "/status/observedGeneration",
        "/spec/replicas",
        "/status/readyReplicas",
        "/status/availableReplicas",
        "/status/updatedReplicas",
        "/status/desiredNumberScheduled",
        "/status/numberReady",
        "/status/numberAvailable",
    ] {
        let (a, b) = (old.value.pointer(path), new.value.pointer(path));
        if a != b {
            changes.push(format!("{path}: {} → {}", display(a), display(b)));
        }
    }
    let deleting = |o: &Object| {
        o.value
            .pointer("/metadata/deletionTimestamp")
            .is_some_and(|v| !v.is_null())
    };
    if !deleting(old) && deleting(new) {
        changes.push("deletionTimestamp: set".into());
    }
    if old.kind == "Pod" && new.kind == "Pod" {
        let (_, _, old_restarts) = health::pod_counts(&old.value);
        let (_, _, new_restarts) = health::pod_counts(&new.value);
        if old_restarts != new_restarts {
            let show = |r: Option<i64>| r.map_or("unknown".into(), |n| n.to_string());
            changes.push(format!(
                "restarts: {} → {}",
                show(old_restarts),
                show(new_restarts)
            ));
        }
        changes.extend(container_changes(
            &old.value,
            &new.value,
            "containerStatuses",
        ));
        changes.extend(container_changes(
            &old.value,
            &new.value,
            "initContainerStatuses",
        ));
        changes.extend(image_changes(&old.value, &new.value));
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pod(phase: &str, restarts: i64, reason: Option<&str>) -> Object {
        let state = match reason {
            Some(r) => json!({"waiting":{"reason":r}}),
            None => json!({"running":{}}),
        };
        Object::new(
            json!({"kind":"Pod","apiVersion":"v1","metadata":{"name":"p","uid":"u1"},"spec":{"containers":[{"name":"c","image":"busybox:1.37"}]},"status":{"phase":phase,"containerStatuses":[{"name":"c","restartCount":restarts,"state":state}]}}),
        )
    }

    #[test]
    fn identical_meaningful_fields_produce_no_changes() {
        let a = pod("Running", 0, None);
        let b = pod("Running", 0, None);
        assert!(meaningful_diff(&a, &b).is_empty());
    }
    #[test]
    fn phase_restart_and_reason_changes_are_each_cited() {
        let a = pod("Pending", 0, None);
        let b = pod("Running", 3, Some("CrashLoopBackOff"));
        let changes = meaningful_diff(&a, &b);
        assert!(
            changes
                .iter()
                .any(|c| c.contains("phase: Pending → Running"))
        );
        assert!(changes.iter().any(|c| c.contains("restarts: 0 → 3")));
        assert!(
            changes
                .iter()
                .any(|c| c.contains("container c: running/ready → CrashLoopBackOff"))
        );
    }
    #[test]
    fn image_change_is_cited_by_container_name() {
        let mut a = pod("Running", 0, None);
        let mut b = pod("Running", 0, None);
        a.value["spec"]["containers"][0]["image"] = json!("busybox:1.36");
        b.value["spec"]["containers"][0]["image"] = json!("busybox:1.37");
        assert!(
            meaningful_diff(&a, &b)
                .iter()
                .any(|c| c.contains("image c: busybox:1.36 → busybox:1.37"))
        );
    }
    #[test]
    fn deletion_timestamp_transition_is_one_way() {
        let mut a = pod("Running", 0, None);
        let mut b = pod("Running", 0, None);
        b.value["metadata"]["deletionTimestamp"] = json!("2026-01-01T00:00:00Z");
        assert!(
            meaningful_diff(&a, &b)
                .iter()
                .any(|c| c == "deletionTimestamp: set")
        );
        a.value["metadata"]["deletionTimestamp"] = json!("2026-01-01T00:00:00Z");
        b.value["metadata"]["deletionTimestamp"] = json!(null);
        assert!(meaningful_diff(&a, &b).is_empty());
    }
}
