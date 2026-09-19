//! M9.1: read-only live evidence against a real Flux install on the
//! dedicated `sauron-m9` kind cluster (never `kind-sauron-test` -- see
//! docs/M9_ACCEPTANCE.md's resolved "dedicated cluster" decision). Run
//! only through `scripts/test-cluster-m9.sh flux-test`, which sets
//! `SAURON_TEST_M9_KUBECONFIG` -- a distinct env var name from M8B's own
//! `SAURON_TEST_KUBECONFIG` so a stray unset variable can never point an
//! M9 live test at the M1-M8B regression cluster instead.
use sauron::{
    graph::{Provenance, references},
    integrations::{self, flux},
    kube::{self, ConnectOptions, relationships::resolve_target},
    resources::Object,
    config::Config,
};

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

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and a real Flux install required"]
async fn live_flux_discovery_reports_every_installed_kind_as_present() {
    let c = connect().await;
    let discovery = integrations::discover(&c.catalog, integrations::Integration::Flux, flux::KINDS);
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
    assert_eq!(target.api_version, "", "Flux never persists a resolved apiVersion in sourceRef");

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
    assert!(report.contains("sourceRef: HelmRepository sauron-m9/podinfo"), "{report}");

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
async fn live_never_reconciled_kustomization_reports_the_sentinel_and_an_unresolvable_missing_source() {
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
    assert!(report.contains("observedGeneration: -1 -- never reconciled"), "{report}");
    assert!(report.contains("suspended: true"), "{report}");
    assert!(report.contains("dependsOn:\n  - podinfo-kustomize"), "{report}");

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
