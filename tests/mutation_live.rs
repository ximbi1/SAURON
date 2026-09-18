//! Run only through the identity-guarded `scripts/test-cluster.sh m7-test`,
//! against the isolated `sauron-m7` fixture namespace. This is the ONE
//! narrowly-scoped internal proof mutation for M7: it patches a harmless
//! annotation on `m7-target`, never a general mutation escape hatch.
//!
//! A single sequential test, like `relationships_live.rs`: multiple
//! `#[tokio::test]` functions sharing one live fixture object would race
//! each other under cargo's default parallel test execution.
use sauron::{
    app::session::Scope,
    config::Config,
    kube::{self, ConnectOptions, mutation as executor},
    mutation::{
        Confirmation, ConfirmationRequirement, MutationEffect, MutationIntent, MutationOutcome,
        MutationRisk, MutationTarget,
        journal::{Journal, Phase},
        policy::{self, PolicyContext},
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

fn new_journal() -> (Journal, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("sauron-m7-live-journal-{}", std::process::id()));
    (Journal::new(dir.join("journal.jsonl")), dir)
}

fn stable_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

#[tokio::test]
#[ignore = "explicit isolated kind config and m7-fixtures required"]
async fn live_proof_mutation_and_replacement_rejection() {
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
    let resource = c
        .catalog
        .resolve("v1/configmaps", &c.settings.aliases)
        .expect("canonical configmap");
    let api = resource.api(c.client.clone(), Some("sauron-m7"));
    let cancel = CancellationToken::new();

    // --- Phase 1: preview -> dry-run -> confirmation -> commit -> verify -> journal ---
    let fixture = api.get("m7-target").await.expect("fixture");
    let fixture = Object::new(serde_json::to_value(fixture).unwrap());
    let nonce = format!("nonce-{}", std::process::id());
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: resource.id(),
        namespace: fixture.namespace.clone(),
        name: fixture.name.clone(),
        uid: fixture.uid.clone(),
    };
    let payload = serde_json::json!({"metadata":{"annotations":{"m7-proof": nonce}}});
    let payload_hash = format!("{:x}", stable_hash(&payload.to_string()));
    let intent = MutationIntent {
        request_id: 7001,
        target: MutationTarget {
            scope: scope.clone(),
            resource: resource.clone(),
            expected_resource_version: None,
        },
        effect: MutationEffect::Modify,
        risk: MutationRisk::Routine,
        summary: "metadata.annotations[\"m7-proof\"]".into(),
        payload_sha256: Some(payload_hash),
        source_action: "m7_live_proof".into(),
    };

    // 1: preview -- pure local policy evaluation, no network.
    let evaluation = policy::evaluate(&verified_policy(), &intent);
    assert!(
        matches!(
            evaluation.decision,
            sauron::mutation::PolicyDecision::RequireConfirmation
                | sauron::mutation::PolicyDecision::RequireStrongerConfirmation
        ),
        "a routine namespaced ConfigMap Modify must require confirmation, not Allow silently: {evaluation:?}"
    );

    // 2: server dry-run -- never proof that commit will succeed.
    let (journal, dir) = new_journal();
    let dry_run_outcome = executor::preflight(&c, &intent, Some(&payload), &journal, &cancel).await;
    assert_eq!(dry_run_outcome, MutationOutcome::Committed);
    let after_dry_run = api
        .get("m7-target")
        .await
        .expect("fixture unchanged after dry run");
    assert!(
        after_dry_run
            .metadata
            .annotations
            .as_ref()
            .is_none_or(|a| !a.contains_key("m7-proof")),
        "a server dry-run must never actually persist the change"
    );

    // 3: confirmation bound to the exact intent.
    let confirmation = Confirmation {
        request_id: intent.request_id,
        scope: intent.target.scope.clone(),
        effect: intent.effect,
        payload_sha256: intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    assert!(confirmation.authorizes(&intent));

    // 4: commit -- revalidates, journals, sends exactly one PATCH.
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &intent,
        Some(&confirmation),
        Some(payload.clone()),
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);

    // 5: fresh GET verifies the exact expected effect.
    let after = api.get("m7-target").await.expect("fixture after commit");
    assert_eq!(
        after
            .metadata
            .annotations
            .as_ref()
            .and_then(|a| a.get("m7-proof"))
            .map(String::as_str),
        Some(nonce.as_str()),
    );

    // 6: journal verifies the exact UID/action/outcome, and that the dry
    // run was recorded distinctly from the real commit.
    let records = journal.recent(20);
    assert!(records.iter().any(|r| r.phase == Phase::CommitResult
        && r.outcome.as_deref() == Some("Committed")
        && r.uid == fixture.uid
        && r.request_id == intent.request_id));
    assert!(
        records
            .iter()
            .any(|r| r.phase == Phase::PreflightResult && r.outcome.as_deref() == Some("Committed")),
        "the dry run must be journaled separately from the real commit"
    );
    std::fs::remove_dir_all(&dir).ok();

    // --- Phase 2: same-name/new-UID replacement between preview and commit ---
    let stale_uid = after.metadata.uid.clone().unwrap();
    let replacement_scope = Scope {
        uid: stale_uid,
        ..scope
    };
    let replacement_intent = MutationIntent {
        request_id: 7002,
        target: MutationTarget {
            scope: replacement_scope,
            resource: resource.clone(),
            expected_resource_version: None,
        },
        effect: MutationEffect::Modify,
        risk: MutationRisk::Routine,
        summary: "metadata.annotations[\"m7-proof\"]".into(),
        payload_sha256: Some("hash".into()),
        source_action: "m7_live_proof".into(),
    };
    let replacement_confirmation = Confirmation {
        request_id: replacement_intent.request_id,
        scope: replacement_intent.target.scope.clone(),
        effect: replacement_intent.effect,
        payload_sha256: replacement_intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };

    // Delete + recreate: exact same name, brand new UID.
    api.delete("m7-target", &Default::default())
        .await
        .expect("delete");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    api.create(
        &Default::default(),
        &::kube::core::DynamicObject::new("m7-target", &resource.api)
            .within("sauron-m7")
            .data(serde_json::json!({"fixture": "m7-mutation-proof-only"})),
    )
    .await
    .expect("recreate");

    let (journal2, dir2) = new_journal();
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &replacement_intent,
        Some(&replacement_confirmation),
        Some(serde_json::json!({"metadata":{"annotations":{"m7-proof":"should-never-apply"}}})),
        &journal2,
        &CancellationToken::new(),
    )
    .await;
    assert!(
        matches!(
            outcome,
            MutationOutcome::TargetReplaced | MutationOutcome::NotFound
        ),
        "a deleted/replaced target must never silently accept the stale UID: {outcome:?}"
    );
    let final_state = api.get("m7-target").await.expect("recreated fixture");
    assert!(
        final_state
            .metadata
            .annotations
            .as_ref()
            .is_none_or(|a| !a.contains_key("m7-proof")),
        "the rejected commit must never have touched the replacement object"
    );
    std::fs::remove_dir_all(&dir2).ok();
}
