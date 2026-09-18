//! Opt-in live tests. Only an explicit dedicated test kubeconfig is accepted.
use sauron::{
    command::Action,
    config::Config,
    kube::{self, ConnectOptions},
    resources::Object,
};

#[tokio::test]
#[ignore = "requires SAURON_TEST_KUBECONFIG for dedicated kind-sauron-test"]
async fn discovery_documents_explain_and_secrets() {
    let path = std::env::var_os("SAURON_TEST_KUBECONFIG").expect("explicit test config");
    let connection = kube::connect(
        ConnectOptions {
            kubeconfig: Some(path.into()),
            context: Some("kind-sauron-test".into()),
            force_readonly: false,
            mutation_test_cluster_verified: false,
        },
        Config::default(),
    )
    .await
    .expect("test cluster connection");
    let resource = connection
        .catalog
        .resolve("pods", &connection.settings.aliases)
        .expect("pods discovered");
    let api = resource.api(connection.client.clone(), Some("sauron-fixtures"));
    let pod = api.get("unschedulable").await.expect("fixture pod");
    let object = Object::new(serde_json::to_value(pod).expect("serialize"));
    let report = kube::evidence::document(
        &connection,
        &resource,
        &object,
        Action::Explain,
        false,
        None,
    )
    .await
    .expect("report");
    assert!(
        report.contains("Unschedulable") || report.contains("PodScheduled=False"),
        "{report}"
    );
    let secret = connection
        .catalog
        .resolve("secrets", &connection.settings.aliases)
        .expect("secret kind");
    let secret_object = secret
        .api(connection.client.clone(), Some("sauron-fixtures"))
        .get("redaction-sentinel")
        .await
        .expect("secret fixture");
    let secret_object = Object::new(serde_json::to_value(secret_object).expect("serialize"));
    let yaml = kube::evidence::document(
        &connection,
        &secret,
        &secret_object,
        Action::Yaml,
        false,
        None,
    )
    .await
    .expect("yaml");
    assert!(!yaml.contains("SAURON_TEST_SENTINEL_NEVER_DISPLAY"));
    assert!(yaml.contains("<redacted>"));
    let custom = connection
        .catalog
        .resolve("ey", &connection.settings.aliases)
        .expect("discovered custom shortname");
    assert_eq!(custom.api.group, "testing.sauron.local");
}
