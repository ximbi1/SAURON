//! M8B.5: the only Drain orchestrator -- cordon, then a bounded,
//! sequential eviction sweep over a Node's eligible Pods. This is the
//! one documented exception to "one user action, one intent" in the
//! M7/M8/M8B model: NOT a new confirmation/policy/journal/transport
//! subsystem. The cordon step and every eviction step each go through
//! the exact same `mutation::workflow` builders and
//! `kube::mutation::{commit, verify}` executor as if invoked
//! individually -- this module only sequences and reports them.
//!
//! Cancellation semantics: once cancelled, no *further* eviction is
//! attempted -- Pods already evicted stay evicted (there is no
//! rollback, ever). Every remaining planned Pod is reported
//! `StepOutcome::NotAttempted`, never silently omitted.
use super::{Connection, discovery::Resource, mutation as executor};
use crate::app::session::Scope;
use crate::mutation::{
    Confirmation, ConfirmationRequirement, MutationOutcome,
    drain::{DrainReport, DrainStep, PlannedPod, StepOutcome, plan as plan_drain},
    journal::{Journal, Phase},
    policy::PolicyContext,
    workflow,
};
use kube::api::ListParams;
use tokio_util::sync::CancellationToken;

/// A bounded list of Pods currently scheduled on `node_name`, across
/// every namespace -- the same 200-item cap `read_bounded` already uses
/// elsewhere, so a pathologically large node cannot make Drain plan an
/// unbounded number of steps.
/// M8B.5 UI wiring: also used directly by the app layer to build the
/// truthful preview shown before Drain is ever confirmed -- the real
/// commit-time call inside `drain()` below always re-lists fresh, so a
/// preview snapshot going stale between open and confirm is never a
/// TOCTOU concern, only a display-freshness one.
pub(crate) async fn pods_on_node(
    connection: &Connection,
    pod_resource: &Resource,
    node_name: &str,
    cancel: &CancellationToken,
) -> Result<Vec<serde_json::Value>, MutationOutcome> {
    let api = pod_resource.api(connection.client.clone(), None);
    let params = ListParams::default()
        .fields(&format!("spec.nodeName={node_name}"))
        .limit(200);
    let list = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(MutationOutcome::Cancelled),
        result = tokio::time::timeout(connection.timeout(), api.list(&params)) => result,
    };
    match list {
        Ok(Ok(list)) => Ok(list
            .items
            .into_iter()
            .filter_map(|o| serde_json::to_value(o).ok())
            .collect()),
        _ => Err(MutationOutcome::OutcomeUnknown),
    }
}

/// The single entry point. `node_scope`/`node_resource` identify the
/// Node (already TOCTOU-checked exactly like any other cordon, via the
/// cordon step's own `commit()` call below); `pod_resource` is the Pod
/// kind's own `Resource`, resolved by the caller.
#[allow(clippy::too_many_arguments)]
pub async fn drain(
    connection: &Connection,
    policy_context: &PolicyContext,
    current_epoch: u64,
    node_scope: Scope,
    node_resource: Resource,
    pod_resource: Resource,
    request_id: u64,
    journal: &Journal,
    cancel: &CancellationToken,
) -> DrainReport {
    let cordon_built = workflow::cordon(node_scope.clone(), node_resource, request_id)
        .expect("cordon is always supported for Node");
    let _ = journal.append(executor::record(
        &cordon_built.intent,
        Phase::DrainStarted,
        None,
        &[],
        None,
        None,
    ));
    let cordon_confirmation = Confirmation {
        request_id: cordon_built.intent.request_id,
        scope: cordon_built.intent.target.scope.clone(),
        effect: cordon_built.intent.effect,
        payload_sha256: cordon_built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Strong,
    };
    let cordon_outcome = executor::commit(
        connection,
        policy_context,
        current_epoch,
        &cordon_built.intent,
        Some(&cordon_confirmation),
        cordon_built.payload,
        journal,
        cancel,
    )
    .await;

    let planned = if matches!(
        cordon_outcome,
        MutationOutcome::Committed | MutationOutcome::CommittedButJournalIncomplete
    ) {
        match pods_on_node(connection, &pod_resource, &node_scope.name, cancel).await {
            Ok(pods) => plan_drain(&pods),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let mut steps = Vec::with_capacity(planned.len());
    for (i, pod) in planned.into_iter().enumerate() {
        steps.push(
            run_step(
                connection,
                policy_context,
                current_epoch,
                &node_scope,
                &pod_resource,
                pod,
                request_id + 1 + i as u64,
                journal,
                cancel,
            )
            .await,
        );
    }

    let _ = journal.append(executor::record(
        &cordon_built.intent,
        Phase::DrainFinished,
        None,
        &[],
        Some(&cordon_outcome),
        Some(format!("{} steps", steps.len())),
    ));
    DrainReport {
        cordon_outcome,
        steps,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_step(
    connection: &Connection,
    policy_context: &PolicyContext,
    current_epoch: u64,
    node_scope: &Scope,
    pod_resource: &Resource,
    pod: PlannedPod,
    request_id: u64,
    journal: &Journal,
    cancel: &CancellationToken,
) -> DrainStep {
    if let Some(exclusion) = pod.exclusion {
        return DrainStep {
            namespace: pod.namespace,
            name: pod.name,
            uid: pod.uid,
            outcome: StepOutcome::Excluded(exclusion),
            verification: None,
        };
    }
    if cancel.is_cancelled() {
        return DrainStep {
            namespace: pod.namespace,
            name: pod.name,
            uid: pod.uid,
            outcome: StepOutcome::NotAttempted,
            verification: None,
        };
    }
    let pod_scope = Scope {
        epoch: node_scope.epoch,
        request: node_scope.request,
        context: node_scope.context.clone(),
        cluster: node_scope.cluster.clone(),
        resource: pod_resource.id(),
        namespace: pod.namespace.clone(),
        name: pod.name.clone(),
        uid: pod.uid.clone(),
    };
    let built = workflow::evict(pod_scope, pod_resource.clone(), request_id)
        .expect("evict is always supported for Pod");
    let _ = journal.append(executor::record(
        &built.intent,
        Phase::DrainStep,
        None,
        &[],
        None,
        None,
    ));
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Strong,
    };
    let outcome = executor::commit(
        connection,
        policy_context,
        current_epoch,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        journal,
        cancel,
    )
    .await;
    let verification = if matches!(
        outcome,
        MutationOutcome::Committed | MutationOutcome::CommittedButJournalIncomplete
    ) {
        let v = executor::verify(connection, &built.intent, built.payload.as_ref(), cancel).await;
        let _ = journal.append(executor::verification_record(&built.intent, &v));
        Some(v)
    } else {
        None
    };
    let _ = journal.append(executor::record(
        &built.intent,
        Phase::DrainStep,
        None,
        &[],
        Some(&outcome),
        Some(format!("{outcome:?}")),
    ));
    DrainStep {
        namespace: pod.namespace,
        name: pod.name,
        uid: pod.uid,
        outcome: StepOutcome::Attempted(outcome),
        verification,
    }
}
