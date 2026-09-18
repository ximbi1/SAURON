//! Run only through the identity-guarded `scripts/test-cluster.sh m6-test`.
use sauron::{
    config::Config,
    graph::references::extract,
    kube::{self, ConnectOptions, relationships},
    resources::Object,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
#[ignore = "explicit isolated kind config and m6-fixtures required"]
async fn ownership_and_template_references_resolve_live() {
    let path = std::env::var_os("SAURON_TEST_KUBECONFIG").expect("explicit test kubeconfig");
    let c = kube::connect(
        ConnectOptions {
            kubeconfig: Some(path.into()),
            context: Some("kind-sauron-test".into()),
            force_readonly: true,
        },
        Config::default(),
    )
    .await
    .expect("connect");
    let deployment_resource = c
        .catalog
        .resolve("apps/v1/deployments", &c.settings.aliases)
        .expect("canonical deployment");
    let deployment = deployment_resource
        .api(c.client.clone(), Some("sauron-m6"))
        .get("m6-web")
        .await
        .expect("fixture");
    let deployment = Object::new(serde_json::to_value(deployment).unwrap());
    let cancel = CancellationToken::new();
    let references = extract(&deployment);
    assert!(!references.partial && !references.malformed);
    assert_eq!(references.targets.len(), 3);
    for (reference, paths) in references.targets {
        assert!(paths.iter().all(|p| p.starts_with("/spec/template/spec/")));
        let (_, id, object) = relationships::fetch_target(&c, 1, &deployment, &reference, &cancel)
            .await
            .expect("resolved reference");
        assert_eq!(id.namespace, "sauron-m6");
        if reference.kind == "Secret" {
            assert!(object.value.get("data").is_none());
            assert!(object.value.get("stringData").is_none());
        }
    }
    let rs_resource = c
        .catalog
        .resolve("apps/v1/replicasets", &c.settings.aliases)
        .unwrap();
    let (replicasets, partial, _) = relationships::children(&c, &deployment, &rs_resource, &cancel)
        .await
        .expect("replicasets");
    assert!(!partial);
    assert!(!replicasets.is_empty());
    let pod_resource = c.catalog.resolve("v1/pods", &c.settings.aliases).unwrap();
    let mut count = 0;
    for rs in replicasets {
        for (owner, _) in extract(&rs).targets {
            let (_, id, _) = relationships::fetch_target(&c, 1, &rs, &owner, &cancel)
                .await
                .expect("owner UID");
            assert_eq!(id.uid, deployment.uid);
        }
        let (pods, partial, _) = relationships::children(&c, &rs, &pod_resource, &cancel)
            .await
            .expect("pods");
        assert!(!partial);
        count += pods.len();
        for pod in pods {
            for (owner, _) in extract(&pod).targets {
                let (_, id, _) = relationships::fetch_target(&c, 1, &pod, &owner, &cancel)
                    .await
                    .expect("Pod owner UID");
                assert_eq!(id.uid, rs.uid);
            }
        }
    }
    assert!(count > 0);
}
