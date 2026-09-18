//! Read-only rendering for the `:policy` and `:mutations` TUI surfaces.
//! Both are local: `policy_report` never touches the network, and
//! `journal_report` only reads the already-loaded bounded journal tail.
use super::{
    ConfirmationRequirement, MutationEffect, MutationIntent, MutationRisk, MutationTarget,
    journal::Record,
    policy::{self, PolicyContext},
    workflow::Workflow,
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

/// M8.0: TARGET/ACTION/CHANGE/POLICY/PREFLIGHT/CONFIRMATION preview for a
/// pending mutation workflow. Never renders raw payload or Secret content --
/// only the pre-computed, already-redacted `change` summary. This is a pure
/// function of `Workflow`'s current state; it never issues a request itself.
pub fn workflow_report(workflow: &Workflow) -> String {
    let intent = &workflow.intent;
    let scope = &intent.target.scope;
    let mut out = format!(
        "TARGET: {} {}/{}\n  uid: {}\n\nACTION: {}\nCHANGE: {}\n\n",
        scope.resource,
        scope.namespace,
        scope.name,
        scope.uid,
        intent.source_action,
        workflow.change
    );
    out.push_str(&format!("POLICY: {:?}\n", workflow.evaluation.decision));
    for reason in &workflow.evaluation.reasons {
        out.push_str(&format!("  - {reason:?}\n"));
    }
    out.push('\n');
    match &workflow.dry_run {
        Some(outcome) => out.push_str(&format!("PREFLIGHT (server dry-run): {outcome:?}\n\n")),
        None => out.push_str("PREFLIGHT: not run (press the dry-run key to try one)\n\n"),
    }
    let confirmation_line = match workflow.requirement() {
        ConfirmationRequirement::None => "CONFIRMATION: not required".to_string(),
        ConfirmationRequirement::Standard => {
            "CONFIRMATION: required (press confirm once)".to_string()
        }
        ConfirmationRequirement::Strong => {
            if workflow.armed {
                "CONFIRMATION: strong -- press confirm again to commit".to_string()
            } else {
                "CONFIRMATION: strong -- press confirm twice to commit".to_string()
            }
        }
    };
    out.push_str(&confirmation_line);
    out.push('\n');
    if matches!(
        workflow.evaluation.decision,
        super::PolicyDecision::Deny | super::PolicyDecision::Unsupported
    ) {
        out.push_str("This action is DENIED; confirming sends zero requests.\n");
    }
    if let Some(outcome) = &workflow.commit {
        // COMMIT RESULT is never revised by verification below: the API
        // server already confirmed (or refused) the write; verification is
        // a separate, later fact about what a fresh read shows, and never
        // downgrades this line even when it is Unknown/Pending/differs.
        out.push_str(&format!("\nCOMMIT RESULT: {outcome:?}\n"));
        out.push_str(&format!(
            "VERIFICATION: {}\n",
            verification_line(&workflow.verification)
        ));
        out.push_str(
            "(Readiness/availability is not shown here -- see Explain/health (M5); \
             this is a distinct fact from whether the API server accepted the write.)\n",
        );
    }
    out
}

/// M8.5: `None` means verification was never attempted for this outcome
/// (e.g. the commit itself was denied/cancelled -- nothing to verify).
fn verification_line(verification: &Option<super::Verification>) -> String {
    use super::Verification::*;
    match verification {
        None => "not attempted".into(),
        Some(Verified) => {
            "Verified -- fresh observation confirms the exact requested change".into()
        }
        Some(Pending) => {
            "Pending -- accepted but not yet reflected (eventual-consistency window)".into()
        }
        Some(Unknown) => {
            "Unknown -- could not be confirmed (timeout, cancellation, or transport)".into()
        }
        Some(ObservedDifferent(detail)) => format!("ObservedDifferent -- {detail}"),
        Some(TargetReplaced) => {
            "TargetReplaced -- the object's UID changed before verification could run".into()
        }
        Some(DeletionInProgress) => "DeletionInProgress -- deletionTimestamp is now present".into(),
        Some(ObservedGone) => {
            "ObservedGone -- a fresh read confirms the object no longer exists".into()
        }
    }
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
    fn workflow_report_never_shows_a_dry_run_result_before_one_runs() {
        use crate::mutation::{PolicyDecision, PolicyEvaluation, workflow};
        let mut deployment = resource();
        deployment.api.kind = "Deployment".into();
        let built = workflow::scale(scope(), deployment, Some(2), 5, 1).unwrap();
        let wf = workflow::Workflow::new(
            built,
            PolicyEvaluation {
                decision: PolicyDecision::RequireConfirmation,
                reasons: vec![],
            },
        );
        let report = workflow_report(&wf);
        assert!(report.contains("not run"));
        assert!(report.contains("replicas: 2 -> 5"));
        assert!(report.contains("CONFIRMATION: required"));
    }

    #[test]
    fn workflow_report_marks_strong_confirmation_armed_state_distinctly() {
        use crate::mutation::{PolicyDecision, PolicyEvaluation, workflow};
        let built = workflow::delete(scope(), resource(), 1).unwrap();
        let mut wf = workflow::Workflow::new(
            built,
            PolicyEvaluation {
                decision: PolicyDecision::RequireStrongerConfirmation,
                reasons: vec![],
            },
        );
        let before = workflow_report(&wf);
        assert!(before.contains("press confirm twice"));
        wf.armed = true;
        let armed = workflow_report(&wf);
        assert!(armed.contains("press confirm again"));
    }

    #[test]
    fn workflow_report_denied_action_states_zero_requests() {
        use crate::mutation::{PolicyDecision, PolicyEvaluation, PolicyReason, workflow};
        let mut deployment = resource();
        deployment.api.kind = "Deployment".into();
        let built = workflow::scale(scope(), deployment, Some(2), 5, 1).unwrap();
        let wf = workflow::Workflow::new(
            built,
            PolicyEvaluation {
                decision: PolicyDecision::Deny,
                reasons: vec![PolicyReason::ReadonlyMode],
            },
        );
        let report = workflow_report(&wf);
        assert!(report.contains("DENIED"));
        assert!(report.contains("zero requests"));
    }

    #[test]
    fn workflow_report_shows_commit_and_verification_as_two_distinct_facts() {
        use crate::mutation::{
            MutationOutcome, PolicyDecision, PolicyEvaluation, Verification, workflow,
        };
        let built = workflow::delete(scope(), resource(), 1).unwrap();
        let mut wf = workflow::Workflow::new(
            built,
            PolicyEvaluation {
                decision: PolicyDecision::RequireStrongerConfirmation,
                reasons: vec![],
            },
        );
        wf.commit = Some(MutationOutcome::Committed);
        wf.verification = Some(Verification::Unknown);
        let report = workflow_report(&wf);
        assert!(report.contains("COMMIT RESULT: Committed"));
        assert!(report.contains("VERIFICATION: Unknown"));
        assert!(
            !report.contains("COMMIT RESULT: Unknown"),
            "an unknown/failed verification must never be rendered as if the commit itself failed"
        );
    }

    #[test]
    fn workflow_report_never_attempted_verification_is_distinct_from_pending_or_unknown() {
        use crate::mutation::{MutationOutcome, PolicyDecision, PolicyEvaluation, workflow};
        let built = workflow::scale(
            scope(),
            {
                let mut d = resource();
                d.api.kind = "Deployment".into();
                d
            },
            Some(1),
            2,
            1,
        )
        .unwrap();
        let mut wf = workflow::Workflow::new(
            built,
            PolicyEvaluation {
                decision: PolicyDecision::RequireConfirmation,
                reasons: vec![],
            },
        );
        wf.commit = Some(MutationOutcome::Denied);
        let report = workflow_report(&wf);
        assert!(report.contains("VERIFICATION: not attempted"));
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
