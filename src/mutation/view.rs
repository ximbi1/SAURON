//! Read-only rendering for the `:policy` and `:mutations` TUI surfaces.
//! Both are local: `policy_report` never touches the network, and
//! `journal_report` only reads the already-loaded bounded journal tail.
use super::{
    MutationEffect, MutationIntent, MutationRisk, MutationTarget,
    journal::Record,
    policy::{self, PolicyContext},
};
use crate::{app::session::Scope, kube::discovery::Resource};

/// What would happen if the user tried a hypothetical Modify or Delete on
/// the selected object, under the actual runtime policy context. This view
/// itself never mutates and never sends a mutation request.
pub fn policy_report(context: &PolicyContext, scope: &Scope, resource: &Resource) -> String {
    let mut out = format!(
        "POLICY (read-only): {} {}/{}\nUID: {}\n\n",
        scope.resource, scope.namespace, scope.name, scope.uid
    );
    out.push_str(&format!(
        "readonly: {}{}\ncluster verified for mutation: {}\n\n",
        context.readonly,
        if context.readonly_forced {
            " (forced by --readonly)"
        } else {
            ""
        },
        context.cluster_verified_for_mutation
    ));
    for (effect, risk) in [
        (MutationEffect::Modify, MutationRisk::Routine),
        (MutationEffect::Delete, MutationRisk::Destructive),
    ] {
        let intent = MutationIntent {
            request_id: 0,
            target: MutationTarget {
                scope: scope.clone(),
                resource: resource.clone(),
                expected_resource_version: None,
            },
            effect,
            risk,
            summary: format!("hypothetical {effect:?}"),
            payload_sha256: None,
            source_action: "policy_view".into(),
        };
        let evaluation = policy::evaluate(context, &intent);
        out.push_str(&format!("{effect:?}: {:?}\n", evaluation.decision));
        for reason in &evaluation.reasons {
            out.push_str(&format!("  - {reason:?}\n"));
        }
        out.push('\n');
    }
    out.push_str(
        "This view is read-only and issues no request. SAURON has no in-app \
         mechanism to mark a cluster as verified for mutation -- that \
         verification is an external, out-of-band guarantee (see \
         scripts/test-cluster.sh for the guarded acceptance harness). M7 is \
         infrastructure only; no user-facing mutation workflow exists yet.\n",
    );
    out
}

/// Bounded, deterministic, most-recent-last. Never includes raw payload or
/// Secret content -- the journal itself cannot hold either.
pub fn journal_report(records: &[Record]) -> String {
    let mut out = String::from("MUTATION JOURNAL (recent, bounded)\n\n");
    if records.is_empty() {
        out.push_str("No mutation journal records yet.\n");
        return out;
    }
    for r in records {
        out.push_str(&format!(
            "{} #{} {:?} {} {}/{} uid={} [{}] policy={} outcome={}\n",
            r.timestamp,
            r.request_id,
            r.phase,
            r.resource,
            r.namespace,
            r.name,
            r.uid,
            r.effect,
            r.policy_decision.as_deref().unwrap_or("-"),
            r.outcome.as_deref().unwrap_or("-"),
        ));
    }
    out
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
            resource: "v1/configmaps".into(),
            namespace: "sauron-m7".into(),
            name: "m7-target".into(),
            uid: "uid-1".into(),
        }
    }
    fn resource() -> Resource {
        Resource {
            api: ApiResource {
                group: "".into(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "ConfigMap".into(),
                plural: "configmaps".into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec!["patch".into()],
        }
    }

    #[test]
    fn policy_report_never_claims_allowed_without_verified_cluster() {
        let report = policy_report(&PolicyContext::default(), &scope(), &resource());
        assert!(report.contains("UnverifiedCluster"));
        assert!(!report.contains("Modify: Allow"));
        assert!(!report.contains("Delete: Allow"));
    }

    #[test]
    fn policy_report_shows_both_hypothetical_effects_and_reasons() {
        let report = policy_report(&PolicyContext::default(), &scope(), &resource());
        assert!(report.contains("Modify:"));
        assert!(report.contains("Delete:"));
        assert!(report.contains("ReadonlyMode"));
    }

    #[test]
    fn journal_report_lists_records_and_handles_empty() {
        assert!(journal_report(&[]).contains("No mutation journal records"));
        let record = Record {
            schema_version: 1,
            timestamp: "2026-09-18T00:00:00Z".into(),
            request_id: 7,
            phase: crate::mutation::journal::Phase::CommitResult,
            context: "kind-sauron-test".into(),
            cluster: "kind-sauron-test".into(),
            resource: "v1/configmaps".into(),
            namespace: "sauron-m7".into(),
            name: "m7-target".into(),
            uid: "uid-1".into(),
            effect: "Modify".into(),
            summary: "metadata.annotations[\"m7-proof\"]".into(),
            payload_sha256: Some("hash".into()),
            policy_decision: Some("RequireConfirmation".into()),
            policy_reasons: vec!["ConfirmationRequired".into()],
            outcome: Some("Committed".into()),
            detail: None,
        };
        let out = journal_report(&[record]);
        assert!(out.contains("m7-target"));
        assert!(out.contains("Committed"));
    }
}
