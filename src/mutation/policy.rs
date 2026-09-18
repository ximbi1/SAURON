//! Central mutation policy engine: pure, deterministic, testable without any
//! Kubernetes transport. UNKNOWN never silently means Allow -- every branch
//! this module cannot positively justify denies or requires confirmation.
use super::{
    MutationEffect, MutationIntent, MutationRisk, PolicyDecision, PolicyEvaluation, PolicyReason,
};
use std::collections::BTreeSet;

/// Everything policy needs to know that is NOT part of the intent itself.
/// `cluster_verified_for_mutation` must come from an explicit, positively
/// proven identity check (e.g. `scripts/test-cluster.sh`'s Docker/API
/// loopback guard) -- never a context-name heuristic. Defaults to the
/// strictest posture: unverified, readonly, no protected exceptions.
#[derive(Clone, Debug)]
pub struct PolicyContext {
    pub readonly: bool,
    /// True only when readonly was forced by the CLI `--readonly` override
    /// specifically (a distinct, stronger reason than a config default).
    pub readonly_forced: bool,
    pub cluster_verified_for_mutation: bool,
    pub protected_namespaces: BTreeSet<String>,
    /// Cluster-scoped kinds that always require the strongest guardrail
    /// regardless of effect (Namespace, Node, ClusterRole, ...).
    pub cluster_critical_kinds: BTreeSet<String>,
    /// Namespaced kinds that are privilege-sensitive even inside an
    /// otherwise-unprotected namespace (Secret, ServiceAccount, Role,
    /// RoleBinding, ...).
    pub privilege_sensitive_kinds: BTreeSet<String>,
}
impl Default for PolicyContext {
    fn default() -> Self {
        Self {
            readonly: true,
            readonly_forced: false,
            cluster_verified_for_mutation: false,
            protected_namespaces: ["kube-system", "kube-public", "kube-node-lease"]
                .into_iter()
                .map(String::from)
                .collect(),
            cluster_critical_kinds: [
                "Namespace",
                "Node",
                "ClusterRole",
                "ClusterRoleBinding",
                "CustomResourceDefinition",
                "APIService",
                "MutatingWebhookConfiguration",
                "ValidatingWebhookConfiguration",
                "StorageClass",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            privilege_sensitive_kinds: [
                "Secret",
                "ServiceAccount",
                "Role",
                "RoleBinding",
                "PersistentVolume",
                "PersistentVolumeClaim",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        }
    }
}

/// Evaluates every gate in a fixed, deterministic order and accumulates
/// every reason that applies -- never stops at the first one -- so the
/// caller sees the complete picture, not just whichever check happened to
/// run first.
pub fn evaluate(context: &PolicyContext, intent: &MutationIntent) -> PolicyEvaluation {
    let mut reasons = Vec::new();
    let mut hard_deny = false;

    // 1: global readonly state / 2: CLI --readonly override.
    if context.readonly {
        reasons.push(if context.readonly_forced {
            PolicyReason::ReadonlyOverride
        } else {
            PolicyReason::ReadonlyMode
        });
        hard_deny = true;
    }

    // 3: cluster/context safety -- must be positively verified, never a
    // context-name heuristic. Unverified is UNKNOWN, which never means Allow.
    if !context.cluster_verified_for_mutation {
        reasons.push(PolicyReason::UnverifiedCluster);
        hard_deny = true;
    }

    // 4: target identity completeness -- a mutation against an existing
    // object with no UID is unsupported: name alone is never identity.
    if intent.target.scope.uid.is_empty() {
        reasons.push(PolicyReason::TargetIdentityIncomplete);
        hard_deny = true;
    }

    // 5: stale/replaced target status is a caller-time concern (the target
    // was already re-validated before an intent reaches policy); this gate
    // exists at the executor's TOCTOU recheck, not here, since policy has no
    // transport. Nothing to evaluate here beyond identity completeness above.

    // 6: namespace/resource protection policy.
    if context
        .protected_namespaces
        .contains(&intent.target.scope.namespace)
    {
        reasons.push(PolicyReason::ProtectedNamespace(
            intent.target.scope.namespace.clone(),
        ));
        hard_deny = true;
    }
    let kind = intent.target.resource.api.kind.as_str();
    if context.cluster_critical_kinds.contains(kind) {
        reasons.push(PolicyReason::ClusterScopedSensitiveResource);
        reasons.push(PolicyReason::ClusterCriticalResource);
    } else if context.privilege_sensitive_kinds.contains(kind) {
        reasons.push(PolicyReason::PrivilegeSensitiveResource);
    }

    // 7/8: operation/effect and mutation risk.
    if intent.effect == MutationEffect::Delete {
        reasons.push(PolicyReason::DestructiveEffect);
    }
    if intent.risk == MutationRisk::Destructive {
        reasons.push(PolicyReason::DestructiveEffect);
    }

    if hard_deny {
        return PolicyEvaluation {
            decision: PolicyDecision::Deny,
            reasons,
        };
    }

    // 9: whether explicit confirmation is required, and how strong.
    let strong = matches!(
        intent.risk,
        MutationRisk::PrivilegeSensitive | MutationRisk::ClusterCritical
    ) || context.cluster_critical_kinds.contains(kind)
        || context.privilege_sensitive_kinds.contains(kind)
        || intent.effect == MutationEffect::Delete;
    let confirmation_needed = strong
        || matches!(intent.risk, MutationRisk::Destructive)
        || intent.effect != MutationEffect::Create;

    if strong {
        reasons.push(PolicyReason::StrongerConfirmationRequired);
        return PolicyEvaluation {
            decision: PolicyDecision::RequireStrongerConfirmation,
            reasons,
        };
    }
    if confirmation_needed {
        reasons.push(PolicyReason::ConfirmationRequired);
        return PolicyEvaluation {
            decision: PolicyDecision::RequireConfirmation,
            reasons,
        };
    }
    PolicyEvaluation {
        decision: PolicyDecision::Allow,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::session::Scope,
        kube::discovery::Resource,
        mutation::{MutationIntent, MutationTarget},
    };
    use kube::core::ApiResource;

    fn resource(kind: &str, namespaced: bool) -> Resource {
        Resource {
            api: ApiResource {
                group: "".into(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: kind.into(),
                plural: format!("{}s", kind.to_lowercase()),
            },
            namespaced,
            short_names: vec![],
            verbs: vec!["patch".into(), "delete".into()],
        }
    }
    fn intent(namespace: &str, kind: &str, uid: &str, effect: MutationEffect) -> MutationIntent {
        MutationIntent {
            request_id: 1,
            target: MutationTarget {
                scope: Scope {
                    epoch: 1,
                    request: 1,
                    context: "kind-sauron-test".into(),
                    cluster: "kind-sauron-test".into(),
                    resource: format!("v1/{}s", kind.to_lowercase()),
                    namespace: namespace.into(),
                    name: "target".into(),
                    uid: uid.into(),
                },
                resource: resource(kind, true),
                expected_resource_version: None,
            },
            effect,
            risk: MutationRisk::Routine,
            summary: "test".into(),
            payload_sha256: None,
            source_action: "test".into(),
        }
    }
    fn verified() -> PolicyContext {
        PolicyContext {
            readonly: false,
            readonly_forced: false,
            cluster_verified_for_mutation: true,
            ..PolicyContext::default()
        }
    }

    #[test]
    fn readonly_denies_before_anything_else() {
        let mut ctx = verified();
        ctx.readonly = true;
        let eval = evaluate(
            &ctx,
            &intent("sauron-m7", "ConfigMap", "u", MutationEffect::Modify),
        );
        assert_eq!(eval.decision, PolicyDecision::Deny);
        assert!(eval.reasons.contains(&PolicyReason::ReadonlyMode));
    }

    #[test]
    fn readonly_override_is_a_distinct_reason() {
        let mut ctx = verified();
        ctx.readonly = true;
        ctx.readonly_forced = true;
        let eval = evaluate(
            &ctx,
            &intent("sauron-m7", "ConfigMap", "u", MutationEffect::Modify),
        );
        assert_eq!(eval.decision, PolicyDecision::Deny);
        assert!(eval.reasons.contains(&PolicyReason::ReadonlyOverride));
        assert!(!eval.reasons.contains(&PolicyReason::ReadonlyMode));
    }

    #[test]
    fn unverified_cluster_denies_even_if_otherwise_allowed() {
        let ctx = PolicyContext {
            readonly: false,
            cluster_verified_for_mutation: false,
            ..PolicyContext::default()
        };
        let eval = evaluate(
            &ctx,
            &intent("sauron-m7", "ConfigMap", "u", MutationEffect::Modify),
        );
        assert_eq!(eval.decision, PolicyDecision::Deny);
        assert!(eval.reasons.contains(&PolicyReason::UnverifiedCluster));
    }

    #[test]
    fn missing_uid_is_unsupported_never_allowed() {
        let eval = evaluate(
            &verified(),
            &intent("sauron-m7", "ConfigMap", "", MutationEffect::Modify),
        );
        assert_eq!(eval.decision, PolicyDecision::Deny);
        assert!(
            eval.reasons
                .contains(&PolicyReason::TargetIdentityIncomplete)
        );
    }

    #[test]
    fn protected_namespace_denies() {
        let eval = evaluate(
            &verified(),
            &intent("kube-system", "ConfigMap", "u", MutationEffect::Modify),
        );
        assert_eq!(eval.decision, PolicyDecision::Deny);
        assert!(
            eval.reasons
                .contains(&PolicyReason::ProtectedNamespace("kube-system".into()))
        );
    }

    #[test]
    fn normal_fixture_namespace_is_allowed_or_confirmable() {
        let eval = evaluate(
            &verified(),
            &intent("sauron-m7", "ConfigMap", "u", MutationEffect::Modify),
        );
        assert_ne!(eval.decision, PolicyDecision::Deny);
    }

    #[test]
    fn cluster_scoped_sensitive_kind_requires_strong_confirmation() {
        let eval = evaluate(
            &verified(),
            &intent("", "Namespace", "u", MutationEffect::Modify),
        );
        assert_eq!(eval.decision, PolicyDecision::RequireStrongerConfirmation);
        assert!(
            eval.reasons
                .contains(&PolicyReason::ClusterCriticalResource)
        );
    }

    #[test]
    fn destructive_delete_requires_strong_confirmation() {
        let eval = evaluate(
            &verified(),
            &intent("sauron-m7", "ConfigMap", "u", MutationEffect::Delete),
        );
        assert_eq!(eval.decision, PolicyDecision::RequireStrongerConfirmation);
        assert!(eval.reasons.contains(&PolicyReason::DestructiveEffect));
    }

    #[test]
    fn privilege_sensitive_kind_requires_strong_confirmation() {
        let eval = evaluate(
            &verified(),
            &intent("sauron-m7", "Secret", "u", MutationEffect::Modify),
        );
        assert_eq!(eval.decision, PolicyDecision::RequireStrongerConfirmation);
        assert!(
            eval.reasons
                .contains(&PolicyReason::PrivilegeSensitiveResource)
        );
    }

    #[test]
    fn routine_modify_requires_standard_confirmation_not_allow() {
        let eval = evaluate(
            &verified(),
            &intent("sauron-m7", "ConfigMap", "u", MutationEffect::Modify),
        );
        assert_eq!(eval.decision, PolicyDecision::RequireConfirmation);
        assert!(eval.reasons.contains(&PolicyReason::ConfirmationRequired));
    }

    #[test]
    fn create_with_routine_risk_is_allowed_without_confirmation() {
        let eval = evaluate(
            &verified(),
            &intent("sauron-m7", "ConfigMap", "u", MutationEffect::Create),
        );
        assert_eq!(eval.decision, PolicyDecision::Allow);
    }

    #[test]
    fn decision_ordering_is_deterministic_across_repeated_calls() {
        let ctx = verified();
        let i = intent("sauron-m7", "Secret", "u", MutationEffect::Modify);
        let a = evaluate(&ctx, &i);
        let b = evaluate(&ctx, &i);
        assert_eq!(a, b);
    }
}
