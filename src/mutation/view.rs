//! Read-only rendering for the `:policy` and `:mutations` TUI surfaces.
//! Both are local: `policy_report` never touches the network, and
//! `journal_report` only reads the already-loaded bounded journal tail.
use super::{
    ConfirmationRequirement, MutationEffect, MutationIntent, MutationOutcome, MutationRisk,
    MutationTarget, PolicyDecision,
    drain::{DrainPreview, DrainWorkflow, Exclusion, StepOutcome},
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
            create_resource: None,
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
        Some(Created(detail)) => format!("Created -- {detail}"),
    }
}

/// M8B.5: TARGET NODE/CORDON STEP/pod plan/POLICY/CONFIRMATION preview for
/// a pending Drain, and (once run) its per-Pod composite result. Mirrors
/// `workflow_report`'s exact shape and honesty rules -- never renders a
/// specific Pod list before a fresh read has actually returned one, and
/// never collapses a partial result into a single pass/fail boolean.
pub fn drain_report(workflow: &DrainWorkflow) -> String {
    let scope = &workflow.cordon_intent.target.scope;
    let mut out = format!(
        "TARGET NODE: {} {}\n  uid: {}\n\nACTION: drain (cordon the Node, then evict its eligible Pods)\n",
        scope.resource, scope.name, scope.uid
    );
    out.push_str(&format!("CORDON STEP: {}\n\n", workflow.cordon_change));
    match &workflow.preview {
        DrainPreview::Loading => {
            out.push_str("PODS ON THIS NODE: loading -- listing Pods scheduled here...\n\n");
        }
        DrainPreview::Failed(message) => {
            out.push_str(&format!(
                "PODS ON THIS NODE: FAILED to list -- {message}\n\
                 Refusing to show a plan without a fresh Pod list; confirming is refused \
                 until this is retried.\n\n"
            ));
        }
        DrainPreview::Ready(planned) => {
            let eligible: Vec<_> = planned.iter().filter(|p| p.exclusion.is_none()).collect();
            let daemonset: Vec<_> = planned
                .iter()
                .filter(|p| p.exclusion == Some(Exclusion::DaemonSetOwned))
                .collect();
            let local_storage: Vec<_> = planned
                .iter()
                .filter(|p| p.exclusion == Some(Exclusion::LocalStorage))
                .collect();
            out.push_str(&format!(
                "PODS PLANNED FOR EVICTION ({}):\n",
                eligible.len()
            ));
            if eligible.is_empty() {
                out.push_str("  (none)\n");
            }
            for p in &eligible {
                out.push_str(&format!("  - {}/{}\n", p.namespace, p.name));
            }
            out.push_str(&format!(
                "\nEXCLUDED -- DaemonSet-owned ({}): kept running on every node, never evicted \
                 by default (matches kubectl drain):\n",
                daemonset.len()
            ));
            for p in &daemonset {
                out.push_str(&format!("  - {}/{}\n", p.namespace, p.name));
            }
            out.push_str(&format!(
                "\nEXCLUDED -- local storage, emptyDir/hostPath ({}): eviction would lose that \
                 data:\n",
                local_storage.len()
            ));
            for p in &local_storage {
                out.push_str(&format!("  - {}/{}\n", p.namespace, p.name));
            }
            out.push('\n');
        }
    }
    out.push_str(
        "PDB-AWARE EVICTION: each eligible Pod is evicted individually through the eviction \
         subresource; a Pod protected by a PodDisruptionBudget may be denied \
         (DisruptionBudgetDenied) without blocking any other Pod's eviction.\n",
    );
    out.push_str(
        "NO ROLLBACK: once the cordon or a Pod's eviction commits, it stays committed -- Drain \
         never undoes an already-completed step.\n",
    );
    out.push_str(
        "CANCELLATION: stops only *future* steps -- a step already completed keeps its real \
         outcome exactly as committed; cancelling never retries or reverses it.\n\n",
    );
    out.push_str(&format!("POLICY: {:?}\n", workflow.evaluation.decision));
    for reason in &workflow.evaluation.reasons {
        out.push_str(&format!("  - {reason:?}\n"));
    }
    out.push('\n');
    let confirmation_line = match workflow.requirement() {
        ConfirmationRequirement::None => "CONFIRMATION: not required".to_string(),
        ConfirmationRequirement::Standard => {
            "CONFIRMATION: required (press confirm once)".to_string()
        }
        ConfirmationRequirement::Strong => {
            if workflow.armed {
                "CONFIRMATION: strong -- press confirm again to start draining".to_string()
            } else {
                "CONFIRMATION: strong -- press confirm twice to start draining".to_string()
            }
        }
    };
    out.push_str(&confirmation_line);
    out.push('\n');
    if matches!(
        workflow.evaluation.decision,
        PolicyDecision::Deny | PolicyDecision::Unsupported
    ) {
        out.push_str("This action is DENIED; confirming sends zero requests.\n");
    } else if !matches!(workflow.preview, DrainPreview::Ready(_)) {
        out.push_str("Confirming is refused until the Pod list finishes loading.\n");
    }
    if let Some(report) = &workflow.report {
        out.push_str(&format!("\nCORDON RESULT: {:?}\n", report.cordon_outcome));
        for step in &report.steps {
            let outcome_text = match &step.outcome {
                StepOutcome::Excluded(Exclusion::DaemonSetOwned) => {
                    "EXCLUDED (DaemonSet-owned)".to_string()
                }
                StepOutcome::Excluded(Exclusion::LocalStorage) => {
                    "EXCLUDED (local storage)".to_string()
                }
                StepOutcome::NotAttempted => {
                    "NOT ATTEMPTED (drain stopped before this Pod's turn)".to_string()
                }
                StepOutcome::Attempted(outcome) => format!("{outcome:?}"),
            };
            let verification_text = step
                .verification
                .as_ref()
                .map(|v| format!(" · verification: {v:?}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "  - {}/{}: {outcome_text}{verification_text}\n",
                step.namespace, step.name
            ));
        }
        let attempted = report
            .steps
            .iter()
            .filter(|s| matches!(s.outcome, StepOutcome::Attempted(_)))
            .count();
        let committed = report
            .steps
            .iter()
            .filter(|s| {
                matches!(
                    s.outcome,
                    StepOutcome::Attempted(MutationOutcome::Committed)
                        | StepOutcome::Attempted(MutationOutcome::CommittedButJournalIncomplete)
                )
            })
            .count();
        let excluded = report
            .steps
            .iter()
            .filter(|s| matches!(s.outcome, StepOutcome::Excluded(_)))
            .count();
        let not_attempted = report
            .steps
            .iter()
            .filter(|s| matches!(s.outcome, StepOutcome::NotAttempted))
            .count();
        out.push_str(&format!(
            "\nSUMMARY: {committed}/{attempted} attempted evictions committed ({excluded} \
             excluded, {not_attempted} not attempted). This is a partial-progress report, never \
             a single pass/fail boolean.\n"
        ));
    }
    out
}

/// M10.3: bounded past this many rendered targets, in both the pre-commit
/// preview and the post-commit per-target result list -- explicit
/// truncation, never a silent "...and more" with no count.
const BULK_RENDER_BOUND: usize = 50;

/// TARGETS/eligible-vs-excluded/CONFIRMATION/per-target RESULT for a
/// pending or finished bulk mutation. Mirrors `workflow_report`'s exact
/// honesty rules, generalized to N targets: every target's own decision
/// and (once committed) outcome/verification is rendered individually,
/// never collapsed into one aggregate pass/fail line. Pure function of
/// `BulkWorkflow`'s current state; never issues a request itself.
pub fn bulk_report(bulk: &super::bulk::BulkWorkflow) -> String {
    let summary = super::bulk::summarize(bulk);
    let mut out = format!(
        "BULK ACTION: {}\nSELECTED: {} target(s)\n\n",
        bulk.source_action, summary.total_selected
    );
    if bulk.is_empty() {
        out.push_str("Selection is empty -- nothing to do.\n");
        return out;
    }
    out.push_str(&format!(
        "ELIGIBLE: {} · EXCLUDED: {}\n\n",
        bulk.eligible_count(),
        summary.excluded
    ));
    out.push_str("TARGETS\n");
    for (i, item) in bulk.items.iter().enumerate() {
        if i >= BULK_RENDER_BOUND {
            out.push_str(&format!(
                "  ... {} more not shown, all still included\n",
                bulk.items.len() - i
            ));
            break;
        }
        match &item.workflow {
            Ok(w) if matches!(w.evaluation.decision, PolicyDecision::Deny) => {
                out.push_str(&format!(
                    "  [denied]      {}/{} -- {:?}\n",
                    item.namespace, item.name, w.evaluation.reasons
                ))
            }
            Ok(w) if matches!(w.evaluation.decision, PolicyDecision::Unsupported) => out.push_str(
                &format!("  [unsupported] {}/{}\n", item.namespace, item.name),
            ),
            Ok(w) => out.push_str(&format!(
                "  [eligible]    {}/{}: {}\n",
                item.namespace, item.name, w.change
            )),
            Err(reason) => out.push_str(&format!(
                "  [unsupported] {}/{} -- {reason}\n",
                item.namespace, item.name
            )),
        }
    }
    out.push('\n');
    let confirmation_line = match bulk.requirement() {
        ConfirmationRequirement::None => "CONFIRMATION: not required".to_string(),
        ConfirmationRequirement::Standard => {
            "CONFIRMATION: required (press confirm once)".to_string()
        }
        ConfirmationRequirement::Strong => {
            if bulk.armed {
                "CONFIRMATION: strong -- press confirm again to commit".to_string()
            } else {
                "CONFIRMATION: strong -- press confirm twice to commit".to_string()
            }
        }
    };
    out.push_str(&confirmation_line);
    out.push('\n');
    if bulk.eligible_count() == 0 {
        out.push_str(
            "No eligible targets -- every selected target is denied or unsupported; \
             confirming does nothing.\n",
        );
    }
    if let Some(results) = &bulk.results {
        out.push_str(&format!(
            "\nRESULT: {} attempted · {} committed ({} verified) · {} cancelled · {} other\n\n",
            summary.attempted,
            summary.committed,
            summary.verified,
            summary.cancelled,
            summary.other
        ));
        out.push_str("PER-TARGET RESULT\n");
        for (i, r) in results.iter().enumerate() {
            if i >= BULK_RENDER_BOUND {
                out.push_str(&format!("  ... {} more not shown\n", results.len() - i));
                break;
            }
            out.push_str(&format!(
                "  {}/{}: {:?} · verify: {}\n",
                r.namespace,
                r.name,
                r.commit,
                verification_line(&r.verification)
            ));
        }
    }
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

    fn node_scope() -> Scope {
        Scope {
            epoch: 1,
            request: 1,
            context: "kind-sauron-test".into(),
            cluster: "kind-sauron-test".into(),
            resource: "v1/nodes".into(),
            namespace: String::new(),
            name: "node-1".into(),
            uid: "node-uid".into(),
        }
    }
    fn node_resource() -> Resource {
        Resource {
            api: ApiResource {
                group: "".into(),
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
    fn pod_resource() -> Resource {
        Resource {
            api: ApiResource {
                group: "".into(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "Pod".into(),
                plural: "pods".into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec!["delete".into()],
        }
    }
    fn drain_workflow() -> DrainWorkflow {
        use crate::mutation::{PolicyDecision, PolicyEvaluation, PolicyReason, workflow};
        let built = workflow::cordon(node_scope(), node_resource(), 1).unwrap();
        DrainWorkflow::new(
            built.intent,
            built.payload,
            built.change,
            pod_resource(),
            PolicyEvaluation {
                decision: PolicyDecision::RequireStrongerConfirmation,
                reasons: vec![PolicyReason::StrongerConfirmationRequired],
            },
        )
    }
    fn planned_pod(
        namespace: &str,
        name: &str,
        exclusion: Option<Exclusion>,
    ) -> crate::mutation::drain::PlannedPod {
        crate::mutation::drain::PlannedPod {
            namespace: namespace.into(),
            name: name.into(),
            uid: format!("uid-{name}"),
            exclusion,
        }
    }

    #[test]
    fn drain_report_never_shows_a_pod_list_before_a_fresh_read_returns_one() {
        let report = drain_report(&drain_workflow());
        assert!(report.contains("TARGET NODE: v1/nodes node-1"));
        assert!(report.contains("CORDON STEP:"));
        assert!(report.contains("loading"));
        assert!(!report.contains("PODS PLANNED FOR EVICTION"));
        assert!(report.contains("CONFIRMATION: strong -- press confirm twice"));
    }

    #[test]
    fn drain_report_failed_preview_refuses_to_show_a_plan_and_blocks_confirm() {
        let mut wf = drain_workflow();
        wf.preview = DrainPreview::Failed("OutcomeUnknown".into());
        let report = drain_report(&wf);
        assert!(report.contains("FAILED to list"));
        assert!(report.contains("OutcomeUnknown"));
        assert!(report.contains("Confirming is refused"));
    }

    #[test]
    fn drain_report_ready_preview_lists_eligible_and_excluded_pods_with_reasons() {
        let mut wf = drain_workflow();
        wf.preview = DrainPreview::Ready(vec![
            planned_pod("default", "web-1", None),
            planned_pod(
                "kube-system",
                "ds-1",
                Some(super::Exclusion::DaemonSetOwned),
            ),
            planned_pod("default", "cache-1", Some(super::Exclusion::LocalStorage)),
        ]);
        let report = drain_report(&wf);
        assert!(report.contains("PODS PLANNED FOR EVICTION (1):"));
        assert!(report.contains("default/web-1"));
        assert!(report.contains("EXCLUDED -- DaemonSet-owned (1):"));
        assert!(report.contains("kube-system/ds-1"));
        assert!(report.contains("EXCLUDED -- local storage, emptyDir/hostPath (1):"));
        assert!(report.contains("default/cache-1"));
        assert!(report.contains("PDB-AWARE EVICTION"));
        assert!(report.contains("NO ROLLBACK"));
        assert!(report.contains("CANCELLATION: stops only"));
        assert!(!report.contains("Confirming is refused"));
    }

    #[test]
    fn drain_report_armed_state_is_shown_distinctly_from_unarmed() {
        let mut wf = drain_workflow();
        wf.preview = DrainPreview::Ready(vec![]);
        assert!(drain_report(&wf).contains("press confirm twice to start draining"));
        wf.armed = true;
        assert!(drain_report(&wf).contains("press confirm again to start draining"));
    }

    #[test]
    fn drain_report_denied_decision_blocks_confirm_regardless_of_preview_state() {
        use crate::mutation::{PolicyDecision, PolicyReason};
        let mut wf = drain_workflow();
        wf.evaluation.decision = PolicyDecision::Deny;
        wf.evaluation.reasons = vec![PolicyReason::ReadonlyMode];
        wf.preview = DrainPreview::Ready(vec![]);
        let report = drain_report(&wf);
        assert!(report.contains("DENIED; confirming sends zero requests"));
    }

    #[test]
    fn drain_report_partial_outcomes_render_each_step_distinctly_never_one_boolean() {
        use crate::mutation::drain::{DrainReport, DrainStep, StepOutcome};
        let mut wf = drain_workflow();
        wf.preview = DrainPreview::Ready(vec![]);
        wf.report = Some(DrainReport {
            cordon_outcome: MutationOutcome::Committed,
            steps: vec![
                DrainStep {
                    namespace: "kube-system".into(),
                    name: "ds-1".into(),
                    uid: "uid-ds".into(),
                    outcome: StepOutcome::Excluded(super::Exclusion::DaemonSetOwned),
                    verification: None,
                },
                DrainStep {
                    namespace: "default".into(),
                    name: "ok-1".into(),
                    uid: "uid-ok".into(),
                    outcome: StepOutcome::Attempted(MutationOutcome::Committed),
                    verification: Some(crate::mutation::Verification::DeletionInProgress),
                },
                DrainStep {
                    namespace: "default".into(),
                    name: "denied-1".into(),
                    uid: "uid-denied".into(),
                    outcome: StepOutcome::Attempted(MutationOutcome::DisruptionBudgetDenied),
                    verification: None,
                },
                DrainStep {
                    namespace: "default".into(),
                    name: "never-1".into(),
                    uid: "uid-never".into(),
                    outcome: StepOutcome::NotAttempted,
                    verification: None,
                },
            ],
        });
        let report = drain_report(&wf);
        assert!(report.contains("CORDON RESULT: Committed"));
        assert!(report.contains("kube-system/ds-1: EXCLUDED (DaemonSet-owned)"));
        assert!(report.contains("default/ok-1: Committed · verification: DeletionInProgress"));
        assert!(report.contains("default/denied-1: DisruptionBudgetDenied"));
        assert!(report.contains("default/never-1: NOT ATTEMPTED"));
        assert!(
            report.contains(
                "SUMMARY: 1/2 attempted evictions committed (1 excluded, 1 not attempted)"
            ),
            "a partial result must never collapse into a single pass/fail boolean: {report}"
        );
    }
}
