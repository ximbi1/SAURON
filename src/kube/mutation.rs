//! M7.3: the single mutation execution gateway. Every future mutation path
//! (M8) must go through `commit`/`preflight` -- never a scattered "just this
//! one patch" call site. No mutation network request may occur from
//! rendering or an idle frame; callers own cancellation. A lock is never held
//! across an await here.
use super::{Connection, discovery::Resource, relationships::read_bounded};
use crate::mutation::{
    Confirmation, MutationEffect, MutationIntent, MutationOutcome, MutationTarget, PolicyDecision,
    Verification,
    journal::{Journal, Phase, Record},
    policy::{self, PolicyContext},
};
use kube::api::{DeleteParams, Patch, PatchParams, PostParams, Preconditions};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

enum Dispatch {
    /// Cancelled before any bytes left this process; definitely not sent.
    NotSent,
    /// The request may or may not have reached/been processed by the server
    /// -- a dropped connection, a timeout waiting for headers, or a stream
    /// error while draining the response body. Never provably "not sent".
    Ambiguous,
    Responded(u16),
}

async fn dispatch(
    connection: &Connection,
    request: http::Request<Vec<u8>>,
    cancel: &CancellationToken,
) -> Dispatch {
    use http_body_util::BodyExt;
    if cancel.is_cancelled() {
        return Dispatch::NotSent;
    }
    let sent = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Dispatch::NotSent,
        result = tokio::time::timeout(
            connection.timeout(),
            connection.client.send(request.map(kube::client::Body::from)),
        ) => result,
    };
    let response = match sent {
        Ok(Ok(response)) => response,
        _ => return Dispatch::Ambiguous,
    };
    let status = response.status().as_u16();
    // Drain and discard the body boundedly; content is never journaled or
    // inspected -- the status code alone drives classification.
    let mut body = response.into_body();
    loop {
        let frame = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Dispatch::Ambiguous,
            frame = body.frame() => frame,
        };
        match frame {
            Some(Ok(_)) => continue,
            Some(Err(_)) => return Dispatch::Ambiguous,
            None => break,
        }
    }
    Dispatch::Responded(status)
}

fn classify(dispatch: Dispatch) -> MutationOutcome {
    match dispatch {
        Dispatch::NotSent => MutationOutcome::Cancelled,
        Dispatch::Ambiguous => MutationOutcome::OutcomeUnknown,
        Dispatch::Responded(200..=299) => MutationOutcome::Committed,
        Dispatch::Responded(401 | 403) => MutationOutcome::Forbidden,
        Dispatch::Responded(404) => MutationOutcome::NotFound,
        Dispatch::Responded(409) => MutationOutcome::Conflict,
        // M8B.4: a PodDisruptionBudget denial, distinct from an ordinary
        // 409 Conflict -- never treated as the same fact, and callers
        // must never retry or fall back to a plain delete on this.
        Dispatch::Responded(429) => MutationOutcome::DisruptionBudgetDenied,
        Dispatch::Responded(_) => MutationOutcome::TransportFailure,
    }
}

async fn patch_request(
    connection: &Connection,
    target: &MutationTarget,
    payload: &Value,
    dry_run: bool,
    cancel: &CancellationToken,
) -> MutationOutcome {
    let namespace = target
        .resource
        .namespaced
        .then_some(target.scope.namespace.as_str());
    let api = target.resource.api(connection.client.clone(), namespace);
    let builder = kube::core::Request::new(api.resource_url());
    let params = PatchParams {
        dry_run,
        force: false,
        field_manager: Some("sauron".into()),
        field_validation: None,
    };
    let request = match builder.patch(&target.scope.name, &params, &Patch::Merge(payload)) {
        Ok(r) => r,
        Err(_) => return MutationOutcome::Unsupported,
    };
    classify(dispatch(connection, request, cancel).await)
}

async fn delete_request(
    connection: &Connection,
    target: &MutationTarget,
    dry_run: bool,
    cancel: &CancellationToken,
) -> MutationOutcome {
    let namespace = target
        .resource
        .namespaced
        .then_some(target.scope.namespace.as_str());
    let api = target.resource.api(connection.client.clone(), namespace);
    let builder = kube::core::Request::new(api.resource_url());
    let params = DeleteParams {
        dry_run,
        // Defense in depth alongside the client-side TOCTOU `revalidate` GET:
        // the server itself rejects the delete with 409 Conflict if the live
        // object's UID no longer matches what was observed at preview time,
        // even if this exact request is somehow issued after a replacement.
        preconditions: Some(Preconditions {
            resource_version: None,
            uid: Some(target.scope.uid.clone()),
        }),
        ..Default::default()
    };
    let request = match builder.delete(&target.scope.name, &params) {
        Ok(r) => r,
        Err(_) => return MutationOutcome::Unsupported,
    };
    classify(dispatch(connection, request, cancel).await)
}

/// M8B.6: force delete's own, genuinely separate request path --
/// `delete_request` above is NEVER touched by this function and never
/// gains a `grace_period_seconds` parameter, so it is structurally
/// impossible for a future refactor to leak force semantics into an
/// ordinary `:delete` by sharing a code path. Still carries the same
/// server-side UID precondition as `delete_request` (grace=0 and a
/// precondition are independent `DeleteParams` fields, no conflict).
async fn force_delete_request(
    connection: &Connection,
    target: &MutationTarget,
    dry_run: bool,
    cancel: &CancellationToken,
) -> MutationOutcome {
    let namespace = target
        .resource
        .namespaced
        .then_some(target.scope.namespace.as_str());
    let api = target.resource.api(connection.client.clone(), namespace);
    let builder = kube::core::Request::new(api.resource_url());
    let params = DeleteParams {
        dry_run,
        grace_period_seconds: Some(0),
        preconditions: Some(Preconditions {
            resource_version: None,
            uid: Some(target.scope.uid.clone()),
        }),
        ..Default::default()
    };
    let request = match builder.delete(&target.scope.name, &params) {
        Ok(r) => r,
        Err(_) => return MutationOutcome::Unsupported,
    };
    classify(dispatch(connection, request, cancel).await)
}

/// M8B.4: the eviction subresource -- never a plain DELETE. Unlike
/// `delete_request`, there is no UID precondition field on the Eviction
/// API itself, so client-side TOCTOU (`revalidate`, called before this
/// like every other commit) is the only defense here, documented as a
/// narrower belt than Delete's own belt-and-suspenders. A 429 response
/// means the API server itself refused because of a PodDisruptionBudget
/// -- `classify` turns that into `MutationOutcome::DisruptionBudgetDenied`,
/// never a fallback to a plain delete, which this function structurally
/// cannot do (it only ever POSTs to the `eviction` subresource).
async fn evict_request(
    connection: &Connection,
    target: &MutationTarget,
    dry_run: bool,
    cancel: &CancellationToken,
) -> MutationOutcome {
    let namespace = target
        .resource
        .namespaced
        .then_some(target.scope.namespace.as_str());
    let api = target.resource.api(connection.client.clone(), namespace);
    let builder = kube::core::Request::new(api.resource_url());
    let body = serde_json::json!({
        "apiVersion": "policy/v1",
        "kind": "Eviction",
        "metadata": {"name": target.scope.name, "namespace": target.scope.namespace},
    });
    let data = match serde_json::to_vec(&body) {
        Ok(d) => d,
        Err(_) => return MutationOutcome::Unsupported,
    };
    // The standard `?dryRun=All` query parameter (the same mechanism every
    // other POST/PATCH in this module already uses) applies to subresource
    // creates too -- no need for the Eviction body's own `deleteOptions.dryRun`.
    let params = PostParams {
        dry_run,
        field_manager: Some("sauron".into()),
    };
    let request = match builder.create_subresource("eviction", &target.scope.name, &params, data) {
        Ok(r) => r,
        Err(_) => return MutationOutcome::Unsupported,
    };
    classify(dispatch(connection, request, cancel).await)
}

/// M8B.3: the only `Create` request path. `create_resource` is the
/// resource being created (e.g. Job) -- deliberately NOT
/// `target.resource` (the source object, e.g. CronJob, whose UID is
/// TOCTOU-revalidated elsewhere); `payload` is a full object manifest,
/// never a patch.
async fn create_request(
    connection: &Connection,
    create_resource: &Resource,
    namespace: &str,
    payload: &Value,
    dry_run: bool,
    cancel: &CancellationToken,
) -> MutationOutcome {
    let ns = create_resource.namespaced.then_some(namespace);
    let api = create_resource.api(connection.client.clone(), ns);
    let builder = kube::core::Request::new(api.resource_url());
    let params = PostParams {
        dry_run,
        field_manager: Some("sauron".into()),
    };
    let data = match serde_json::to_vec(payload) {
        Ok(d) => d,
        Err(_) => return MutationOutcome::Unsupported,
    };
    let request = match builder.create(&params, data) {
        Ok(r) => r,
        Err(_) => return MutationOutcome::Unsupported,
    };
    classify(dispatch(connection, request, cancel).await)
}

/// Server dry-run only. On the isolated fixture cluster during acceptance,
/// or wherever policy has already allowed it -- callers must not interpret
/// success here as proof a later commit will succeed (DRY-RUN SUCCESS ≠
/// COMMIT SUCCESS). Journals `PreflightStarted`/`PreflightResult` so the
/// audit trail always distinguishes a dry run from a real commit, even
/// though both share `MutationOutcome`'s success variant.
pub async fn preflight(
    connection: &Connection,
    intent: &MutationIntent,
    payload: Option<&Value>,
    journal: &Journal,
    cancel: &CancellationToken,
) -> MutationOutcome {
    let _ = journal.append(record(
        intent,
        Phase::PreflightStarted,
        None,
        &[],
        None,
        None,
    ));
    if cancel.is_cancelled() {
        return MutationOutcome::Cancelled;
    }
    let outcome = match intent.effect {
        // M8B.4: evict uses the same Delete effect as an ordinary delete
        // for policy-classification purposes (same risk/confirmation
        // question either way), but the executor dispatches to a
        // distinct request function based on `source_action` -- mirroring
        // Restart/Label both using `Modify` despite being distinct actions.
        MutationEffect::Delete if intent.source_action == "evict" => {
            evict_request(connection, &intent.target, true, cancel).await
        }
        MutationEffect::Delete if intent.source_action == "force_delete" => {
            force_delete_request(connection, &intent.target, true, cancel).await
        }
        MutationEffect::Delete => delete_request(connection, &intent.target, true, cancel).await,
        MutationEffect::Modify => match payload {
            Some(p) => patch_request(connection, &intent.target, p, true, cancel).await,
            None => MutationOutcome::Unsupported,
        },
        // M8B.3: the first real Create -- needs both a full object body
        // and the resource actually being created, distinct from
        // `target.resource` (see `MutationIntent::create_resource`'s own
        // doc comment).
        MutationEffect::Create => match (&intent.create_resource, payload) {
            (Some(create_resource), Some(p)) => {
                create_request(
                    connection,
                    create_resource,
                    &intent.target.scope.namespace,
                    p,
                    true,
                    cancel,
                )
                .await
            }
            _ => MutationOutcome::Unsupported,
        },
    };
    let _ = journal.append(record(
        intent,
        Phase::PreflightResult,
        None,
        &[],
        Some(&outcome),
        Some(format!("{outcome:?}")),
    ));
    outcome
}

/// Fresh metadata-only GET immediately before a real mutation: TOCTOU
/// protection. PREVIEW TIME IDENTITY ≠ COMMIT TIME IDENTITY. A same-name/
/// new-UID replacement is rejected here, never silently retargeted.
async fn revalidate(
    connection: &Connection,
    target: &MutationTarget,
    cancel: &CancellationToken,
) -> Result<Option<String>, MutationOutcome> {
    let namespace = target
        .resource
        .namespaced
        .then_some(target.scope.namespace.as_str());
    let api = target.resource.api(connection.client.clone(), namespace);
    let value = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(MutationOutcome::Cancelled),
        result = read_bounded(connection, api.resource_url(), Some(&target.scope.name), true) => result,
    }
    .map_err(|reason| match reason {
            crate::evidence::Unknown::Forbidden => MutationOutcome::Forbidden,
            crate::evidence::Unknown::NotFound => MutationOutcome::NotFound,
            crate::evidence::Unknown::Stale => MutationOutcome::Cancelled,
            crate::evidence::Unknown::TimedOut => MutationOutcome::TimedOutBeforeSend,
            crate::evidence::Unknown::Unsupported => MutationOutcome::Unsupported,
            _ => MutationOutcome::TransportFailure,
        })?;
    let uid = value.pointer("/metadata/uid").and_then(Value::as_str);
    if uid != Some(target.scope.uid.as_str()) {
        return Err(MutationOutcome::TargetReplaced);
    }
    let resource_version = value
        .pointer("/metadata/resourceVersion")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(expected) = &target.expected_resource_version
        && Some(expected) != resource_version.as_ref()
    {
        return Err(MutationOutcome::Conflict);
    }
    Ok(resource_version)
}

pub(crate) fn record(
    intent: &MutationIntent,
    phase: Phase,
    decision: Option<PolicyDecision>,
    reasons: &[String],
    outcome: Option<&MutationOutcome>,
    detail: Option<String>,
) -> Record {
    Record {
        schema_version: crate::mutation::journal::SCHEMA_VERSION,
        timestamp: chrono::Utc::now().to_rfc3339(),
        request_id: intent.request_id,
        phase,
        context: intent.target.scope.context.clone(),
        cluster: intent.target.scope.cluster.clone(),
        resource: intent.target.scope.resource.clone(),
        namespace: intent.target.scope.namespace.clone(),
        name: intent.target.scope.name.clone(),
        uid: intent.target.scope.uid.clone(),
        effect: format!("{:?}", intent.effect),
        summary: intent.summary.clone(),
        payload_sha256: intent.payload_sha256.clone(),
        policy_decision: decision.map(|d| format!("{d:?}")),
        policy_reasons: reasons.to_vec(),
        outcome: outcome.map(|o| format!("{o:?}")),
        detail,
    }
}

/// The one gateway. Re-evaluates policy at commit time (never trusts a
/// stale preview decision), requires a confirmation that exactly authorizes
/// this intent when policy demands one, revalidates the context epoch and
/// the target's live UID/resourceVersion immediately before mutating,
/// journals before sending, issues exactly one bounded/cancellable request,
/// and journals the result -- a post-commit journal failure is reported as
/// `CommittedButJournalIncomplete`, never as if nothing happened.
#[allow(clippy::too_many_arguments)]
pub async fn commit(
    connection: &Connection,
    policy_context: &PolicyContext,
    current_epoch: u64,
    intent: &MutationIntent,
    confirmation: Option<&Confirmation>,
    payload: Option<Value>,
    journal: &Journal,
    cancel: &CancellationToken,
) -> MutationOutcome {
    let evaluation = policy::evaluate(policy_context, intent);
    let reasons: Vec<String> = evaluation
        .reasons
        .iter()
        .map(|r| format!("{r:?}"))
        .collect();
    if journal
        .append(record(
            intent,
            Phase::PolicyEvaluated,
            Some(evaluation.decision),
            &reasons,
            None,
            None,
        ))
        .is_err()
    {
        return MutationOutcome::Denied;
    }
    if matches!(
        evaluation.decision,
        PolicyDecision::Deny | PolicyDecision::Unsupported
    ) {
        return MutationOutcome::Denied;
    }
    let needs_confirmation = matches!(
        evaluation.decision,
        PolicyDecision::RequireConfirmation | PolicyDecision::RequireStrongerConfirmation
    );
    if needs_confirmation && !confirmation.is_some_and(|c| c.authorizes(intent)) {
        return MutationOutcome::Denied;
    }
    if cancel.is_cancelled() {
        return MutationOutcome::Cancelled;
    }
    if intent.target.scope.epoch != current_epoch {
        return MutationOutcome::TargetReplaced;
    }
    if let Err(outcome) = revalidate(connection, &intent.target, cancel).await {
        return outcome;
    }
    if journal
        .append(record(
            intent,
            Phase::CommitStarted,
            Some(evaluation.decision),
            &reasons,
            None,
            None,
        ))
        .is_err()
    {
        return MutationOutcome::Denied;
    }
    if cancel.is_cancelled() {
        return MutationOutcome::Cancelled;
    }
    let outcome = match intent.effect {
        MutationEffect::Delete if intent.source_action == "evict" => {
            evict_request(connection, &intent.target, false, cancel).await
        }
        MutationEffect::Delete if intent.source_action == "force_delete" => {
            force_delete_request(connection, &intent.target, false, cancel).await
        }
        MutationEffect::Delete => delete_request(connection, &intent.target, false, cancel).await,
        MutationEffect::Modify => match &payload {
            Some(p) => patch_request(connection, &intent.target, p, false, cancel).await,
            None => MutationOutcome::Unsupported,
        },
        MutationEffect::Create => match (&intent.create_resource, &payload) {
            (Some(create_resource), Some(p)) => {
                create_request(
                    connection,
                    create_resource,
                    &intent.target.scope.namespace,
                    p,
                    false,
                    cancel,
                )
                .await
            }
            _ => MutationOutcome::Unsupported,
        },
    };
    let wrote = journal
        .append(record(
            intent,
            Phase::CommitResult,
            Some(evaluation.decision),
            &reasons,
            Some(&outcome),
            // M8B.6: an auditor must be able to tell a force delete apart
            // from an ordinary one directly from this one record, without
            // cross-referencing `source_action` elsewhere.
            Some(if intent.source_action == "force_delete" {
                format!("{outcome:?} (grace_period_seconds=0)")
            } else {
                format!("{outcome:?}")
            }),
        ))
        .is_ok();
    if !wrote && matches!(outcome, MutationOutcome::Committed) {
        return MutationOutcome::CommittedButJournalIncomplete;
    }
    outcome
}

/// Walks a single-leaf JSON merge-patch document (exactly what every M8
/// workflow builds: one field, however deeply nested) down to its leaf,
/// returning a JSON pointer path and the expected value at that path.
/// `None` if the payload does not have this shape -- verification then
/// reports `Unknown` rather than guessing. RFC 6901 escaping matters here:
/// annotation keys routinely contain literal `/` (e.g.
/// `kubectl.kubernetes.io/restartedAt`), which must become `~1` inside the
/// pointer, not a path separator.
fn leaf_path_and_value(payload: &Value) -> Option<(String, Value)> {
    let mut path = String::new();
    let mut node = payload;
    loop {
        let Value::Object(map) = node else {
            return None;
        };
        if map.len() != 1 {
            return None;
        }
        let (key, value) = map.iter().next()?;
        path.push('/');
        path.push_str(&key.replace('~', "~0").replace('/', "~1"));
        match value {
            Value::Object(_) => node = value,
            other => return Some((path, other.clone())),
        }
    }
}

/// M8.5: one bounded, cancellable fresh GET immediately after a successful
/// commit, comparing the observed value at the payload's own leaf path to
/// what was requested. Never retried, never blocks indefinitely -- a
/// timeout or transport failure reports `Unknown`, never treated as the
/// mutation having failed (the server already accepted the write).
async fn verify_modify(
    connection: &Connection,
    target: &MutationTarget,
    path: &str,
    expected: &Value,
    cancel: &CancellationToken,
) -> Verification {
    let namespace = target
        .resource
        .namespaced
        .then_some(target.scope.namespace.as_str());
    let api = target.resource.api(connection.client.clone(), namespace);
    let fetch = read_bounded(
        connection,
        api.resource_url(),
        Some(&target.scope.name),
        false,
    );
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Verification::Unknown,
        result = tokio::time::timeout(connection.timeout(), fetch) => result,
    };
    let value = match result {
        Ok(Ok(value)) => value,
        Ok(Err(crate::evidence::Unknown::NotFound)) => {
            return Verification::ObservedDifferent("object no longer exists".into());
        }
        _ => return Verification::Unknown,
    };
    let uid = value.pointer("/metadata/uid").and_then(Value::as_str);
    if uid != Some(target.scope.uid.as_str()) {
        return Verification::TargetReplaced;
    }
    let observed = value.pointer(path);
    let matches = match expected {
        Value::Null => observed.is_none() || observed == Some(&Value::Null),
        // Kubernetes' own `omitempty` convention drops a `false` boolean
        // field from the serialized object entirely (e.g. Node's
        // `spec.unschedulable` is absent when schedulable, not present as
        // `false`) -- absent and explicit-false are the same observed
        // fact for such a field, never a mismatch.
        Value::Bool(false) => observed.is_none() || observed == Some(&Value::Bool(false)),
        other => observed == Some(other),
    };
    if matches {
        Verification::Verified
    } else {
        Verification::ObservedDifferent(format!(
            "expected {path}={expected}, observed {path}={}",
            observed
                .map(ToString::to_string)
                .unwrap_or_else(|| "<absent>".into())
        ))
    }
}

/// M8.5: after a successful delete, `deletionTimestamp` means accepted and
/// in progress; a later 404 (or the same name resolving to a different UID)
/// means observed gone. Still fully present with no `deletionTimestamp` is
/// `Pending`, not a problem -- an ordinary eventual-consistency window,
/// never polled in a loop here.
async fn verify_delete(
    connection: &Connection,
    target: &MutationTarget,
    cancel: &CancellationToken,
) -> Verification {
    let namespace = target
        .resource
        .namespaced
        .then_some(target.scope.namespace.as_str());
    let api = target.resource.api(connection.client.clone(), namespace);
    let fetch = read_bounded(
        connection,
        api.resource_url(),
        Some(&target.scope.name),
        true,
    );
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Verification::Unknown,
        result = tokio::time::timeout(connection.timeout(), fetch) => result,
    };
    match result {
        Ok(Ok(value)) => {
            let uid = value.pointer("/metadata/uid").and_then(Value::as_str);
            if uid != Some(target.scope.uid.as_str()) {
                return Verification::ObservedGone;
            }
            if value.pointer("/metadata/deletionTimestamp").is_some() {
                Verification::DeletionInProgress
            } else {
                Verification::Pending
            }
        }
        Ok(Err(crate::evidence::Unknown::NotFound)) => Verification::ObservedGone,
        _ => Verification::Unknown,
    }
}

/// M8.5: the one place verification is attempted -- exactly once, only
/// after a real commit (`Committed`/`CommittedButJournalIncomplete`; callers
/// must not call this for any other outcome). `Create` has no verification
/// path yet, matching its documented unsupported status elsewhere in this
/// module.
pub async fn verify(
    connection: &Connection,
    intent: &MutationIntent,
    payload: Option<&Value>,
    cancel: &CancellationToken,
) -> Verification {
    match intent.effect {
        MutationEffect::Delete => verify_delete(connection, &intent.target, cancel).await,
        // set_image's payload replaces the whole `containers` array (see
        // its builder's own doc comment), so `leaf_path_and_value`'s
        // single-scalar-leaf assumption does not apply -- it needs its
        // own name-keyed comparison, checked first and explicitly, never
        // silently falling through to the generic path below.
        MutationEffect::Modify if intent.source_action == "set_image" => match payload {
            Some(p) => verify_set_image(connection, &intent.target, p, cancel).await,
            None => Verification::Unknown,
        },
        MutationEffect::Modify => match payload.and_then(leaf_path_and_value) {
            Some((path, expected)) => {
                verify_modify(connection, &intent.target, &path, &expected, cancel).await
            }
            None => Verification::Unknown,
        },
        MutationEffect::Create => match (&intent.create_resource, payload) {
            (Some(create_resource), Some(p)) => {
                verify_create(
                    connection,
                    create_resource,
                    &intent.target.scope.namespace,
                    p,
                    cancel,
                )
                .await
            }
            _ => Verification::Unknown,
        },
    }
}

/// M8B.3: confirms the newly created object exists under its own
/// deterministic, previously-generated name (read from the payload's own
/// `metadata.name`, never re-derived) -- a fresh GET, exactly once,
/// bounded and cancellable like every other verification path. `Pending`
/// on a 404 (creation just accepted, not yet visible), never treated as
/// failure.
async fn verify_create(
    connection: &Connection,
    create_resource: &Resource,
    namespace: &str,
    payload: &Value,
    cancel: &CancellationToken,
) -> Verification {
    let Some(name) = payload.pointer("/metadata/name").and_then(Value::as_str) else {
        return Verification::Unknown;
    };
    let ns = create_resource.namespaced.then_some(namespace);
    let api = create_resource.api(connection.client.clone(), ns);
    let fetch = read_bounded(connection, api.resource_url(), Some(name), false);
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Verification::Unknown,
        result = tokio::time::timeout(connection.timeout(), fetch) => result,
    };
    match result {
        Ok(Ok(value)) => {
            let observed_name = value.pointer("/metadata/name").and_then(Value::as_str);
            if observed_name == Some(name) {
                Verification::Created(format!("{name} exists"))
            } else {
                Verification::ObservedDifferent(format!(
                    "expected name {name}, observed {observed_name:?}"
                ))
            }
        }
        Ok(Err(crate::evidence::Unknown::NotFound)) => Verification::Pending,
        _ => Verification::Unknown,
    }
}

/// M8B.2: verifies every container named in the payload's own
/// reconstructed `containers` array matches the live object's container
/// of the same name by `image` -- not just the one that changed, so an
/// accidental cross-container corruption from the builder's own
/// full-array reconstruction would also be caught here, not only by unit
/// tests. Container order is never assumed stable; matching is by `name`.
async fn verify_set_image(
    connection: &Connection,
    target: &MutationTarget,
    payload: &Value,
    cancel: &CancellationToken,
) -> Verification {
    let expected_containers = match payload.pointer("/spec/template/spec/containers") {
        Some(Value::Array(containers)) => containers,
        _ => return Verification::Unknown,
    };
    let namespace = target
        .resource
        .namespaced
        .then_some(target.scope.namespace.as_str());
    let api = target.resource.api(connection.client.clone(), namespace);
    let fetch = read_bounded(
        connection,
        api.resource_url(),
        Some(&target.scope.name),
        false,
    );
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Verification::Unknown,
        result = tokio::time::timeout(connection.timeout(), fetch) => result,
    };
    let value = match result {
        Ok(Ok(value)) => value,
        Ok(Err(crate::evidence::Unknown::NotFound)) => {
            return Verification::ObservedDifferent("object no longer exists".into());
        }
        _ => return Verification::Unknown,
    };
    let uid = value.pointer("/metadata/uid").and_then(Value::as_str);
    if uid != Some(target.scope.uid.as_str()) {
        return Verification::TargetReplaced;
    }
    let observed_containers = match value.pointer("/spec/template/spec/containers") {
        Some(Value::Array(containers)) => containers,
        _ => {
            return Verification::ObservedDifferent(
                "containers not found on fresh observation".into(),
            );
        }
    };
    for expected in expected_containers {
        let name = expected.get("name").and_then(Value::as_str);
        let expected_image = expected.get("image").and_then(Value::as_str);
        let observed_image = observed_containers
            .iter()
            .find(|c| c.get("name").and_then(Value::as_str) == name)
            .and_then(|c| c.get("image"))
            .and_then(Value::as_str);
        if observed_image != expected_image {
            return Verification::ObservedDifferent(format!(
                "container \"{}\" expected image {:?}, observed {:?}",
                name.unwrap_or("<unnamed>"),
                expected_image,
                observed_image
            ));
        }
    }
    Verification::Verified
}

/// A `Phase::VerificationResult` record correlated by `request_id` to the
/// `CommitResult` it follows -- callers append this themselves (verify()
/// itself does not journal, matching the same separation `preflight`/
/// `commit` already keep between the network call and the audit write).
pub fn verification_record(intent: &MutationIntent, verification: &Verification) -> Record {
    record(
        intent,
        Phase::VerificationResult,
        None,
        &[],
        None,
        Some(format!("{verification:?}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_path_and_value_walks_a_single_key_chain_to_its_leaf() {
        let (path, value) = leaf_path_and_value(&serde_json::json!({"spec": {"replicas": 5}}))
            .expect("single-leaf payload");
        assert_eq!(path, "/spec/replicas");
        assert_eq!(value, serde_json::json!(5));
    }

    #[test]
    fn leaf_path_and_value_escapes_slashes_in_annotation_keys_per_rfc6901() {
        let (path, value) = leaf_path_and_value(&serde_json::json!({
            "spec": {"template": {"metadata": {"annotations": {
                "kubectl.kubernetes.io/restartedAt": "2026-09-18T00:00:00Z"
            }}}}
        }))
        .expect("nested single-leaf payload");
        assert_eq!(
            path,
            "/spec/template/metadata/annotations/kubectl.kubernetes.io~1restartedAt"
        );
        assert_eq!(value, serde_json::json!("2026-09-18T00:00:00Z"));
    }

    #[test]
    fn leaf_path_and_value_handles_a_removal_null_leaf() {
        let (path, value) =
            leaf_path_and_value(&serde_json::json!({"metadata": {"labels": {"team": null}}}))
                .expect("removal payload");
        assert_eq!(path, "/metadata/labels/team");
        assert_eq!(value, serde_json::Value::Null);
    }

    #[test]
    fn leaf_path_and_value_rejects_a_multi_key_object_rather_than_guessing() {
        assert!(
            leaf_path_and_value(&serde_json::json!({"spec": {"replicas": 5, "paused": true}}))
                .is_none()
        );
    }

    #[test]
    fn leaf_path_and_value_rejects_a_non_object_payload() {
        assert!(leaf_path_and_value(&serde_json::json!(5)).is_none());
    }
}
