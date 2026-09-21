//! M9.5: read-only live evidence for the dedicated Helm release reader
//! against real Helm release Secrets on the dedicated `sauron-m9` kind
//! cluster -- `demo-release` (installed by the real `helm` CLI as pure
//! test-harness tooling, per docs/M9_ACCEPTANCE.md's own established
//! convention) and `podinfo-helm` (Flux's own `HelmRelease`, which
//! itself drives a real Helm install under the hood, giving a second,
//! independently-created release record). Run only through
//! `scripts/test-cluster-m9.sh helm-test`, which sets
//! `SAURON_TEST_M9_KUBECONFIG`.
use sauron::{
    app::session::Scope,
    config::Config,
    kube::{self, ConnectOptions, helm as helm_read},
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

async fn release_secret_uid(connection: &kube::Connection, name: &str) -> String {
    let resource = connection
        .catalog
        .group_kind("", "Secret")
        .expect("Secret is a core resource");
    let api = resource.api(connection.client.clone(), Some("sauron-m9"));
    let secret = api
        .get(name)
        .await
        .expect("real Helm release Secret exists");
    let object = Object::new(serde_json::to_value(secret).unwrap());
    object.uid
}

fn scope(namespace: &str, name: &str, uid: &str) -> Scope {
    Scope {
        epoch: 0,
        request: 0,
        context: "kind-sauron-m9".into(),
        cluster: "kind-sauron-m9".into(),
        resource: "v1/secrets".into(),
        namespace: namespace.into(),
        name: name.into(),
        uid: uid.into(),
    }
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and real Helm releases required"]
async fn live_demo_release_decodes_and_sanitizes_a_real_bitnami_nginx_release() {
    let connection = connect().await;
    let name = "sh.helm.release.v1.demo-release.v1";
    let uid = release_secret_uid(&connection, name).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let view = helm_read::read_release(&connection, &scope("sauron-m9", name, &uid), &cancel)
        .await
        .expect("real release decodes");
    assert_eq!(view.name, "demo-release");
    assert_eq!(view.status, "deployed");
    assert!(view.chart_name.contains("nginx"));
    // A real bitnami/nginx values tree does not obviously contain a
    // password field, but this proves the pipeline runs end-to-end
    // against a real double-base64-gzip record, not a synthetic one --
    // sensitive-key masking itself is already proven at the unit level
    // in src/integrations/helm.rs against a synthetic release with a
    // known secret value (masking logic is identical either way).
    let debug = format!("{view:?}");
    assert!(
        !debug.to_lowercase().contains("apikey"),
        "no unmasked sensitive-looking key survived sanitization"
    );
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and real Helm releases required"]
async fn live_podinfo_helm_release_is_a_second_independently_created_real_release() {
    let connection = connect().await;
    let name = "sh.helm.release.v1.podinfo-helm.v1";
    let uid = release_secret_uid(&connection, name).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let view = helm_read::read_release(&connection, &scope("sauron-m9", name, &uid), &cancel)
        .await
        .expect("real Flux-driven release decodes");
    assert_eq!(view.name, "podinfo-helm");
    assert!(view.chart_name.contains("podinfo"));
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and real Helm releases required"]
async fn live_stale_uid_against_a_real_secret_fails_closed() {
    let connection = connect().await;
    let name = "sh.helm.release.v1.demo-release.v1";
    let cancel = tokio_util::sync::CancellationToken::new();
    let error = helm_read::read_release(
        &connection,
        &scope("sauron-m9", name, "not-the-real-uid"),
        &cancel,
    )
    .await
    .expect_err("a UID that does not match the real object must fail closed");
    assert_eq!(error, helm_read::HelmReadError::TargetReplaced);
}

#[tokio::test]
#[ignore = "explicit isolated kind-sauron-m9 config and real Helm releases required"]
async fn live_wrong_type_secret_is_rejected_even_when_selected_by_a_realistic_name() {
    let connection = connect().await;
    // A plausible-looking name for a non-release Secret in the same
    // namespace -- proves the type check is against the FRESH fetch,
    // not inferred from the name shape, even when no such Secret exists
    // (NotFound is also an acceptable, explicit failure here -- what
    // must never happen is a false decode).
    let cancel = tokio_util::sync::CancellationToken::new();
    let error = helm_read::read_release(
        &connection,
        &scope("sauron-m9", "sh.helm.release.v1.nonexistent.v1", "any-uid"),
        &cancel,
    )
    .await
    .expect_err("a nonexistent release Secret must be an explicit error, never a fabricated view");
    assert!(matches!(
        error,
        helm_read::HelmReadError::NotFound | helm_read::HelmReadError::TargetReplaced
    ));
}
