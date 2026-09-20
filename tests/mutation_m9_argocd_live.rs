//! M9.3: read-only live evidence against a real Argo CD v3.5.3 install on
//! the dedicated `sauron-m9` kind cluster (installed alongside Flux --
//! see docs/M9_ACCEPTANCE.md's resolved Open question 1a). Run only
//! through `scripts/test-cluster-m9.sh argocd-test`, which sets
//! `SAURON_TEST_M9_KUBECONFIG` -- the same dedicated M9 kubeconfig
//! `mutation_m9_live.rs` (Flux) already uses.
use sauron::{
    app::session::Scope,
    config::Config,
    graph::{Provenance, references},
    integrations::{self, argocd},
    kube::{self, ConnectOptions, mutation as executor, relationships::resolve_target},
    mutation::{
        Confirmation, ConfirmationRequirement, MutationOutcome, Verification, policy, workflow,
    },
    resources::Object,
};
use tokio_util::sync::CancellationToken;

async fn connect() -> kube::Connection {
    let path = std::env::var_os("SAURON_TEST_M9_KUBECONFIG").expect("explicit M9 test kubeconfig");
    kube::connect(
        ConnectOptions {
            kubeconfig: Some(path.into()),
            context: Some("kind-sauron-m9".into()),
            force_readonly: true,
            mutation_test_cluster_verified: false,
        },
        Config::default(),
    )
    .await
    .expect("connect to sauron-m9")
}

async fn connect_mutating() -> kube::Connection {
    let path = std::env::var_os("SAURON_TEST_M9_KUBECONFIG").expect("explicit M9 test kubeconfig");
    kube::connect(
        ConnectOptions {
            kubeconfig: Some(path.into()),
            context: Some("kind-sauron-m9".into()),
            force_readonly: false,
            mutation_test_cluster_verified: false,
        },
        Config::default(),
    )
    .await
    .expect("connect to sauron-m9")
}

fn verified_policy() -> policy::PolicyContext {
    policy::PolicyContext {
        readonly: false,
        readonly_forced: false,
        cluster_verified_for_mutation: true,
        ..policy::PolicyContext::default()
    }
}

fn new_journal(label: &str) -> (sauron::mutation::journal::Journal, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "sauron-m9-argocd-live-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("journal dir");
    (
        sauron::mutation::journal::Journal::new(dir.join("mutations.jsonl")),
        dir,
    )
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and a real Argo CD install required"]
async fn live_argocd_discovery_reports_every_installed_kind_as_present() {
    let c = connect().await;
    let discovery =
        integrations::discover(&c.catalog, integrations::Integration::ArgoCd, argocd::KINDS);
    assert_eq!(
        discovery.state(),
        None,
        "the full argo-cd v3.5.3 install manifest installs every kind this app expects: {:?}",
        discovery.found
    );
    for label in ["Application", "ApplicationSet", "AppProject"] {
        assert!(
            discovery.resource(label).is_some(),
            "{label} must be discovered as present on a real Argo CD install"
        );
    }
}

async fn fetch(c: &kube::Connection, name: &str) -> Object {
    let resource = c
        .catalog
        .group_kind("argoproj.io", "Application")
        .cloned()
        .expect("Application discovered");
    let api = resource.api(c.client.clone(), Some("argocd"));
    let object = api.get(name).await.expect("get");
    Object::new(serde_json::to_value(object).unwrap())
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and m9-argocd.yaml fixtures required"]
async fn live_synced_application_shows_real_status_and_resolvable_managed_resources() {
    let c = connect().await;
    let app = fetch(&c, "guestbook").await;
    let report = argocd::status_report("Application", &app.name, &app.value);
    assert!(report.contains("project: default"), "{report}");
    assert!(
        report.contains("path=guestbook targetRevision=master"),
        "{report}"
    );
    assert!(
        report.contains("DESTINATION: https://kubernetes.default.svc / sauron-m9"),
        "{report}"
    );
    assert!(report.contains("SYNC STATUS: Synced"), "{report}");
    assert!(report.contains("HEALTH STATUS:"), "{report}");

    let refs = references::extract(&app);
    assert!(!refs.malformed);
    let project = refs
        .targets
        .keys()
        .find(|t| t.kind == "AppProject")
        .expect("AppProject reference extracted");
    assert_eq!(project.name, "default");
    assert_eq!(project.provenance, Provenance::StatusReference);

    let service = refs
        .targets
        .keys()
        .find(|t| t.kind == "Service")
        .expect("managed Service extracted from status.resources");
    assert_eq!(service.api_version, "v1");
    assert_eq!(service.namespace, "sauron-m9");
    // Prove the real-group/version-from-status.resources path genuinely
    // resolves against a REAL discovery catalog.
    let resolved = resolve_target(&c.catalog, service).expect("Service resolves live");
    assert_eq!(resolved.api.kind, "Service");

    let deployment = refs
        .targets
        .keys()
        .find(|t| t.kind == "Deployment")
        .expect("managed Deployment extracted from status.resources");
    assert_eq!(deployment.api_version, "apps/v1");
    let resolved = resolve_target(&c.catalog, deployment).expect("Deployment resolves live");
    assert_eq!(resolved.api.group, "apps");
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and m9-argocd.yaml fixtures required"]
async fn live_broken_application_shows_unknown_sync_and_a_real_comparison_error() {
    let c = connect().await;
    let app = fetch(&c, "guestbook-broken").await;
    let report = argocd::status_report("Application", &app.name, &app.value);
    assert!(report.contains("SYNC STATUS: Unknown"), "{report}");
    assert!(
        report.contains("ComparisonError:"),
        "a real, live-reported condition, never fabricated: {report}"
    );
    assert!(
        report.contains("unable to resolve"),
        "the exact controller-reported message must be shown verbatim: {report}"
    );
}

/// M9.4: exercises the full guarded-action gateway against a real
/// Application. Rollback targets the same revision `guestbook` is
/// already synced to (a mechanism proof, matching M8B.6 Force delete's
/// own precedent -- a genuinely different-revision rollback scenario
/// would need a second real Git revision to be meaningful, which is out
/// of proportion to fabricate here) -- what this test proves is that
/// `argocd_rollback`'s exact CRD-level mechanism (an explicit `revision`
/// echoed back in `status.operationState`) works against a real
/// controller, not that the app picked a materially different target.
#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and m9-argocd.yaml fixtures required; syncs and rolls back a real Application"]
async fn live_argocd_sync_refresh_and_rollback_round_trip_on_a_real_application() {
    let c = connect_mutating().await;
    let resource = c
        .catalog
        .group_kind("argoproj.io", "Application")
        .cloned()
        .expect("Application discovered");
    let api = resource.api(c.client.clone(), Some("argocd"));
    let before =
        Object::new(serde_json::to_value(api.get("guestbook").await.expect("get")).unwrap());
    let known_revision = before
        .value
        .pointer("/status/history/0/revision")
        .and_then(serde_json::Value::as_str)
        .expect("guestbook already has a recorded sync in status.history")
        .to_string();
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: c.context.clone(),
        cluster: c.cluster.clone(),
        resource: resource.id(),
        namespace: before.namespace.clone(),
        name: before.name.clone(),
        uid: before.uid.clone(),
    };
    let cancel = CancellationToken::new();

    // --- Sync ---
    let built = workflow::argocd_sync(scope.clone(), resource.clone(), 1)
        .expect("argocd_sync supported for Application");
    let evaluation = policy::evaluate(&verified_policy(), &built.intent);
    assert_eq!(
        evaluation.decision,
        sauron::mutation::PolicyDecision::RequireConfirmation
    );
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("sync");
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
        matches!(verification, Verification::Verified | Verification::Pending),
        "sync verification only confirms operationState was recorded, never final success: {verification:?}"
    );
    std::fs::remove_dir_all(&dir).ok();

    // --- Refresh ---
    let built = workflow::argocd_refresh(scope.clone(), resource.clone(), false, 2)
        .expect("argocd_refresh supported for Application");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("refresh");
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
        "the annotation itself must be committed and observed -- Argo CD's own consumption/clearing of it is a separate, later fact"
    );
    std::fs::remove_dir_all(&dir).ok();

    // --- Rollback ---
    let built = workflow::argocd_rollback(scope, resource.clone(), &known_revision, 3)
        .expect("argocd_rollback supported for Application");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("rollback");
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
        matches!(verification, Verification::Verified | Verification::Pending),
        "rollback verification only confirms the exact requested revision was echoed back, never final success: {verification:?}"
    );
    std::fs::remove_dir_all(&dir).ok();

    // Third, separate fact: a fresh read of Argo CD's OWN sync/health,
    // never claimed by the commit/verification facts above.
    let after =
        Object::new(serde_json::to_value(api.get("guestbook").await.expect("get")).unwrap());
    let report = argocd::status_report("Application", &after.name, &after.value);
    assert!(report.contains("OPERATION STATE:"), "{report}");
}
