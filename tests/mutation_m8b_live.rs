//! Run only through the identity-guarded `scripts/test-cluster.sh m8b-test`.
//! Proves M8B.1 (Cordon/Uncordon) against the real Node of
//! `kind-sauron-test` and M8B.2 (Set Image) against a dedicated
//! two-container Deployment fixture. The cordon test cordons and
//! immediately uncordons the cluster's ONE node, briefly and within one
//! sequential test function -- never left cordoned, never held cordoned
//! across multiple test functions (which would race under cargo's default
//! parallel test execution, matching every other `*_live.rs` file's own
//! precedent). The set_image test targets a distinct object (`m8b-multi`),
//! so it is a separate test function without racing the cordon test.
use sauron::{
    app::session::Scope,
    config::Config,
    kube::{self, ConnectOptions, mutation as executor},
    mutation::{
        Confirmation, ConfirmationRequirement, MutationOutcome, PolicyDecision, Verification,
        policy, workflow,
    },
    resources::Object,
};
use tokio_util::sync::CancellationToken;

fn verified_policy() -> policy::PolicyContext {
    policy::PolicyContext {
        readonly: false,
        readonly_forced: false,
        cluster_verified_for_mutation: true,
        ..policy::PolicyContext::default()
    }
}

#[tokio::test]
#[ignore = "explicit isolated kind config required; cordons/uncordons the cluster's only node briefly"]
async fn live_cordon_then_uncordon_the_single_node() {
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
    let node_resource = c
        .catalog
        .resolve("v1/nodes", &c.settings.aliases)
        .expect("canonical node");
    let node_api = node_resource.api(c.client.clone(), None);
    let nodes: Vec<_> = node_api
        .list(&Default::default())
        .await
        .expect("list nodes")
        .items;
    assert_eq!(
        nodes.len(),
        1,
        "this live test assumes the single-node kind default"
    );
    let node = Object::new(serde_json::to_value(&nodes[0]).unwrap());
    assert_eq!(
        node.value.pointer("/spec/unschedulable"),
        None,
        "node must start schedulable (see scripts/test-cluster.sh m8b-fixtures)"
    );
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: node_resource.id(),
        namespace: node.namespace.clone(),
        name: node.name.clone(),
        uid: node.uid.clone(),
    };
    let cancel = CancellationToken::new();

    // --- Cordon ---
    let built = workflow::cordon(scope.clone(), node_resource.clone(), 1)
        .expect("cordon is supported for Node");
    let evaluation = policy::evaluate(&verified_policy(), &built.intent);
    assert_eq!(
        evaluation.decision,
        PolicyDecision::RequireStrongerConfirmation
    );
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Strong,
    };
    let (journal, dir) = new_journal("cordon");
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
        "spec.unschedulable must read back true"
    );
    std::fs::remove_dir_all(&dir).ok();

    let after_cordon = node_api.get(&node.name).await.expect("node after cordon");
    assert_eq!(
        after_cordon.data.pointer("/spec/unschedulable"),
        Some(&serde_json::Value::Bool(true))
    );

    // --- Uncordon (same session, immediately) ---
    let built =
        workflow::uncordon(scope, node_resource, 2).expect("uncordon is supported for Node");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Strong,
    };
    let (journal, dir) = new_journal("uncordon");
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
        "spec.unschedulable must read back false"
    );
    std::fs::remove_dir_all(&dir).ok();

    let after_uncordon = node_api.get(&node.name).await.expect("node after uncordon");
    assert_ne!(
        after_uncordon.data.pointer("/spec/unschedulable"),
        Some(&serde_json::Value::Bool(true)),
        "node must be schedulable again -- this test must never leave the cluster's only node cordoned"
    );
}

#[tokio::test]
#[ignore = "explicit isolated kind config and m8b-fixtures (m8b-multi Deployment) required"]
async fn live_set_image_changes_only_the_named_container() {
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
    let deployment_resource = c
        .catalog
        .resolve("apps/v1/deployments", &c.settings.aliases)
        .expect("canonical deployment");
    let deployment_api = deployment_resource.api(c.client.clone(), Some("sauron-m8b"));
    let deployment = Object::new(
        serde_json::to_value(deployment_api.get("m8b-multi").await.expect("m8b-multi")).unwrap(),
    );
    let containers = deployment
        .value
        .pointer("/spec/template/spec/containers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("m8b-multi has containers");
    assert_eq!(
        containers.len(),
        2,
        "fixture must have exactly two containers"
    );
    let sidecar_image_before = containers
        .iter()
        .find(|c| c["name"] == "sidecar")
        .and_then(|c| c["image"].as_str())
        .expect("sidecar image")
        .to_owned();
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
    let cancel = CancellationToken::new();
    let new_web_image = "registry.k8s.io/pause:3.10";

    let built = workflow::set_image(
        scope,
        deployment_resource,
        &containers,
        Some("web"),
        new_web_image,
        1,
    )
    .expect("set_image is supported for Deployment");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("set_image");
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

    let after = deployment_api
        .get("m8b-multi")
        .await
        .expect("m8b-multi after set_image");
    let after_containers = after
        .data
        .pointer("/spec/template/spec/containers")
        .and_then(serde_json::Value::as_array)
        .expect("containers after set_image");
    let web_after = after_containers
        .iter()
        .find(|c| c["name"] == "web")
        .and_then(|c| c["image"].as_str());
    assert_eq!(web_after, Some(new_web_image));
    let sidecar_after = after_containers
        .iter()
        .find(|c| c["name"] == "sidecar")
        .and_then(|c| c["image"].as_str());
    assert_eq!(
        sidecar_after,
        Some(sidecar_image_before.as_str()),
        "the unrelated sidecar container's image must survive untouched"
    );
}

fn new_journal(label: &str) -> (sauron::mutation::journal::Journal, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "sauron-m8b-live-journal-{label}-{}",
        std::process::id()
    ));
    (
        sauron::mutation::journal::Journal::new(dir.join("journal.jsonl")),
        dir,
    )
}
