//! M9.3: read-only live evidence against a real Argo CD v3.5.3 install on
//! the dedicated `sauron-m9` kind cluster (installed alongside Flux --
//! see docs/M9_ACCEPTANCE.md's resolved Open question 1a). Run only
//! through `scripts/test-cluster-m9.sh argocd-test`, which sets
//! `SAURON_TEST_M9_KUBECONFIG` -- the same dedicated M9 kubeconfig
//! `mutation_m9_live.rs` (Flux) already uses.
use sauron::{
    config::Config,
    graph::{Provenance, references},
    integrations::{self, argocd},
    kube::{self, ConnectOptions, relationships::resolve_target},
    resources::Object,
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
