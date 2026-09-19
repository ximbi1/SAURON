//! M9.1: read-only live evidence against a real Flux install on the
//! dedicated `sauron-m9` kind cluster (never `kind-sauron-test` -- see
//! docs/M9_ACCEPTANCE.md's resolved "dedicated cluster" decision). Run
//! only through `scripts/test-cluster-m9.sh flux-test`, which sets
//! `SAURON_TEST_M9_KUBECONFIG` -- a distinct env var name from M8B's own
//! `SAURON_TEST_KUBECONFIG` so a stray unset variable can never point an
//! M9 live test at the M1-M8B regression cluster instead.
use sauron::{
    app::session::Scope,
    config::Config,
    graph::{Provenance, references},
    integrations::{self, flux},
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
    let dir = std::env::temp_dir().join(format!("sauron-m9-live-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("journal dir");
    (
        sauron::mutation::journal::Journal::new(dir.join("mutations.jsonl")),
        dir,
    )
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and a real Flux install required"]
async fn live_flux_discovery_reports_every_installed_kind_as_present() {
    let c = connect().await;
    let discovery =
        integrations::discover(&c.catalog, integrations::Integration::Flux, flux::KINDS);
    assert_eq!(
        discovery.state(),
        None,
        "the full flux2 v2.9.5 install manifest installs every kind this app expects: {:?}",
        discovery.found
    );
    for label in [
        "Kustomization",
        "HelmRelease",
        "GitRepository",
        "OCIRepository",
        "HelmRepository",
        "Bucket",
    ] {
        assert!(
            discovery.resource(label).is_some(),
            "{label} must be discovered as present on a real Flux install"
        );
    }
}

async fn fetch(c: &kube::Connection, group: &str, kind: &str, plural: &str, name: &str) -> Object {
    let resource = c
        .catalog
        .group_kind(group, kind)
        .cloned()
        .unwrap_or_else(|| panic!("{kind} not discovered"));
    assert_eq!(resource.api.plural, plural);
    let api = resource.api(c.client.clone(), Some("sauron-m9"));
    let object = api.get(name).await.expect("get");
    Object::new(serde_json::to_value(object).unwrap())
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and m9-flux.yaml fixtures required"]
async fn live_ready_kustomization_shows_real_conditions_and_a_resolvable_source_reference() {
    let c = connect().await;
    let kustomization = fetch(
        &c,
        "kustomize.toolkit.fluxcd.io",
        "Kustomization",
        "kustomizations",
        "podinfo-kustomize",
    )
    .await;
    let report = flux::status_report("Kustomization", &kustomization.name, &kustomization.value);
    assert!(report.contains("observedGeneration:"));
    assert!(report.contains("Ready = True"), "{report}");
    assert!(report.contains("lastAppliedRevision:"), "{report}");
    assert!(report.contains("sourceRef: GitRepository"), "{report}");
    assert!(!report.contains("STALE"), "{report}");

    let refs = references::extract(&kustomization);
    assert!(!refs.malformed);
    let (target, _) = refs
        .targets
        .iter()
        .find(|(t, _)| t.kind == "GitRepository")
        .expect("sourceRef target extracted");
    assert_eq!(target.name, "podinfo");
    assert_eq!(target.namespace, "sauron-m9");
    assert_eq!(target.provenance, Provenance::StatusReference);
    assert_eq!(
        target.api_version, "",
        "Flux never persists a resolved apiVersion in sourceRef"
    );

    // Prove the empty-api_version/StatusReference path genuinely resolves
    // against a REAL discovery catalog, not just a hand-built fake one.
    let resolved = resolve_target(&c.catalog, target).expect("GitRepository resolves live");
    assert_eq!(resolved.api.kind, "GitRepository");
    assert_eq!(resolved.api.group, "source.toolkit.fluxcd.io");
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and m9-flux.yaml fixtures required"]
async fn live_helm_release_shows_ready_and_released_as_two_distinct_conditions() {
    let c = connect().await;
    let release = fetch(
        &c,
        "helm.toolkit.fluxcd.io",
        "HelmRelease",
        "helmreleases",
        "podinfo-helm",
    )
    .await;
    let report = flux::status_report("HelmRelease", &release.name, &release.value);
    assert!(report.contains("Ready = True"), "{report}");
    assert!(report.contains("Released = True"), "{report}");
    assert!(
        report.contains("sourceRef: HelmRepository sauron-m9/podinfo"),
        "{report}"
    );

    let refs = references::extract(&release);
    let (target, _) = refs
        .targets
        .iter()
        .find(|(t, _)| t.kind == "HelmRepository")
        .expect("chart sourceRef target extracted");
    let resolved = resolve_target(&c.catalog, target).expect("HelmRepository resolves live");
    assert_eq!(resolved.api.kind, "HelmRepository");
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and m9-flux.yaml fixtures required"]
async fn live_never_reconciled_kustomization_reports_the_sentinel_and_an_unresolvable_missing_source()
 {
    let c = connect().await;
    let dependent = fetch(
        &c,
        "kustomize.toolkit.fluxcd.io",
        "Kustomization",
        "kustomizations",
        "podinfo-dependent",
    )
    .await;
    let report = flux::status_report("Kustomization", &dependent.name, &dependent.value);
    assert!(
        report.contains("observedGeneration: -1 -- never reconciled"),
        "{report}"
    );
    assert!(report.contains("suspended: true"), "{report}");
    assert!(
        report.contains("dependsOn:\n  - podinfo-kustomize"),
        "{report}"
    );

    let refs = references::extract(&dependent);
    assert!(!refs.malformed);
    let (missing_source, _) = refs
        .targets
        .iter()
        .find(|(t, _)| t.kind == "GitRepository")
        .expect("sourceRef to the missing GitRepository is still extracted");
    assert_eq!(missing_source.name, "podinfo-missing");
    // The source genuinely does not exist on this cluster -- resolving
    // the KIND succeeds (GitRepository is installed), but fetching the
    // named object must fail, never fabricate a placeholder.
    let resource = resolve_target(&c.catalog, missing_source).expect("kind resolves");
    let api = resource.api(c.client.clone(), Some("sauron-m9"));
    assert!(
        api.get("podinfo-missing").await.is_err(),
        "the referenced source must genuinely not exist"
    );

    let (dep_target, _) = refs
        .targets
        .iter()
        .find(|(t, _)| t.kind == "Kustomization")
        .expect("dependsOn target extracted");
    assert_eq!(dep_target.name, "podinfo-kustomize");
    assert_eq!(dep_target.provenance, Provenance::ExplicitReference);
}

/// M9.2: exercises the full guarded-action gateway (policy -> confirm ->
/// commit -> verify) against a real Kustomization, then leaves it exactly
/// as found (suspend=false) -- self-restoring, matching every other
/// live test's own no-drift discipline. Proves the "reconcile requested"
/// != "reconciliation completed" distinction: verification only ever
/// confirms the annotation itself changed; the Ready condition read
/// immediately after (via the same `flux::status_report` M9.1 already
/// built) is a separate, later fact, never folded into `Verification`.
#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and m9-flux.yaml fixtures required; suspends/resumes and reconciles a real Kustomization briefly"]
async fn live_flux_suspend_resume_and_reconcile_round_trip_on_a_real_kustomization() {
    let c = connect_mutating().await;
    let resource = c
        .catalog
        .group_kind("kustomize.toolkit.fluxcd.io", "Kustomization")
        .cloned()
        .expect("Kustomization discovered");
    let api = resource.api(c.client.clone(), Some("sauron-m9"));
    let before = Object::new(
        serde_json::to_value(api.get("podinfo-kustomize").await.expect("get")).unwrap(),
    );
    assert_ne!(
        before.value.pointer("/spec/suspend"),
        Some(&serde_json::Value::Bool(true)),
        "must start unsuspended (absent, matching the pristine fixture, or an explicit \
         false left over from a prior run of this same self-restoring test)"
    );
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

    // --- Suspend ---
    let built = workflow::flux_suspend(scope.clone(), resource.clone(), 1)
        .expect("flux_suspend supported for Kustomization");
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
    let (journal, dir) = new_journal("suspend");
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
    let suspended = Object::new(
        serde_json::to_value(api.get("podinfo-kustomize").await.expect("get")).unwrap(),
    );
    assert_eq!(
        suspended.value.pointer("/spec/suspend"),
        Some(&serde_json::Value::Bool(true))
    );

    // --- Resume ---
    let built = workflow::flux_resume(scope.clone(), resource.clone(), 2)
        .expect("flux_resume supported for Kustomization");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("resume");
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
    let resumed = Object::new(
        serde_json::to_value(api.get("podinfo-kustomize").await.expect("get")).unwrap(),
    );
    assert_ne!(
        resumed.value.pointer("/spec/suspend"),
        Some(&serde_json::Value::Bool(true)),
        "must be left unsuspended -- this test must never leave the fixture suspended"
    );

    // --- Reconcile ---
    let timestamp = chrono::Utc::now().to_rfc3339();
    let built = workflow::flux_reconcile(scope, resource, &timestamp, 3)
        .expect("flux_reconcile supported for Kustomization");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let (journal, dir) = new_journal("reconcile");
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
        "verification only confirms the annotation itself changed"
    );
    std::fs::remove_dir_all(&dir).ok();

    // Third, separate fact: a fresh read of Flux's OWN Ready condition,
    // never claimed by the commit/verification facts above.
    let reconciled = Object::new(
        serde_json::to_value(api.get("podinfo-kustomize").await.expect("get")).unwrap(),
    );
    assert_eq!(
        reconciled
            .value
            .pointer("/metadata/annotations/reconcile.fluxcd.io~1requestedAt"),
        Some(&serde_json::Value::String(timestamp)),
    );
    // The controller's own later convergence is a separate, possibly-
    // still-in-progress fact, never awaited or polled for here -- a real
    // run of this exact test caught the controller mid-reconciliation
    // (`Reconciling=True`, `Ready=Unknown`, and a real `STALE` flag from
    // observedGeneration genuinely lagging generation), which is the
    // correct, honest thing to observe and show, not a bug to work
    // around by waiting until it looks done.
    let report = flux::status_report("Kustomization", &reconciled.name, &reconciled.value);
    assert!(report.contains("CONDITIONS"), "{report}");
    assert!(
        report.contains("Ready = True") || report.contains("Ready = Unknown"),
        "Ready must be one of Flux's own real reported values, never fabricated: {report}"
    );
}
