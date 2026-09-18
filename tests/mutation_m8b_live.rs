//! Run only through the identity-guarded `scripts/test-cluster.sh m8b-test`.
//! Proves M8B.1 (Cordon/Uncordon) against the real Node of
//! `kind-sauron-test`, M8B.2 (Set Image) against a dedicated two-container
//! Deployment fixture, M8B.3 (CronJob trigger) against a dedicated CronJob
//! whose own schedule never fires during a test run, and M8B.4 (Evict)
//! against a PDB-protected Pod (real 429 denial) and a PDB-free Pod (real
//! success). The cordon test cordons and immediately uncordons the
//! cluster's ONE node, briefly and within one sequential test function --
//! never left cordoned, never held cordoned across multiple test functions
//! (which would race under cargo's default parallel test execution,
//! matching every other `*_live.rs` file's own precedent). Every other
//! test targets a distinct object, so each is a separate test function
//! without racing the cordon test or each other.
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

#[tokio::test]
#[ignore = "explicit isolated kind config and m8b-fixtures (m8b-nightly CronJob) required"]
async fn live_trigger_creates_a_job_traceable_to_the_source_cronjob() {
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
    let cronjob_resource = c
        .catalog
        .resolve("batch/v1/cronjobs", &c.settings.aliases)
        .expect("canonical cronjob");
    let job_resource = c
        .catalog
        .resolve("batch/v1/jobs", &c.settings.aliases)
        .expect("canonical job");
    let cronjob_api = cronjob_resource.api(c.client.clone(), Some("sauron-m8b"));
    let cronjob = Object::new(
        serde_json::to_value(cronjob_api.get("m8b-nightly").await.expect("m8b-nightly")).unwrap(),
    );
    let job_template_spec = cronjob
        .value
        .pointer("/spec/jobTemplate/spec")
        .cloned()
        .expect("m8b-nightly has a jobTemplate.spec");
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: cronjob_resource.id(),
        namespace: cronjob.namespace.clone(),
        name: cronjob.name.clone(),
        uid: cronjob.uid.clone(),
    };
    let cancel = CancellationToken::new();
    let job_name = format!(
        "m8b-nightly-trigger-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    let built = workflow::trigger_cronjob(
        scope,
        cronjob_resource,
        job_resource.clone(),
        &job_template_spec,
        &job_name,
        1,
    )
    .expect("trigger is supported for CronJob");
    let evaluation = policy::evaluate(&verified_policy(), &built.intent);
    assert_eq!(evaluation.decision, PolicyDecision::RequireConfirmation);
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("trigger");
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
    assert!(
        matches!(verification, Verification::Created(_)),
        "expected Created, got {verification:?}"
    );
    std::fs::remove_dir_all(&dir).ok();

    let job_api = job_resource.api(c.client.clone(), Some("sauron-m8b"));
    let created = job_api.get(&job_name).await.expect("created Job");
    assert_eq!(
        created
            .metadata
            .labels
            .as_ref()
            .and_then(|l| l.get("sauron.io/triggered-from"))
            .map(String::as_str),
        Some("m8b-nightly"),
        "the created Job must be traceable back to its source CronJob"
    );
}

#[tokio::test]
#[ignore = "explicit isolated kind config and m8b-fixtures (m8b-evict-blocked/free Pods + PDB) required"]
async fn live_evict_denied_by_pdb_then_succeeds_on_a_pdb_free_pod() {
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
    let pod_resource = c
        .catalog
        .resolve("v1/pods", &c.settings.aliases)
        .expect("canonical pod");
    let pod_api = pod_resource.api(c.client.clone(), Some("sauron-m8b"));
    let cancel = CancellationToken::new();

    // --- Denied by PDB ---
    let blocked = Object::new(
        serde_json::to_value(
            pod_api
                .get("m8b-evict-blocked")
                .await
                .expect("m8b-evict-blocked"),
        )
        .unwrap(),
    );
    let blocked_scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: pod_resource.id(),
        namespace: blocked.namespace.clone(),
        name: blocked.name.clone(),
        uid: blocked.uid.clone(),
    };
    let built = workflow::evict(blocked_scope, pod_resource.clone(), 1)
        .expect("evict is supported for Pod");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Strong,
    };
    let (journal, dir) = new_journal("evict-denied");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload,
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::DisruptionBudgetDenied);
    std::fs::remove_dir_all(&dir).ok();
    let still_present = pod_api.get("m8b-evict-blocked").await;
    assert!(
        still_present.is_ok(),
        "a PDB denial must never fall back to a plain delete"
    );

    // --- Succeeds on a Pod with no PDB ---
    let free = Object::new(
        serde_json::to_value(pod_api.get("m8b-evict-free").await.expect("m8b-evict-free")).unwrap(),
    );
    let free_scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: pod_resource.id(),
        namespace: free.namespace.clone(),
        name: free.name.clone(),
        uid: free.uid.clone(),
    };
    let built = workflow::evict(free_scope, pod_resource, 2).expect("evict is supported for Pod");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Strong,
    };
    let (journal, dir) = new_journal("evict-success");
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
    assert!(
        matches!(
            verification,
            Verification::DeletionInProgress | Verification::ObservedGone | Verification::Pending
        ),
        "expected an eviction-in-progress-or-complete state, got {verification:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
#[ignore = "explicit isolated kind config and m8b-fixtures (m8b-force-delete Pod) required"]
async fn live_force_delete_removes_the_pod_with_grace_period_zero() {
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
    let pod_resource = c
        .catalog
        .resolve("v1/pods", &c.settings.aliases)
        .expect("canonical pod");
    let pod_api = pod_resource.api(c.client.clone(), Some("sauron-m8b"));
    let pod = Object::new(
        serde_json::to_value(
            pod_api
                .get("m8b-force-delete")
                .await
                .expect("m8b-force-delete"),
        )
        .unwrap(),
    );
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: pod_resource.id(),
        namespace: pod.namespace.clone(),
        name: pod.name.clone(),
        uid: pod.uid.clone(),
    };
    let cancel = CancellationToken::new();
    let built =
        workflow::force_delete(scope, pod_resource, 1).expect("force_delete is supported for Pod");
    let evaluation = policy::evaluate(&verified_policy(), &built.intent);
    assert_eq!(
        evaluation.decision,
        PolicyDecision::RequireStrongerConfirmation
    );
    assert!(
        evaluation
            .reasons
            .contains(&sauron::mutation::PolicyReason::ForceSemantics),
        "force delete must carry its own auditable policy reason: {:?}",
        evaluation.reasons
    );
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Strong,
    };
    let (journal, dir) = new_journal("force-delete");
    let outcome = executor::commit(
        &c,
        &verified_policy(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload,
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
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
            "unexpected force-delete verification state: {last:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(
        resolved_gone,
        "force delete never resolved to ObservedGone within the bounded poll window (last: {last:?})"
    );
    std::fs::remove_dir_all(&dir).ok();
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
