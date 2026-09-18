//! Run only through the identity-guarded `scripts/test-cluster.sh m8-test`,
//! against the isolated `sauron-m8` fixture namespace. Proves M8.5's
//! verification model (`kube::mutation::verify`) against real API server
//! observations for all four Modify-shaped workflows plus Delete -- never
//! against production, never against the default kubeconfig.
//!
//! A single sequential test, like `relationships_live.rs`/`mutation_live.rs`:
//! multiple `#[tokio::test]` functions sharing the same live fixture objects
//! would race each other under cargo's default parallel test execution.
use sauron::{
    app::session::Scope,
    config::Config,
    kube::{self, ConnectOptions, mutation as executor},
    mutation::{
        Confirmation, ConfirmationRequirement, MutationOutcome, PolicyDecision, Verification,
        journal::{Journal, Phase},
        policy::{self, PolicyContext},
        workflow,
    },
    resources::Object,
};
use tokio_util::sync::CancellationToken;

fn verified_policy() -> PolicyContext {
    PolicyContext {
        readonly: false,
        readonly_forced: false,
        cluster_verified_for_mutation: true,
        ..PolicyContext::default()
    }
}

fn new_journal(label: &str) -> (Journal, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "sauron-m8-live-journal-{label}-{}",
        std::process::id()
    ));
    (Journal::new(dir.join("journal.jsonl")), dir)
}

#[tokio::test]
#[ignore = "explicit isolated kind config and m8-fixtures required"]
async fn live_m8_5_verification_matches_real_cluster_observations() {
    let path = std::env::var_os("SAURON_TEST_KUBECONFIG").expect("explicit test kubeconfig");
    let c = kube::connect(
        ConnectOptions {
            kubeconfig: Some(path.into()),
            context: Some("kind-sauron-test".into()),
            force_readonly: false,
            mutation_test_cluster_verified: false,
        },
        Config::default(),
    )
    .await
    .expect("connect");
    let cancel = CancellationToken::new();
    let mut request_id = 8001u64;
    let mut next_request = || {
        request_id += 1;
        request_id
    };

    // --- SCALE: m8-deploy 1 -> 2, verify observed desired replicas ---
    let deployment_resource = c
        .catalog
        .resolve("apps/v1/deployments", &c.settings.aliases)
        .expect("canonical deployment");
    let deployment_api = deployment_resource.api(c.client.clone(), Some("sauron-m8"));
    let deployment = Object::new(
        serde_json::to_value(deployment_api.get("m8-deploy").await.expect("m8-deploy")).unwrap(),
    );
    let current = deployment
        .value
        .pointer("/spec/replicas")
        .and_then(serde_json::Value::as_i64);
    assert_eq!(current, Some(1), "fixture must start at replicas=1");
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: deployment_resource.id(),
        namespace: deployment.namespace.clone(),
        name: deployment.name.clone(),
        uid: deployment.uid.clone(),
    };
    let request_id = next_request();
    let built = workflow::scale(
        scope.clone(),
        deployment_resource.clone(),
        current,
        2,
        request_id,
    )
    .expect("scale is supported for Deployment");
    let evaluation = policy::evaluate(&verified_policy(), &built.intent);
    assert_eq!(evaluation.decision, PolicyDecision::RequireConfirmation);
    let confirmation = Confirmation {
        request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("scale");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    let verification = executor::verify(&c, &built.intent, built.payload.as_ref(), &cancel).await;
    assert_eq!(
        verification,
        Verification::Verified,
        "a fresh GET must confirm spec.replicas now reads 2"
    );
    std::fs::remove_dir_all(&dir).ok();

    // Restore replicas=1 before the next phase, using the same builder/executor
    // path (never a raw kubectl patch) so the reset is itself proof the same
    // machinery round-trips cleanly in both directions.
    let request_id = next_request();
    let built_back = workflow::scale(
        scope.clone(),
        deployment_resource.clone(),
        Some(2),
        1,
        request_id,
    )
    .unwrap();
    let confirmation_back = Confirmation {
        request_id,
        scope: built_back.intent.target.scope.clone(),
        effect: built_back.intent.effect,
        payload_sha256: built_back.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("scale-reset");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built_back.intent,
        Some(&confirmation_back),
        built_back.payload,
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();

    // --- RESTART: verify the exact template annotation value observed ---
    let request_id = next_request();
    let timestamp = "2026-09-18T00:00:00Z";
    let built = workflow::restart(
        scope.clone(),
        deployment_resource.clone(),
        timestamp,
        request_id,
    )
    .expect("restart is supported for Deployment");
    let confirmation = Confirmation {
        request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("restart");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    let verification = executor::verify(&c, &built.intent, built.payload.as_ref(), &cancel).await;
    assert_eq!(
        verification,
        Verification::Verified,
        "a fresh GET must confirm the exact restartedAt annotation value"
    );
    std::fs::remove_dir_all(&dir).ok();

    // --- LABEL: set then remove on m8-meta; unrelated metadata must survive ---
    let configmap_resource = c
        .catalog
        .resolve("v1/configmaps", &c.settings.aliases)
        .expect("canonical configmap");
    let configmap_api = configmap_resource.api(c.client.clone(), Some("sauron-m8"));
    let configmap = Object::new(
        serde_json::to_value(configmap_api.get("m8-meta").await.expect("m8-meta")).unwrap(),
    );
    let meta_scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: configmap_resource.id(),
        namespace: configmap.namespace.clone(),
        name: configmap.name.clone(),
        uid: configmap.uid.clone(),
    };
    let request_id = next_request();
    let built = workflow::label(
        meta_scope.clone(),
        configmap_resource.clone(),
        "live-proof",
        Some("set"),
        request_id,
    )
    .expect("label key/value are valid");
    let confirmation = Confirmation {
        request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("label-set");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    let verification = executor::verify(&c, &built.intent, built.payload.as_ref(), &cancel).await;
    assert_eq!(verification, Verification::Verified);
    let after_set = configmap_api.get("m8-meta").await.expect("m8-meta");
    assert_eq!(
        after_set
            .metadata
            .labels
            .as_ref()
            .and_then(|l| l.get("kept"))
            .map(String::as_str),
        Some("unrelated-label-must-survive"),
        "setting one label must never disturb an unrelated existing label"
    );
    std::fs::remove_dir_all(&dir).ok();

    let request_id = next_request();
    let built = workflow::label(
        meta_scope.clone(),
        configmap_resource.clone(),
        "live-proof",
        None,
        request_id,
    )
    .expect("label removal");
    let confirmation = Confirmation {
        request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("label-remove");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    let verification = executor::verify(&c, &built.intent, built.payload.as_ref(), &cancel).await;
    assert_eq!(verification, Verification::Verified);
    let after_remove = configmap_api.get("m8-meta").await.expect("m8-meta");
    assert!(
        after_remove
            .metadata
            .labels
            .as_ref()
            .is_none_or(|l| !l.contains_key("live-proof")),
        "the removed label must actually be gone"
    );
    assert_eq!(
        after_remove
            .metadata
            .labels
            .as_ref()
            .and_then(|l| l.get("kept"))
            .map(String::as_str),
        Some("unrelated-label-must-survive"),
        "removing one label must never disturb an unrelated existing label"
    );
    std::fs::remove_dir_all(&dir).ok();

    // --- ANNOTATE: same set/remove grammar, unrelated annotation preserved ---
    let request_id = next_request();
    let built = workflow::annotate(
        meta_scope.clone(),
        configmap_resource.clone(),
        "live-proof",
        Some("set"),
        request_id,
    )
    .expect("annotation key/value are valid");
    let confirmation = Confirmation {
        request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("annotate-set");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    let verification = executor::verify(&c, &built.intent, built.payload.as_ref(), &cancel).await;
    assert_eq!(verification, Verification::Verified);
    let after_set = configmap_api.get("m8-meta").await.expect("m8-meta");
    assert_eq!(
        after_set
            .metadata
            .annotations
            .as_ref()
            .and_then(|a| a.get("kept"))
            .map(String::as_str),
        Some("unrelated-annotation-must-survive"),
    );
    std::fs::remove_dir_all(&dir).ok();

    let request_id = next_request();
    let built = workflow::annotate(
        meta_scope,
        configmap_resource,
        "live-proof",
        None,
        request_id,
    )
    .expect("annotation removal");
    let confirmation = Confirmation {
        request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("annotate-remove");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    let verification = executor::verify(&c, &built.intent, built.payload.as_ref(), &cancel).await;
    assert_eq!(verification, Verification::Verified);
    std::fs::remove_dir_all(&dir).ok();

    // --- DELETE: m8-pod -- accepted, then deletionTimestamp/gone, never blocking ---
    let pod_resource = c
        .catalog
        .resolve("v1/pods", &c.settings.aliases)
        .expect("canonical pod");
    let pod_api = pod_resource.api(c.client.clone(), Some("sauron-m8"));
    let pod =
        Object::new(serde_json::to_value(pod_api.get("m8-pod").await.expect("m8-pod")).unwrap());
    let pod_scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: pod_resource.id(),
        namespace: pod.namespace.clone(),
        name: pod.name.clone(),
        uid: pod.uid.clone(),
    };
    let request_id = next_request();
    let built =
        workflow::delete(pod_scope, pod_resource, request_id).expect("delete is supported for Pod");
    let confirmation = Confirmation {
        request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Strong,
    };
    let (journal, dir) = new_journal("delete");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built.intent,
        Some(&confirmation),
        None,
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed, "delete accepted");
    // Bounded, finite polling in the TEST HARNESS ONLY (never inside the app
    // itself, which attempts exactly one verification) -- proving the delete
    // eventually resolves to ObservedGone without ever blocking indefinitely.
    let mut last = Verification::Unknown;
    let mut resolved_gone = false;
    for _ in 0..20 {
        last = executor::verify(&c, &built.intent, None, &cancel).await;
        if last == Verification::ObservedGone {
            resolved_gone = true;
            break;
        }
        assert!(
            matches!(
                last,
                Verification::DeletionInProgress | Verification::Pending
            ),
            "unexpected delete verification state: {last:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(
        resolved_gone,
        "delete never resolved to ObservedGone within the bounded poll window (last: {last:?})"
    );
    journal
        .append(executor::verification_record(&built.intent, &last))
        .expect("journal verification result");
    let records = journal.recent(20);
    assert!(
        records.iter().any(
            |r| r.phase == Phase::VerificationResult && r.request_id == confirmation.request_id
        )
    );
    std::fs::remove_dir_all(&dir).ok();
}
