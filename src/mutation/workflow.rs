//! M8.0: the one shared shell every user-facing mutation action builds on.
//! Each action (`scale`/`restart`/`delete`/`label`/`annotate`) is a pure
//! function that turns validated semantic arguments into a `MutationIntent`
//! plus an optional merge-patch payload -- nothing here issues a network
//! request or makes a policy decision; that stays in `policy::evaluate` and
//! `kube::mutation`'s executor. Unsupported resource/action combinations are
//! rejected here, before any intent is even constructed.
use super::{
    Confirmation, ConfirmationRequirement, MutationEffect, MutationIntent, MutationOutcome,
    MutationRisk, MutationTarget, PolicyEvaluation, Verification,
};
use crate::{app::session::Scope, kube::discovery::Resource};
use serde_json::Value;
use std::hash::{Hash, Hasher};

pub const SCALE_KINDS: &[&str] = &["Deployment", "StatefulSet", "ReplicaSet"];
pub const RESTART_KINDS: &[&str] = &["Deployment", "StatefulSet", "DaemonSet"];
pub const DELETE_KINDS: &[&str] = &[
    "Pod",
    "Deployment",
    "StatefulSet",
    "DaemonSet",
    "ReplicaSet",
    "Job",
    "CronJob",
    "ConfigMap",
];
const PROTECTED_METADATA_PREFIXES: &[&str] = &["kubernetes.io/", "k8s.io/"];
pub const RESTART_ANNOTATION: &str = "kubectl.kubernetes.io/restartedAt";

/// A stable fingerprint, not a cryptographic digest -- matches the intent
/// model's own documented purpose (binding a confirmation to an exact
/// requested change without storing the payload itself).
fn payload_hash(value: &Value) -> String {
    let canonical = serde_json::to_string(value).unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[allow(clippy::too_many_arguments)]
fn intent(
    scope: Scope,
    resource: Resource,
    effect: MutationEffect,
    risk: MutationRisk,
    summary: String,
    payload: &Option<Value>,
    source_action: &str,
    request_id: u64,
) -> MutationIntent {
    MutationIntent {
        request_id,
        target: MutationTarget {
            scope,
            resource,
            expected_resource_version: None,
        },
        effect,
        risk,
        summary,
        payload_sha256: payload.as_ref().map(payload_hash),
        source_action: source_action.into(),
    }
}

/// Result of building a workflow's intent: what policy/preview/executor need,
/// plus a human-readable "before -> after" line for the preview renderer.
pub struct Built {
    pub intent: MutationIntent,
    pub payload: Option<Value>,
    pub change: String,
}

fn unsupported(action: &str, kind: &str) -> String {
    format!("{action} is not supported for {kind}")
}

/// M8.0: the shared state every UI action (Scale/Restart/Delete/Label/
/// Annotate) carries through preview -> dry-run -> confirm -> commit ->
/// verify. Confirmation is never stored ahead of time -- `confirmation()`
/// rebuilds it fresh from the *current* intent, so a target/context change
/// invalidates it automatically rather than by a separate check the caller
/// could forget. `armed` is UX state only (a double-press gate for
/// `RequireStrongerConfirmation`); it authorizes nothing by itself.
pub struct Workflow {
    pub intent: MutationIntent,
    pub payload: Option<Value>,
    pub change: String,
    pub evaluation: PolicyEvaluation,
    pub dry_run: Option<MutationOutcome>,
    pub armed: bool,
    pub commit: Option<MutationOutcome>,
    pub verification: Option<Verification>,
}

impl Workflow {
    pub fn new(built: Built, evaluation: PolicyEvaluation) -> Self {
        Self {
            intent: built.intent,
            payload: built.payload,
            change: built.change,
            evaluation,
            dry_run: None,
            armed: false,
            commit: None,
            verification: None,
        }
    }
    pub fn requirement(&self) -> ConfirmationRequirement {
        self.evaluation.decision.confirmation_requirement()
    }
    /// Fresh evidence bound to the exact current intent -- never cached, per
    /// `Confirmation::authorizes`' own contract.
    pub fn confirmation(&self) -> Confirmation {
        Confirmation {
            request_id: self.intent.request_id,
            scope: self.intent.target.scope.clone(),
            effect: self.intent.effect,
            payload_sha256: self.intent.payload_sha256.clone(),
            requirement: self.requirement(),
        }
    }
}

/// `current_replicas` must come from the already-synced watch object, never
/// guessed -- `None` renders as UNKNOWN, never as zero.
pub fn scale(
    scope: Scope,
    resource: Resource,
    current_replicas: Option<i64>,
    requested: u32,
    request_id: u64,
) -> Result<Built, String> {
    let kind = resource.api.kind.clone();
    if !SCALE_KINDS.contains(&kind.as_str()) {
        return Err(unsupported("scale", &kind));
    }
    let current_desc = match current_replicas {
        Some(c) => c.to_string(),
        None => "UNKNOWN".into(),
    };
    let payload = Some(serde_json::json!({"spec": {"replicas": requested}}));
    let change = format!("replicas: {current_desc} -> {requested}");
    Ok(Built {
        intent: intent(
            scope,
            resource,
            MutationEffect::Modify,
            MutationRisk::Routine,
            "spec.replicas".into(),
            &payload,
            "scale",
            request_id,
        ),
        payload,
        change,
    })
}

/// `timestamp` must be generated exactly once by the caller when the intent
/// is first built and reused verbatim across preview/dry-run/commit -- this
/// function never generates its own, so it stays deterministic and testable.
pub fn restart(
    scope: Scope,
    resource: Resource,
    timestamp: &str,
    request_id: u64,
) -> Result<Built, String> {
    let kind = resource.api.kind.clone();
    if !RESTART_KINDS.contains(&kind.as_str()) {
        return Err(unsupported("restart", &kind));
    }
    let payload = Some(serde_json::json!({
        "spec": {"template": {"metadata": {"annotations": {RESTART_ANNOTATION: timestamp}}}}
    }));
    let change = format!("{RESTART_ANNOTATION}: {timestamp}");
    Ok(Built {
        intent: intent(
            scope,
            resource,
            MutationEffect::Modify,
            MutationRisk::Routine,
            format!("spec.template.metadata.annotations[\"{RESTART_ANNOTATION}\"]"),
            &payload,
            "restart",
            request_id,
        ),
        payload,
        change,
    })
}

pub fn delete(scope: Scope, resource: Resource, request_id: u64) -> Result<Built, String> {
    let kind = resource.api.kind.clone();
    if !DELETE_KINDS.contains(&kind.as_str()) {
        return Err(unsupported("delete", &kind));
    }
    let change = format!("delete {} {}/{}", kind, scope.namespace, scope.name);
    Ok(Built {
        intent: intent(
            scope,
            resource,
            MutationEffect::Delete,
            MutationRisk::Destructive,
            "delete".into(),
            &None,
            "delete",
            request_id,
        ),
        payload: None,
        change,
    })
}

pub const CORDON_KINDS: &[&str] = &["Node"];

/// M8B.1: Node-only, `unschedulable` is an *implementation detail* of this
/// builder -- never a user-authored patch. Cordon/uncordon are the same
/// shape with an inverted boolean, so both funnel through here; the two
/// public functions below keep their own distinct `source_action`, which
/// keeps the journal/preview honest about which direction was requested.
fn set_unschedulable(
    scope: Scope,
    resource: Resource,
    unschedulable: bool,
    request_id: u64,
    source_action: &str,
) -> Result<Built, String> {
    let kind = resource.api.kind.clone();
    if !CORDON_KINDS.contains(&kind.as_str()) {
        return Err(unsupported(source_action, &kind));
    }
    let payload = Some(serde_json::json!({"spec": {"unschedulable": unschedulable}}));
    let change = format!("spec.unschedulable: {} -> {unschedulable}", !unschedulable);
    Ok(Built {
        intent: intent(
            scope,
            resource,
            MutationEffect::Modify,
            MutationRisk::ClusterCritical,
            "spec.unschedulable".into(),
            &payload,
            source_action,
            request_id,
        ),
        payload,
        change,
    })
}

pub fn cordon(scope: Scope, resource: Resource, request_id: u64) -> Result<Built, String> {
    set_unschedulable(scope, resource, true, request_id, "cordon")
}

pub fn uncordon(scope: Scope, resource: Resource, request_id: u64) -> Result<Built, String> {
    set_unschedulable(scope, resource, false, request_id, "uncordon")
}

fn valid_dns_label_part(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars().last().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn validate_key(key: &str) -> Result<(), String> {
    let (prefix, name) = match key.split_once('/') {
        Some((p, n)) => (Some(p), n),
        None => (None, key),
    };
    if let Some(p) = prefix
        && (p.is_empty()
            || p.len() > 253
            || !p
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.')))
    {
        return Err(format!("invalid key prefix: {p}"));
    }
    if !valid_dns_label_part(name) {
        return Err(format!("invalid key name: {name}"));
    }
    Ok(())
}

fn validate_label_value(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if !valid_dns_label_part(value) {
        return Err(format!("invalid label value: {value}"));
    }
    Ok(())
}

fn is_protected_key(key: &str) -> bool {
    PROTECTED_METADATA_PREFIXES
        .iter()
        .any(|p| key.starts_with(p))
}

/// `value` of `None` means remove (`KEY-` grammar): a JSON `null` merge patch
/// entry, which the Kubernetes API server treats as a deletion of that one
/// key -- never a whole-map replacement, so unrelated keys survive untouched.
fn metadata_patch(
    scope: Scope,
    resource: Resource,
    field: &str,
    key: &str,
    value: Option<&str>,
    request_id: u64,
    source_action: &str,
) -> Result<Built, String> {
    validate_key(key)?;
    if is_protected_key(key) {
        return Err(format!("{key} is a protected metadata key"));
    }
    if let Some(v) = value
        && field == "labels"
    {
        validate_label_value(v)?;
    }
    let entry = match value {
        Some(v) => Value::String(v.to_owned()),
        None => Value::Null,
    };
    let payload = Some(serde_json::json!({"metadata": {field: {key: entry}}}));
    let change = match value {
        Some(v) => format!("metadata.{field}[\"{key}\"] = \"{v}\""),
        None => format!("metadata.{field}[\"{key}\"] removed"),
    };
    Ok(Built {
        intent: intent(
            scope,
            resource,
            MutationEffect::Modify,
            MutationRisk::Routine,
            format!("metadata.{field}[\"{key}\"]"),
            &payload,
            source_action,
            request_id,
        ),
        payload,
        change,
    })
}

pub fn label(
    scope: Scope,
    resource: Resource,
    key: &str,
    value: Option<&str>,
    request_id: u64,
) -> Result<Built, String> {
    metadata_patch(scope, resource, "labels", key, value, request_id, "label")
}

pub fn annotate(
    scope: Scope,
    resource: Resource,
    key: &str,
    value: Option<&str>,
    request_id: u64,
) -> Result<Built, String> {
    metadata_patch(
        scope,
        resource,
        "annotations",
        key,
        value,
        request_id,
        "annotate",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kube::core::ApiResource;

    fn scope() -> Scope {
        Scope {
            epoch: 1,
            request: 1,
            context: "kind-sauron-test".into(),
            cluster: "kind-sauron-test".into(),
            resource: "apps/v1/deployments".into(),
            namespace: "sauron-m8".into(),
            name: "m8-deploy".into(),
            uid: "uid-1".into(),
        }
    }
    fn resource(kind: &str) -> Resource {
        Resource {
            api: ApiResource {
                group: "apps".into(),
                version: "v1".into(),
                api_version: "apps/v1".into(),
                kind: kind.into(),
                plural: format!("{}s", kind.to_lowercase()),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec!["patch".into(), "delete".into()],
        }
    }

    #[test]
    fn scale_unsupported_kind_is_rejected_before_building_an_intent() {
        assert!(scale(scope(), resource("CronJob"), Some(1), 3, 1).is_err());
    }

    #[test]
    fn scale_unknown_current_is_never_rendered_as_zero() {
        let built = scale(scope(), resource("Deployment"), None, 3, 1).unwrap();
        assert!(built.change.contains("UNKNOWN -> 3"));
        assert!(!built.change.contains("0 -> 3"));
    }

    #[test]
    fn scale_payload_is_exact_replicas_merge_patch() {
        let built = scale(scope(), resource("StatefulSet"), Some(2), 5, 1).unwrap();
        assert_eq!(
            built.payload,
            Some(serde_json::json!({"spec": {"replicas": 5}}))
        );
    }

    #[test]
    fn scale_same_inputs_produce_same_payload_hash() {
        let a = scale(scope(), resource("ReplicaSet"), Some(1), 2, 1).unwrap();
        let b = scale(scope(), resource("ReplicaSet"), Some(1), 2, 1).unwrap();
        assert_eq!(a.intent.payload_sha256, b.intent.payload_sha256);
        let c = scale(scope(), resource("ReplicaSet"), Some(1), 3, 1).unwrap();
        assert_ne!(a.intent.payload_sha256, c.intent.payload_sha256);
    }

    #[test]
    fn restart_unsupported_kind_is_rejected() {
        assert!(restart(scope(), resource("Pod"), "2026-09-18T00:00:00Z", 1).is_err());
    }

    #[test]
    fn restart_reuses_exact_timestamp_in_payload_and_hash() {
        let a = restart(scope(), resource("Deployment"), "2026-09-18T00:00:00Z", 1).unwrap();
        let b = restart(scope(), resource("Deployment"), "2026-09-18T00:00:00Z", 1).unwrap();
        assert_eq!(a.payload, b.payload);
        assert_eq!(a.intent.payload_sha256, b.intent.payload_sha256);
        let c = restart(scope(), resource("Deployment"), "2026-09-18T00:00:01Z", 1).unwrap();
        assert_ne!(a.intent.payload_sha256, c.intent.payload_sha256);
    }

    #[test]
    fn delete_unsupported_kind_is_rejected() {
        assert!(delete(scope(), resource("Secret"), 1).is_err());
    }

    #[test]
    fn delete_supported_kind_has_no_payload_and_destructive_risk() {
        let built = delete(scope(), resource("Pod"), 1).unwrap();
        assert!(built.payload.is_none());
        assert_eq!(built.intent.risk, MutationRisk::Destructive);
        assert_eq!(built.intent.effect, MutationEffect::Delete);
    }

    fn node_scope() -> Scope {
        Scope {
            epoch: 1,
            request: 1,
            context: "kind-sauron-test".into(),
            cluster: "kind-sauron-test".into(),
            resource: "v1/nodes".into(),
            namespace: String::new(),
            name: "sauron-test-control-plane".into(),
            uid: "node-uid-1".into(),
        }
    }
    fn node_resource() -> Resource {
        Resource {
            api: ApiResource {
                group: String::new(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "Node".into(),
                plural: "nodes".into(),
            },
            namespaced: false,
            short_names: vec![],
            verbs: vec!["patch".into()],
        }
    }

    #[test]
    fn cordon_unsupported_kind_is_rejected_before_building_an_intent() {
        assert!(cordon(scope(), resource("Deployment"), 1).is_err());
    }

    #[test]
    fn cordon_sets_unschedulable_true_with_cluster_critical_risk() {
        let built = cordon(node_scope(), node_resource(), 1).unwrap();
        assert_eq!(
            built.payload,
            Some(serde_json::json!({"spec": {"unschedulable": true}}))
        );
        assert_eq!(built.intent.risk, MutationRisk::ClusterCritical);
        assert_eq!(built.intent.effect, MutationEffect::Modify);
        assert_eq!(built.intent.source_action, "cordon");
        assert!(built.change.contains("false -> true"));
    }

    #[test]
    fn uncordon_sets_unschedulable_false_and_has_a_distinct_source_action() {
        let built = uncordon(node_scope(), node_resource(), 1).unwrap();
        assert_eq!(
            built.payload,
            Some(serde_json::json!({"spec": {"unschedulable": false}}))
        );
        assert_eq!(built.intent.source_action, "uncordon");
        assert!(built.change.contains("true -> false"));
    }

    #[test]
    fn cordon_and_uncordon_produce_different_payload_hashes() {
        let cordoned = cordon(node_scope(), node_resource(), 1).unwrap();
        let uncordoned = uncordon(node_scope(), node_resource(), 1).unwrap();
        assert_ne!(
            cordoned.intent.payload_sha256,
            uncordoned.intent.payload_sha256
        );
    }

    #[test]
    fn label_protected_prefix_is_denied() {
        assert!(
            label(
                scope(),
                resource("Deployment"),
                "kubernetes.io/managed",
                Some("x"),
                1
            )
            .is_err()
        );
        assert!(label(scope(), resource("Deployment"), "k8s.io/team", Some("x"), 1).is_err());
    }

    #[test]
    fn label_invalid_key_is_denied() {
        assert!(label(scope(), resource("Deployment"), "", Some("x"), 1).is_err());
        assert!(label(scope(), resource("Deployment"), "bad key", Some("x"), 1).is_err());
    }

    #[test]
    fn label_invalid_value_is_denied() {
        assert!(
            label(
                scope(),
                resource("Deployment"),
                "team",
                Some("has space"),
                1
            )
            .is_err()
        );
    }

    #[test]
    fn label_set_and_remove_produce_distinct_single_key_patches() {
        let set = label(scope(), resource("Deployment"), "team", Some("infra"), 1).unwrap();
        assert_eq!(
            set.payload,
            Some(serde_json::json!({"metadata": {"labels": {"team": "infra"}}}))
        );
        let remove = label(scope(), resource("Deployment"), "team", None, 1).unwrap();
        assert_eq!(
            remove.payload,
            Some(serde_json::json!({"metadata": {"labels": {"team": null}}}))
        );
        assert_ne!(set.intent.payload_sha256, remove.intent.payload_sha256);
    }

    #[test]
    fn annotate_allows_values_label_syntax_would_reject() {
        // Annotation values are documented as free-form; only the key follows
        // label-key syntax. A value with punctuation must not be denied here.
        let built = annotate(
            scope(),
            resource("Deployment"),
            "sauron.io/note",
            Some("multi word value: ok"),
            1,
        )
        .unwrap();
        assert!(built.payload.is_some());
    }

    #[test]
    fn annotate_protected_prefix_is_denied() {
        assert!(
            annotate(
                scope(),
                resource("Deployment"),
                "kubernetes.io/x",
                Some("y"),
                1
            )
            .is_err()
        );
    }

    fn evaluation(decision: crate::mutation::PolicyDecision) -> crate::mutation::PolicyEvaluation {
        crate::mutation::PolicyEvaluation {
            decision,
            reasons: vec![],
        }
    }

    #[test]
    fn workflow_requirement_matches_policy_decision() {
        use crate::mutation::PolicyDecision;
        let built = scale(scope(), resource("Deployment"), Some(1), 2, 1).unwrap();
        let wf = Workflow::new(
            built,
            evaluation(PolicyDecision::RequireStrongerConfirmation),
        );
        assert_eq!(wf.requirement(), ConfirmationRequirement::Strong);
    }

    #[test]
    fn workflow_confirmation_is_rebuilt_fresh_and_stops_authorizing_after_replacement() {
        use crate::mutation::PolicyDecision;
        let built = scale(scope(), resource("Deployment"), Some(1), 2, 1).unwrap();
        let mut wf = Workflow::new(built, evaluation(PolicyDecision::RequireConfirmation));
        let confirmation = wf.confirmation();
        assert!(confirmation.authorizes(&wf.intent));
        wf.intent.target.scope.uid = "uid-2".into();
        assert!(
            !confirmation.authorizes(&wf.intent),
            "a stale confirmation must not authorize a replaced target"
        );
        let fresh = wf.confirmation();
        assert!(fresh.authorizes(&wf.intent));
    }
}
