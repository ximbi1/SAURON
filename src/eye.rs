//! Eye = a priority-ordered view of the currently watched table, not a
//! second diagnosis engine. Ordering reuses `resources::priority`'s own
//! `Severity`-reversed sort verbatim; the "why" is `Health.evidence`
//! verbatim. Zero network: this is a pure rendering of whatever
//! `State::rows` already holds. See `docs/M11_ACCEPTANCE.md`'s M11.2
//! section.
use crate::adjacent::Target;
use crate::kube::discovery::Resource;
use crate::resources::{SharedObject, health::Severity, priority};

/// Explicit signal that the current scope may not be the whole truth --
/// bound hit, RBAC-forbidden list, or a watch that has not synced yet.
/// Eye must say so rather than silently rendering "all clear" over an
/// incomplete or stale set. Mirrors `evidence::Unknown`'s own
/// "distinguish, never erase" discipline without duplicating that enum --
/// these reasons are specific to *why this scope's row set itself* may be
/// incomplete, not why one value inside a row is unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeCaveat {
    NotYetSynced,
    Incomplete,
    UnknownFieldsExcluded(usize),
}
impl ScopeCaveat {
    fn text(self) -> String {
        match self {
            Self::NotYetSynced => "list not yet synchronized".into(),
            Self::Incomplete => "PARTIAL: object cache bound reached".into(),
            Self::UnknownFieldsExcluded(n) => {
                format!("PARTIAL: {n} row(s) excluded (unknown filter membership)")
            }
        }
    }
}

pub fn report(
    rows: &[SharedObject],
    resource: Option<&Resource>,
    caveats: &[ScopeCaveat],
) -> (String, Vec<Target>) {
    let ordered = priority::by_attention(rows);
    let counts = priority::severity_counts(rows);
    let mut text = format!(
        "EYE: {} row(s) -- {} critical, {} warning, {} unknown, {} healthy\n",
        counts.total(),
        counts.critical,
        counts.warning,
        counts.unknown,
        counts.healthy
    );
    if caveats.is_empty() {
        text.push_str("No scope caveats: this view reflects the full current selection.\n");
    } else {
        for caveat in caveats {
            text.push_str(&format!("CAVEAT: {}\n", caveat.text()));
        }
    }
    text.push('\n');
    if ordered.is_empty() {
        text.push_str("No rows in the current scope.\n");
        return (text, vec![]);
    }
    let mut targets = Vec::new();
    for object in &ordered {
        let reason = if object.health.evidence.is_empty() {
            match object.health.severity {
                Severity::Healthy => "no evidence -- healthy".to_string(),
                Severity::Unknown => format!("unknown: {}", object.health.status),
                _ => object.health.status.clone(),
            }
        } else {
            object.health.evidence.join("; ")
        };
        let severity_tag = match object.health.severity {
            Severity::Critical => "CRITICAL",
            Severity::Warning => "WARNING",
            Severity::Unknown => "UNKNOWN",
            Severity::Healthy => "healthy",
        };
        let line = text.lines().count();
        text.push_str(&format!(
            "[{severity_tag}] {}/{} ({})\n  {}\n",
            object.namespace, object.name, object.health.status, reason
        ));
        if let Some(resource) = resource {
            targets.push(Target {
                line,
                resource: resource.clone(),
                namespace: object.namespace.clone(),
                name: object.name.clone(),
                uid: object.uid.clone(),
            });
        }
    }
    (text, targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::Object;
    use serde_json::json;
    use std::sync::Arc;

    fn pod(name: &str, phase: &str) -> SharedObject {
        let mut status = json!({"phase": phase});
        if phase == "Running" {
            status["conditions"] = json!([{"type": "Ready", "status": "True"}]);
        }
        Arc::new(Object::new(json!({
            "apiVersion": "v1", "kind": "Pod",
            "metadata": {"namespace": "default", "name": name, "uid": name},
            "status": status,
        })))
    }

    #[test]
    fn empty_scope_says_so_and_never_claims_healthy() {
        let (text, targets) = report(&[], None, &[]);
        assert!(text.contains("No rows in the current scope"));
        assert!(targets.is_empty());
    }

    #[test]
    fn broken_pod_sorts_above_healthy_and_carries_its_evidence() {
        let (text, _) = report(
            &[pod("healthy-a", "Running"), pod("broken-b", "Failed")],
            None,
            &[],
        );
        let broken_pos = text.find("broken-b").expect("broken pod listed");
        let healthy_pos = text.find("healthy-a").expect("healthy pod listed");
        assert!(broken_pos < healthy_pos, "broken pod must be listed first");
        assert!(text.contains("[CRITICAL]"));
    }

    #[test]
    fn unknown_severity_is_never_rendered_as_healthy() {
        let unknown = Arc::new(Object::new(json!({
            "apiVersion": "v1", "kind": "Widget",
            "metadata": {"namespace": "default", "name": "w", "uid": "w"},
        })));
        let (text, _) = report(&[unknown], None, &[]);
        assert!(text.contains("[UNKNOWN]"));
        assert!(!text.contains("[healthy]") || text.contains("[UNKNOWN]"));
    }

    #[test]
    fn caveats_are_explicit_never_silently_dropped() {
        let (text, _) = report(
            &[],
            None,
            &[ScopeCaveat::Incomplete, ScopeCaveat::NotYetSynced],
        );
        assert!(text.contains("PARTIAL: object cache bound reached"));
        assert!(text.contains("list not yet synchronized"));
        assert!(!text.contains("No scope caveats"));
    }

    #[test]
    fn no_caveats_is_stated_explicitly_not_merely_absent() {
        let (text, _) = report(&[pod("a", "Running")], None, &[]);
        assert!(text.contains("No scope caveats"));
    }

    #[test]
    fn navigable_targets_carry_exact_uid_never_name_only() {
        let resource = Resource {
            api: kube::core::ApiResource {
                group: "".into(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "Pod".into(),
                plural: "pods".into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec![],
        };
        let a = pod("same-name", "Failed");
        let (_, targets) = report(std::slice::from_ref(&a), Some(&resource), &[]);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].uid, a.uid);
        assert_eq!(targets[0].name, "same-name");
    }
}
