//! A real HTTP endpoint with a scripted Kubernetes protocol, independent of kubeconfig.
use kube::{Client, Config, core::ApiResource};
use sauron::{
    app::event::Payload,
    command::Action,
    config::Settings,
    kube::{
        Connection,
        discovery::{Catalog, Resource},
        evidence, printer, watch,
        watch::Query,
    },
    resources::{Object, store::Store},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

struct Server {
    url: String,
    task: JoinHandle<()>,
}

#[tokio::test]
async fn metrics_success_pod_node_and_independent_malformed_fields() {
    use sauron::{evidence::Unknown, kube::metrics};
    let server = Server::new(|path| {
        assert!(path.starts_with("/apis/metrics.k8s.io/v1beta1/"));
        assert!(path.contains("limit=5000"));
        if path.contains("/nodes") {
            (200, json!({"items":[{"metadata":{"name":"node"},"timestamp":chrono::Utc::now().to_rfc3339(),"window":"15s","usage":{"cpu":"123456789n","memory":"1Gi"}}]}).to_string())
        } else {
            assert!(path.contains("/namespaces/default/pods"));
            (200, json!({"metadata":{"continue":"more"},"items":[{"metadata":{"name":"p","namespace":"default"},"timestamp":chrono::Utc::now().to_rfc3339(),"window":"15s","containers":[{"name":"c","usage":{"cpu":"250m","memory":"bad"}}]}]}).to_string())
        }
    }).await;
    let c = connection(server.client());
    let batch = metrics::fetch(&c, &resource(), Some("default"))
        .await
        .expect("pods");
    assert!(batch.coverage.partial());
    let sample = batch.samples["default/p"].value.as_ref().expect("sample");
    assert_eq!(sample.containers["c"].cpu, Ok(0.25));
    assert_eq!(sample.containers["c"].memory, Err(Unknown::Malformed));
    let mut node = resource();
    node.api.plural = "nodes".into();
    node.api.kind = "Node".into();
    node.namespaced = false;
    let batch = metrics::fetch(&c, &node, None).await.expect("nodes");
    let node = batch.samples["/node"]
        .value
        .as_ref()
        .expect("sample")
        .node
        .as_ref()
        .expect("usage");
    assert!((node.cpu.expect("cpu") - 0.123456789).abs() < 1e-12);
    assert_eq!(node.memory, Ok(1073741824.0));
}

#[tokio::test]
async fn metrics_errors_are_distinct_and_response_bounded() {
    use sauron::{evidence::Unknown, kube::metrics};
    for (code, body, reason) in [
        (404, "credential sentinel".into(), Unknown::Unavailable),
        (403, "credential sentinel".into(), Unknown::Forbidden),
        (500, "credential sentinel".into(), Unknown::TransportError),
        (200, "not json".into(), Unknown::Malformed),
        (200, "x".repeat(metrics::MAX_BYTES + 1), Unknown::Partial),
    ] {
        let server = Server::new(move |_| (code, body.clone())).await;
        let result = metrics::fetch(&connection(server.client()), &resource(), None).await;
        assert_eq!(result.expect_err("unknown"), reason);
    }
}

#[tokio::test]
async fn metrics_body_timeout_and_cancel_are_bounded() {
    use sauron::{evidence::Unknown, kube::metrics};
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut request = [0; 4096];
        let _ = socket.read(&mut request).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n")
            .await
            .expect("headers");
        std::future::pending::<()>().await;
    });
    let server = Server { url, task };
    let mut c = connection(server.client());
    c.settings.request_timeout_secs = 1;
    let r = resource();
    assert_eq!(
        metrics::fetch(&c, &r, None).await.expect_err("timeout"),
        Unknown::TimedOut
    );
    let token = CancellationToken::new();
    token.cancel();
    tokio::select! { biased; _ = token.cancelled() => {}, _ = metrics::fetch(&c, &r, None) => panic!("cancel must preempt connect") }
}

#[tokio::test]
async fn owned_logs_clip_sanitize_and_reject_replaced_uid() {
    use sauron::app::session::{Kind, Outcome, Scope, Sessions, State};
    let pod = json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"log-test","namespace":"default","uid":"current"},"spec":{"containers":[{"name":"worker"}]}});
    let response = pod.to_string();
    let server = Server::new(move |path| {
        if path.contains("/log?") {
            assert!(path.contains("container=worker"));
            assert!(path.contains("previous=true"));
            assert!(path.contains("timestamps=true"));
            (
                200,
                format!("\u{1b}[31munsafe\u{7}\n{}\nlast", "x".repeat(20_000)),
            )
        } else {
            (200, response.clone())
        }
    })
    .await;
    let mut sessions = Sessions::default();
    let scope = Scope {
        epoch: 7,
        request: 8,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/pods".into(),
        namespace: "default".into(),
        name: "log-test".into(),
        uid: "current".into(),
    };
    let (tx, mut rx) = mpsc::channel(16);
    let client = connection(server.client());
    let object = Arc::new(Object::new(pod.clone()));
    let id = sessions
        .spawn(Kind::Logs, scope.clone(), CancellationToken::new(), |id| {
            evidence::logs(
                client,
                object,
                evidence::LogOptions {
                    container: Some("worker".into()),
                    previous: true,
                },
                7,
                8,
                tx,
                id,
            )
        })
        .expect("spawn");
    let record = tokio::time::timeout(Duration::from_secs(2), sessions.join_next())
        .await
        .expect("deadline")
        .expect("end");
    assert_eq!(record.state, State::Ended(Outcome::Completed));
    assert!(
        matches!(rx.recv().await.expect("started").payload, Payload::LogStarted { session, .. } if session == id)
    );
    let mut lines = vec![];
    while let Ok(event) = rx.try_recv() {
        assert_eq!(event.epoch, 7);
        if let Payload::LogLine {
            request,
            session,
            line,
        } = event.payload
        {
            assert_eq!(request, 8);
            assert_eq!(session, id);
            assert!(!line.contains('\u{1b}') && !line.contains('\u{7}'));
            lines.push(line);
        }
    }
    assert_eq!(lines.len(), 3);
    assert!(lines[1].ends_with("[line truncated at 16 KiB]"));
    assert!(lines[1].len() < 16_450);
    assert!(lines[2].ends_with("] last"));
    let mut replaced = pod;
    replaced["metadata"]["uid"] = "old".into();
    let (tx, mut rx) = mpsc::channel(16);
    let client = connection(server.client());
    sessions
        .spawn(Kind::Logs, scope, CancellationToken::new(), |id| {
            evidence::logs(
                client,
                Arc::new(Object::new(replaced)),
                evidence::LogOptions {
                    container: None,
                    previous: false,
                },
                7,
                9,
                tx,
                id,
            )
        })
        .expect("spawn");
    let record = sessions.join_next().await.expect("replaced end");
    assert!(
        matches!(record.state, State::Ended(Outcome::Failed(ref message)) if message.contains("replaced"))
    );
    assert!(matches!(
        rx.try_recv().expect("explicit source failure").payload,
        Payload::LogSourceError { .. }
    ));
    assert!(rx.try_recv().is_err());
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn new(handler: impl Fn(&str) -> (u16, String) + Send + Sync + 'static) -> Self {
        Self::with_request(move |request| handler(request.split_whitespace().nth(1).unwrap_or("/")))
            .await
    }
    async fn with_request(handler: impl Fn(&str) -> (u16, String) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let handler = Arc::new(handler);
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let mut data = Vec::new();
                let mut buffer = [0; 1024];
                while !data.windows(4).any(|w| w == b"\r\n\r\n") && data.len() < 16_384 {
                    let n = socket.read(&mut buffer).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buffer[..n]);
                }
                // Requests with a body (PATCH/DELETE preconditions) carry a
                // Content-Length header; read exactly that many more bytes so
                // handlers can inspect the body too, not just the headers.
                let header_end = data
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|i| i + 4);
                if let Some(header_end) = header_end {
                    let headers = String::from_utf8_lossy(&data[..header_end]);
                    let content_length = headers
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                        .and_then(|l| l.split(':').nth(1))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    while data.len() < header_end + content_length && data.len() < 16_384 {
                        let n = socket.read(&mut buffer).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        data.extend_from_slice(&buffer[..n]);
                    }
                }
                let request = String::from_utf8_lossy(&data);
                let (code, body) = handler(&request);
                let response = format!(
                    "HTTP/1.1 {code} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        Self { url, task }
    }
    fn client(&self) -> Client {
        // Match src/kube/mod.rs's own fix: kube-rs's transport-level retry
        // (on by default) would otherwise silently retry 429/503/504
        // responses below every test in this file, hiding exactly the kind
        // of bug `mutation_evict_pdb_denial_is_distinct_and_never_falls_
        // back_to_delete` exists to catch.
        let mut config = Config::new(self.url.parse().expect("URI"));
        config.default_retry = false;
        Client::try_from(config).expect("client")
    }
}

#[tokio::test]
async fn graph_secret_resolution_requests_metadata_only_without_fallback() {
    use sauron::{
        evidence::Unknown,
        graph::{Provenance, references::Target},
        kube::relationships::fetch_target,
    };
    for code in [200, 403, 406] {
        let server = Server::with_request(move |request| {
            assert!(request.starts_with("GET /api/v1/namespaces/default/secrets/s "));
            let header = request.lines().find(|line| line.to_ascii_lowercase().starts_with("accept:")).expect("accept");
            assert!(header.contains("as=PartialObjectMetadata"));
            assert!(!header.contains(','), "no full-object fallback");
            let body = if code == 200 { json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"default","name":"s","uid":"secret-uid"}}) }
            else { json!({"kind":"Status","apiVersion":"v1","status":"Failure","message":"private sentinel","reason":"Forbidden","code":code}) };
            (code, body.to_string())
        }).await;
        let mut secret = resource();
        secret.api.kind = "Secret".into();
        secret.api.plural = "secrets".into();
        let connection = connection_with(server.client(), vec![secret]);
        let source = Object::new(pod("p-uid", "1", "p"));
        let target = Target {
            api_version: "v1".into(),
            kind: "Secret".into(),
            namespace: "default".into(),
            name: "s".into(),
            expected_uid: None,
            provenance: Provenance::ExplicitReference,
        };
        let result =
            fetch_target(&connection, 1, &source, &target, &CancellationToken::new()).await;
        if code == 200 {
            let (_, id, object) = result.expect("metadata");
            assert_eq!(id.uid, "secret-uid");
            assert!(object.value.get("data").is_none());
        } else {
            assert_eq!(
                result.expect_err("explicit unknown"),
                if code == 403 {
                    Unknown::Forbidden
                } else {
                    Unknown::Unsupported
                }
            );
        }
    }
}

#[tokio::test]
async fn graph_target_replacement_and_precancel_reject_identity() {
    use sauron::{
        evidence::Unknown,
        graph::{Provenance, references::Target},
        kube::relationships::fetch_target,
    };
    let server = Server::new(|_| (200, pod("replacement", "2", "p").to_string())).await;
    let connection = connection(server.client());
    let source = Object::new(pod("source", "1", "child"));
    let target = Target {
        api_version: "v1".into(),
        kind: "Pod".into(),
        namespace: "default".into(),
        name: "p".into(),
        expected_uid: Some("original".into()),
        provenance: Provenance::OwnerReference,
    };
    let cancel = CancellationToken::new();
    assert_eq!(
        fetch_target(&connection, 1, &source, &target, &cancel)
            .await
            .expect_err("replacement"),
        Unknown::TargetReplaced
    );
    cancel.cancel();
    assert_eq!(
        fetch_target(&connection, 1, &source, &target, &cancel)
            .await
            .expect_err("cancel"),
        Unknown::Stale
    );
}

#[tokio::test]
async fn graph_reverse_ownership_is_uid_scoped_bounded_and_metadata_only() {
    use sauron::kube::relationships::children;
    let server = Server::with_request(|request| {
        assert!(request.contains("limit=200"));
        assert!(request.contains("as=PartialObjectMetadataList"));
        let matching = json!({"apiVersion":"apps/v1","kind":"ReplicaSet","name":"rs","uid":"owner"});
        let stale = json!({"apiVersion":"apps/v1","kind":"ReplicaSet","name":"rs","uid":"replacement"});
        let items: Vec<_> = (0..60).map(|n| json!({"metadata":{"namespace":"default","name":format!("child-{n}"),"uid":format!("uid-{n}"),"ownerReferences":[if n==0 {stale.clone()} else {matching.clone()}]}})).collect();
        (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadataList","metadata":{"continue":"more"},"items":items}).to_string())
    }).await;
    let connection = connection(server.client());
    let owner = Object::new(
        json!({"apiVersion":"apps/v1","kind":"ReplicaSet","metadata":{"namespace":"default","name":"rs","uid":"owner"}}),
    );
    let (objects, partial, inspected) =
        children(&connection, &owner, &resource(), &CancellationToken::new())
            .await
            .expect("list");
    assert!(partial);
    assert_eq!(inspected, 60);
    assert_eq!(objects.len(), 50);
    assert!(!objects.iter().any(|o| o.name == "child-0"));
    assert!(objects.iter().all(|o| o.value.get("spec").is_none()));
}

#[tokio::test]
async fn graph_report_keeps_verified_edges_when_another_source_is_forbidden() {
    use sauron::evidence::Unknown;
    use sauron::kube::relationships::report::adjacent;
    let root = json!({"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"p","uid":"root","resourceVersion":"1"},"spec":{"volumes":[{"configMap":{"name":"cm"}},{"secret":{"secretName":"s"}}]}});
    let body = root.to_string();
    let server = Server::new(move |path| {
        if path.ends_with("/pods/p") { (200,body.clone()) }
        else if path.ends_with("/configmaps/cm") { (200,json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"namespace":"default","name":"cm","uid":"cm"}}).to_string()) }
        else if path.ends_with("/secrets/s") { (403,json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"Forbidden","message":"private sentinel","code":403}).to_string()) }
        else { (200,json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadataList","items":[]}).to_string()) }
    }).await;
    let mut cm = resource();
    cm.api.kind = "ConfigMap".into();
    cm.api.plural = "configmaps".into();
    let mut secret = resource();
    secret.api.kind = "Secret".into();
    secret.api.plural = "secrets".into();
    let connection = connection_with(server.client(), vec![resource(), cm, secret]);
    let report = adjacent(
        &connection,
        1,
        &resource(),
        &Object::new(root),
        &CancellationToken::new(),
    )
    .await
    .expect("partial usable graph");
    assert_eq!(report.graph.edges().len(), 1);
    assert!(report.issues.iter().any(|i| i.reason == Unknown::Forbidden));
    assert!(
        report
            .issues
            .iter()
            .all(|i| !i.source.contains("private sentinel"))
    );
    assert_eq!(report.requests, 5);
}

#[tokio::test]
async fn graph_report_reverse_service_selector_from_pod_root() {
    use sauron::graph::Provenance;
    use sauron::kube::relationships::report::adjacent;
    let root = json!({"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"p","uid":"root","resourceVersion":"1","labels":{"app":"web"}}});
    let body = root.to_string();
    let server = Server::new(move |path| {
        if path.ends_with("/pods/p") {
            (200, body.clone())
        } else if path.contains("/namespaces/default/pods") {
            (200,json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadataList","items":[]}).to_string())
        } else if path.contains("/namespaces/default/services") {
            (200, json!({"items":[
                {"apiVersion":"v1","kind":"Service","metadata":{"namespace":"default","name":"svc-match","uid":"svc-match"},"spec":{"selector":{"app":"web"}}},
                {"apiVersion":"v1","kind":"Service","metadata":{"namespace":"default","name":"svc-other","uid":"svc-other"},"spec":{"selector":{"app":"other"}}}
            ]}).to_string())
        } else {
            (200, json!({"items":[]}).to_string())
        }
    }).await;
    let mut service = resource();
    service.api.kind = "Service".into();
    service.api.plural = "services".into();
    let connection = connection_with(server.client(), vec![resource(), service]);
    let report = adjacent(
        &connection,
        1,
        &resource(),
        &Object::new(root),
        &CancellationToken::new(),
    )
    .await
    .expect("pod report with reverse service selector edge");
    let edges: Vec<_> = report.graph.edges().keys().collect();
    assert!(edges.iter().any(|e| e.from.resource == "v1/services"
        && e.to.resource == "v1/pods"
        && e.provenance == Provenance::SelectorMatch));
    assert!(
        !report
            .nodes
            .keys()
            .any(|id| id.resource == "v1/services" && id.name == "svc-other")
    );
}

#[tokio::test]
async fn graph_report_reverse_configmap_reference_from_pod_and_deployment() {
    use sauron::graph::Provenance;
    use sauron::kube::relationships::report::adjacent;
    let root = json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"namespace":"default","name":"cm","uid":"cm-uid","resourceVersion":"1"}});
    let body = root.to_string();
    let server = Server::new(move |path| {
        if path.ends_with("/configmaps/cm") {
            (200, body.clone())
        } else if path.contains("/namespaces/default/pods") {
            (200, json!({"items":[
                {"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"mounts-cm","uid":"pod-match"},"spec":{"volumes":[{"configMap":{"name":"cm"}}]}},
                {"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"unrelated","uid":"pod-other"},"spec":{"volumes":[{"configMap":{"name":"other"}}]}}
            ]}).to_string())
        } else if path.contains("/namespaces/default/deployments") {
            (200, json!({"items":[
                {"apiVersion":"apps/v1","kind":"Deployment","metadata":{"namespace":"default","name":"web","uid":"dep-match"},"spec":{"template":{"spec":{"containers":[{"envFrom":[{"configMapRef":{"name":"cm"}}]}]}}}}
            ]}).to_string())
        } else {
            (200, json!({"items":[]}).to_string())
        }
    })
    .await;
    let mut configmap = resource();
    configmap.api.kind = "ConfigMap".into();
    configmap.api.plural = "configmaps".into();
    let mut deployment = resource();
    deployment.api.kind = "Deployment".into();
    deployment.api.group = "apps".into();
    deployment.api.api_version = "apps/v1".into();
    deployment.api.plural = "deployments".into();
    let connection = connection_with(
        server.client(),
        vec![configmap.clone(), resource(), deployment],
    );
    let report = adjacent(
        &connection,
        1,
        &configmap,
        &Object::new(root),
        &CancellationToken::new(),
    )
    .await
    .expect("configmap reverse reference report");
    let edges: Vec<_> = report.graph.edges().keys().collect();
    assert!(edges.iter().any(|e| e.from.resource == "v1/pods"
        && e.to.resource == "v1/configmaps"
        && e.provenance == Provenance::ExplicitReference));
    assert!(
        edges
            .iter()
            .any(|e| e.from.resource == "apps/v1/deployments"
                && e.to.resource == "v1/configmaps"
                && e.provenance == Provenance::ExplicitReference)
    );
    assert!(
        !report
            .nodes
            .keys()
            .any(|id| id.resource == "v1/pods" && id.name == "unrelated")
    );
}

#[tokio::test]
async fn xray_traverses_two_hops_cycle_safely_and_bounds_at_depth() {
    use sauron::graph::Provenance;
    use sauron::kube::relationships::report::xray;
    let dep_json = json!({"apiVersion":"apps/v1","kind":"Deployment","metadata":{"namespace":"default","name":"dep","uid":"dep-uid","resourceVersion":"1"}});
    let dep_body = dep_json.to_string();
    let root = Object::new(dep_json);
    // A real ownerReference back to the root: proves revisiting an already-
    // expanded node during deeper traversal never re-expands or cycles.
    let rs_body = json!({"apiVersion":"apps/v1","kind":"ReplicaSet","metadata":{"namespace":"default","name":"rs","uid":"rs-uid","ownerReferences":[{"apiVersion":"apps/v1","kind":"Deployment","name":"dep","uid":"dep-uid"}]}}).to_string();
    let server = Server::new(move |path| {
        if path.ends_with("/deployments/dep") {
            (200, dep_body.clone())
        } else if path.contains("/replicasets/rs") {
            (200, rs_body.clone())
        } else if path.contains("/namespaces/default/deployments") {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadataList","items":[]}).to_string())
        } else if path.contains("/namespaces/default/replicasets") {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadataList","items":[
                {"metadata":{"namespace":"default","name":"rs","uid":"rs-uid","ownerReferences":[{"apiVersion":"apps/v1","kind":"Deployment","name":"dep","uid":"dep-uid"}]}}
            ]}).to_string())
        } else if path.contains("/namespaces/default/pods") {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadataList","items":[
                {"metadata":{"namespace":"default","name":"pod","uid":"pod-uid","ownerReferences":[{"apiVersion":"apps/v1","kind":"ReplicaSet","name":"rs","uid":"rs-uid"}]}}
            ]}).to_string())
        } else {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadataList","items":[]}).to_string())
        }
    })
    .await;
    let mut deployment = resource();
    deployment.api.kind = "Deployment".into();
    deployment.api.group = "apps".into();
    deployment.api.api_version = "apps/v1".into();
    deployment.api.plural = "deployments".into();
    let mut replicaset = resource();
    replicaset.api.kind = "ReplicaSet".into();
    replicaset.api.group = "apps".into();
    replicaset.api.api_version = "apps/v1".into();
    replicaset.api.plural = "replicasets".into();
    let connection = connection_with(
        server.client(),
        vec![deployment.clone(), resource(), replicaset],
    );
    // depth 1 behaves exactly like Adjacent: only the direct neighbor (the
    // owned ReplicaSet) is visible, the Pod two hops away is not.
    let shallow = xray(
        &connection,
        1,
        &deployment,
        &root,
        &CancellationToken::new(),
        1,
    )
    .await
    .expect("depth 1");
    assert!(
        shallow
            .nodes
            .keys()
            .any(|id| id.resource == "apps/v1/replicasets")
    );
    assert!(!shallow.nodes.keys().any(|id| id.resource == "v1/pods"));
    // depth 2 follows one more hop: the ReplicaSet is re-fetched fresh and
    // expanded, discovering the Pod it owns.
    let deep = xray(
        &connection,
        1,
        &deployment,
        &root,
        &CancellationToken::new(),
        2,
    )
    .await
    .expect("depth 2");
    assert!(deep.nodes.keys().any(|id| id.resource == "v1/pods"));
    // The RS's own ownerReference back to the already-expanded root produces
    // an edge, not a second expansion or an infinite loop.
    assert!(
        deep.graph
            .edges()
            .keys()
            .any(|e| e.from.resource == "apps/v1/replicasets"
                && e.to.resource == "apps/v1/deployments"
                && e.provenance == Provenance::OwnerReference)
    );
    assert!(
        deep.issues
            .iter()
            .any(|i| i.source.contains("traversal bounded at depth 2"))
    );
}

#[tokio::test]
async fn graph_report_rechecks_root_uid_after_collection() {
    use sauron::{evidence::Unknown, kube::relationships::report::adjacent};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let reads = AtomicUsize::new(0);
    let server = Server::new(move |path| {
        if path.ends_with("/pods/p") {
            let uid = if reads.fetch_add(1,Ordering::SeqCst)==0 {"original"} else {"replacement"};
            (200,pod(uid,"1","p").to_string())
        } else { (200,json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadataList","items":[]}).to_string()) }
    }).await;
    let result = adjacent(
        &connection(server.client()),
        1,
        &resource(),
        &Object::new(pod("original", "1", "p")),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(result, Err(Unknown::TargetReplaced)));
}

#[tokio::test]
async fn graph_response_allocation_is_bounded_before_json_decode() {
    use sauron::{
        evidence::Unknown,
        graph::{Provenance, references::Target},
        kube::relationships::fetch_target,
    };
    let server = Server::new(|_| (200, " ".repeat(2 * 1024 * 1024 + 1))).await;
    let c = connection(server.client());
    let source = Object::new(pod("original", "1", "p"));
    let target = Target {
        api_version: "v1".into(),
        kind: "Pod".into(),
        namespace: "default".into(),
        name: "p".into(),
        expected_uid: Some("original".into()),
        provenance: Provenance::ExplicitReference,
    };
    assert_eq!(
        fetch_target(&c, 1, &source, &target, &CancellationToken::new())
            .await
            .expect_err("bounded response"),
        Unknown::Partial
    );
}
fn mutation_intent(
    uid: &str,
    effect: sauron::mutation::MutationEffect,
) -> sauron::mutation::MutationIntent {
    use sauron::{app::session::Scope, mutation::MutationTarget};
    sauron::mutation::MutationIntent {
        request_id: 1,
        target: MutationTarget {
            scope: Scope {
                epoch: 1,
                request: 1,
                context: "fake".into(),
                cluster: "fake".into(),
                resource: "v1/configmaps".into(),
                namespace: "sauron-m7".into(),
                name: "m7-target".into(),
                uid: uid.into(),
            },
            resource: {
                let mut r = resource();
                r.api.kind = "ConfigMap".into();
                r.api.plural = "configmaps".into();
                r
            },
            expected_resource_version: None,
        },
        effect,
        risk: sauron::mutation::MutationRisk::Routine,
        summary: "metadata.annotations[\"m7-proof\"]".into(),
        payload_sha256: Some("hash".into()),
        source_action: "test".into(),
        create_resource: None,
    }
}
fn verified_policy_context() -> sauron::mutation::policy::PolicyContext {
    sauron::mutation::policy::PolicyContext {
        readonly: false,
        readonly_forced: false,
        cluster_verified_for_mutation: true,
        ..Default::default()
    }
}
fn test_journal() -> (sauron::mutation::journal::Journal, std::path::PathBuf) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "sauron-mutation-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let path = dir.join("journal.jsonl");
    (sauron::mutation::journal::Journal::new(&path), dir)
}

#[tokio::test]
async fn mutation_policy_denial_sends_zero_http_writes() {
    use sauron::kube::mutation::commit;
    let server = Server::new(|_| panic!("policy denial must never reach the transport")).await;
    let connection = connection(server.client());
    let mut ctx = verified_policy_context();
    ctx.readonly = true;
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
    let outcome = commit(
        &connection,
        &ctx,
        1,
        &intent,
        None,
        Some(json!({"metadata":{"annotations":{"m7-proof":"nonce"}}})),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Denied);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_missing_confirmation_sends_zero_http_writes() {
    use sauron::kube::mutation::commit;
    let server =
        Server::new(|_| panic!("missing confirmation must never reach the transport")).await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &intent,
        None,
        Some(json!({"metadata":{"annotations":{"m7-proof":"nonce"}}})),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Denied);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_uid_mismatch_before_commit_sends_zero_mutation_request() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::Confirmation;
    let server = Server::with_request(|request| {
        assert!(
            request.starts_with("GET"),
            "a UID mismatch on the revalidation GET must prevent any write request: {request}"
        );
        (
            200,
            json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"replaced-uid","resourceVersion":"9"}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
    let confirmation = Confirmation {
        request_id: intent.request_id,
        scope: intent.target.scope.clone(),
        effect: intent.effect,
        payload_sha256: intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &intent,
        Some(&confirmation),
        Some(json!({"metadata":{"annotations":{"m7-proof":"nonce"}}})),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::TargetReplaced);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_precommit_journal_failure_sends_zero_http_writes() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::Confirmation;
    let server =
        Server::new(|_| panic!("a pre-commit journal failure must fail closed before any write"))
            .await;
    let connection = connection(server.client());
    // A path that cannot be created (parent is a regular file, not a
    // directory) reliably fails every append() without touching the network.
    let blocked =
        std::env::temp_dir().join(format!("sauron-mutation-blocked-{}", std::process::id()));
    std::fs::write(&blocked, "not a directory").unwrap();
    let journal = sauron::mutation::journal::Journal::new(blocked.join("journal.jsonl"));
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
    let confirmation = Confirmation {
        request_id: intent.request_id,
        scope: intent.target.scope.clone(),
        effect: intent.effect,
        payload_sha256: intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &intent,
        Some(&confirmation),
        Some(json!({"metadata":{"annotations":{"m7-proof":"nonce"}}})),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Denied);
    std::fs::remove_file(&blocked).ok();
}

#[tokio::test]
async fn mutation_commit_revalidates_then_patches_and_journals() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::Confirmation;
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(request.starts_with("PATCH"), "unexpected method: {request}");
            assert!(!request.contains("dryRun"), "a real commit must not carry dryRun: {request}");
            (200, json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
    let confirmation = Confirmation {
        request_id: intent.request_id,
        scope: intent.target.scope.clone(),
        effect: intent.effect,
        payload_sha256: intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &intent,
        Some(&confirmation),
        Some(json!({"metadata":{"annotations":{"m7-proof":"nonce"}}})),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    let records = journal.recent(10);
    assert!(records.iter().any(
        |r| r.phase == sauron::mutation::journal::Phase::CommitResult
            && r.outcome.as_deref() == Some("Committed")
    ));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_dry_run_preflight_never_commits_and_is_journaled_distinctly() {
    use sauron::kube::mutation::preflight;
    let server = Server::with_request(|request| {
        assert!(request.starts_with("PATCH"), "unexpected method: {request}");
        assert!(request.contains("dryRun=All"), "preflight must request a server dry-run: {request}");
        (200, json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"}}).to_string())
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
    let outcome = preflight(
        &connection,
        &intent,
        Some(&json!({"metadata":{"annotations":{"m7-proof":"nonce"}}})),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    let records = journal.recent(10);
    assert!(
        records
            .iter()
            .any(|r| r.phase == sauron::mutation::journal::Phase::PreflightResult)
    );
    assert!(
        !records
            .iter()
            .any(|r| r.phase == sauron::mutation::journal::Phase::CommitResult),
        "a dry-run preflight must never be journaled as a commit"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_delete_commit_carries_the_exact_uid_server_side_precondition() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::Confirmation;
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(request.starts_with("DELETE"), "unexpected method: {request}");
            assert!(
                request.contains("\"uid\":\"uid-1\""),
                "server-side delete precondition must carry the exact previewed UID: {request}"
            );
            (200, json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Delete);
    let confirmation = Confirmation {
        request_id: intent.request_id,
        scope: intent.target.scope.clone(),
        effect: intent.effect,
        payload_sha256: intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Strong,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &intent,
        Some(&confirmation),
        None,
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_force_delete_sends_grace_period_zero_and_the_uid_precondition() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::{Confirmation, workflow};
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(request.starts_with("DELETE"), "unexpected method: {request}");
            assert!(
                request.contains("\"gracePeriodSeconds\":0"),
                "force delete must send grace_period_seconds=0: {request}"
            );
            assert!(
                request.contains("\"uid\":\"uid-1\""),
                "force delete must still carry the exact UID precondition: {request}"
            );
            (200, json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let scope = sauron::app::session::Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/pods".into(),
        namespace: "sauron-m7".into(),
        name: "m7-target".into(),
        uid: "uid-1".into(),
    };
    let mut pod_resource = resource();
    pod_resource.api.kind = "Pod".into();
    pod_resource.api.plural = "pods".into();
    let built =
        workflow::force_delete(scope, pod_resource, 1).expect("force_delete supported for Pod");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Strong,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload,
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    let records = journal.recent(10);
    assert!(
        records.iter().any(
            |r| r.phase == sauron::mutation::journal::Phase::CommitResult
                && r.detail.as_deref() == Some("Committed (grace_period_seconds=0)")
        ),
        "the journal detail must distinguish a force delete from an ordinary one: {records:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_ordinary_delete_never_sends_grace_period_zero_force_delete_exists_alongside_it() {
    // Regression proof on M8.3's own delete path: force_delete's existence
    // as a genuinely separate function/code path must never leak
    // grace_period_seconds=0 into an ordinary :delete.
    use sauron::kube::mutation::commit;
    use sauron::mutation::Confirmation;
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(request.starts_with("DELETE"), "unexpected method: {request}");
            assert!(
                !request.contains("gracePeriodSeconds"),
                "an ordinary delete must never carry a gracePeriodSeconds field at all: {request}"
            );
            (200, json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Delete);
    let confirmation = Confirmation {
        request_id: intent.request_id,
        scope: intent.target.scope.clone(),
        effect: intent.effect,
        payload_sha256: intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Strong,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &intent,
        Some(&confirmation),
        None,
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_cordon_requires_strong_confirmation_and_commits_the_exact_patch() {
    use sauron::app::session::Scope;
    use sauron::kube::{discovery::Resource, mutation::commit};
    use sauron::mutation::{Confirmation, PolicyDecision, policy, workflow};
    let node_scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/nodes".into(),
        namespace: String::new(),
        name: "node-1".into(),
        uid: "uid-1".into(),
    };
    let node_resource = Resource {
        api: {
            let mut a = resource().api;
            a.group = String::new();
            a.kind = "Node".into();
            a.plural = "nodes".into();
            a
        },
        namespaced: false,
        short_names: vec![],
        verbs: vec!["patch".into()],
    };
    let built =
        workflow::cordon(node_scope.clone(), node_resource, 1).expect("cordon supported for Node");
    let evaluation = policy::evaluate(&verified_policy_context(), &built.intent);
    assert_eq!(
        evaluation.decision,
        PolicyDecision::RequireStrongerConfirmation,
        "Node is cluster-critical; cordon must never be a weak-confirmation action"
    );

    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"name":"node-1","uid":"uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(request.starts_with("PATCH"), "unexpected method: {request}");
            assert!(
                request.contains("\"unschedulable\":true"),
                "the exact cordon payload must be sent: {request}"
            );
            (200, json!({"apiVersion":"v1","kind":"Node","metadata":{"name":"node-1","uid":"uid-1"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Strong,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload,
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_flux_suspend_and_reconcile_commit_via_the_generic_modify_path() {
    use sauron::app::session::Scope;
    use sauron::kube::{discovery::Resource, mutation::commit};
    use sauron::mutation::{
        Confirmation, ConfirmationRequirement, MutationOutcome, PolicyDecision, policy, workflow,
    };
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "kustomize.toolkit.fluxcd.io/v1/kustomizations".into(),
        namespace: "sauron-m9".into(),
        name: "podinfo-kustomize".into(),
        uid: "uid-1".into(),
    };
    let flux_resource = Resource {
        api: {
            let mut a = resource().api;
            a.group = "kustomize.toolkit.fluxcd.io".into();
            a.api_version = "kustomize.toolkit.fluxcd.io/v1".into();
            a.kind = "Kustomization".into();
            a.plural = "kustomizations".into();
            a
        },
        namespaced: true,
        short_names: vec![],
        verbs: vec!["patch".into()],
    };

    // --- Suspend: commit only (a dedicated `verify()`-only test below
    // proves the zero-new-verification-code claim, matching
    // `verify_cordon_confirms_observed_unschedulable_flip_with_zero_new_
    // code`'s own precedent of testing revalidate+PATCH and a fresh-read
    // verify against two independently-scripted fake servers, never one
    // static-response server standing in for both a pre-mutation
    // revalidation GET and a post-mutation verification GET at once).
    let built = workflow::flux_suspend(scope.clone(), flux_resource.clone(), 1)
        .expect("flux_suspend supported for Kustomization");
    let evaluation = policy::evaluate(&verified_policy_context(), &built.intent);
    assert_eq!(evaluation.decision, PolicyDecision::RequireConfirmation);
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m9","name":"podinfo-kustomize","uid":"uid-1","resourceVersion":"5"}}).to_string())
        } else {
            assert!(request.starts_with("PATCH"), "unexpected method: {request}");
            assert!(request.contains("\"suspend\":true"), "the exact suspend payload must be sent: {request}");
            (200, json!({"apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization","metadata":{"namespace":"sauron-m9","name":"podinfo-kustomize","uid":"uid-1"},"spec":{"suspend":true}}).to_string())
        }
    })
    .await;
    let fake_connection = connection(server.client());
    let (journal, dir) = test_journal();
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &fake_connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();

    // --- Reconcile: commit only, same rationale as above.
    let built = workflow::flux_reconcile(scope, flux_resource, "2026-09-19T00:00:00Z", 2)
        .expect("flux_reconcile supported for Kustomization");
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m9","name":"podinfo-kustomize","uid":"uid-1","resourceVersion":"5"}}).to_string())
        } else {
            assert!(request.starts_with("PATCH"), "unexpected method: {request}");
            assert!(request.contains("reconcile.fluxcd.io/requestedAt"), "the exact reconcile annotation must be sent: {request}");
            (200, json!({"apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization","metadata":{"namespace":"sauron-m9","name":"podinfo-kustomize","uid":"uid-1","annotations":{"reconcile.fluxcd.io/requestedAt":"2026-09-19T00:00:00Z"}}}).to_string())
        }
    })
    .await;
    let fake_connection = connection(server.client());
    let (journal, dir) = test_journal();
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &fake_connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn verify_flux_suspend_and_reconcile_confirm_via_the_generic_modify_path_zero_new_code() {
    use sauron::app::session::Scope;
    use sauron::kube::{discovery::Resource, mutation::verify};
    use sauron::mutation::workflow;
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "kustomize.toolkit.fluxcd.io/v1/kustomizations".into(),
        namespace: "sauron-m9".into(),
        name: "podinfo-kustomize".into(),
        uid: "uid-1".into(),
    };
    let flux_resource = Resource {
        api: {
            let mut a = resource().api;
            a.group = "kustomize.toolkit.fluxcd.io".into();
            a.api_version = "kustomize.toolkit.fluxcd.io/v1".into();
            a.kind = "Kustomization".into();
            a.plural = "kustomizations".into();
            a
        },
        namespaced: true,
        short_names: vec![],
        verbs: vec!["patch".into()],
    };

    // Design test: a fresh read already reflecting `spec.suspend: true`
    // must verify as `Verified` with zero new `kube::mutation::verify`
    // dispatch code -- the exact same generic `Modify`/
    // `leaf_path_and_value` path, plus M8B.1's own boolean-`omitempty`
    // fix, that Cordon already exercises.
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization","metadata":{"namespace":"sauron-m9","name":"podinfo-kustomize","uid":"uid-1"},"spec":{"suspend":true}}).to_string(),
        )
    })
    .await;
    let fake_connection = connection(server.client());
    let built = workflow::flux_suspend(scope.clone(), flux_resource.clone(), 1)
        .expect("flux_suspend supported for Kustomization");
    let outcome = verify(
        &fake_connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::Verification::Verified);

    // Reconcile: same generic path as M8's own Restart annotation bump.
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization","metadata":{"namespace":"sauron-m9","name":"podinfo-kustomize","uid":"uid-1","annotations":{"reconcile.fluxcd.io/requestedAt":"2026-09-19T00:00:00Z"}}}).to_string(),
        )
    })
    .await;
    let fake_connection = connection(server.client());
    let built = workflow::flux_reconcile(scope, flux_resource, "2026-09-19T00:00:00Z", 2)
        .expect("flux_reconcile supported for Kustomization");
    let outcome = verify(
        &fake_connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::Verification::Verified);
}

#[tokio::test]
async fn mutation_argocd_sync_and_rollback_commit_the_exact_operation_field() {
    use sauron::app::session::Scope;
    use sauron::kube::{discovery::Resource, mutation::commit};
    use sauron::mutation::{
        Confirmation, ConfirmationRequirement, MutationOutcome, PolicyDecision, policy, workflow,
    };
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "argoproj.io/v1alpha1/applications".into(),
        namespace: "argocd".into(),
        name: "guestbook".into(),
        uid: "uid-1".into(),
    };
    let argocd_resource = Resource {
        api: {
            let mut a = resource().api;
            a.group = "argoproj.io".into();
            a.api_version = "argoproj.io/v1alpha1".into();
            a.kind = "Application".into();
            a.plural = "applications".into();
            a
        },
        namespaced: true,
        short_names: vec![],
        verbs: vec!["patch".into()],
    };

    // --- Sync: no explicit revision.
    let built = workflow::argocd_sync(scope.clone(), argocd_resource.clone(), 1)
        .expect("argocd_sync supported for Application");
    let evaluation = policy::evaluate(&verified_policy_context(), &built.intent);
    assert_eq!(evaluation.decision, PolicyDecision::RequireConfirmation);
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1","resourceVersion":"5"}}).to_string())
        } else {
            assert!(request.starts_with("PATCH"), "unexpected method: {request}");
            assert!(request.contains("\"operation\":{\"sync\":{}}"), "the exact sync payload must be sent: {request}");
            (200, json!({"apiVersion":"argoproj.io/v1alpha1","kind":"Application","metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1"}}).to_string())
        }
    })
    .await;
    let fake_connection = connection(server.client());
    let (journal, dir) = test_journal();
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &fake_connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();

    // --- Rollback: explicit prior revision.
    let built = workflow::argocd_rollback(
        scope,
        argocd_resource,
        "8088f4c0d970abb09e250248cc97e35623447cb5",
        2,
    )
    .expect("argocd_rollback supported for Application");
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1","resourceVersion":"5"}}).to_string())
        } else {
            assert!(request.starts_with("PATCH"), "unexpected method: {request}");
            assert!(request.contains("8088f4c0d970abb09e250248cc97e35623447cb5"), "the exact rollback revision must be sent: {request}");
            (200, json!({"apiVersion":"argoproj.io/v1alpha1","kind":"Application","metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1"}}).to_string())
        }
    })
    .await;
    let fake_connection = connection(server.client());
    let (journal, dir) = test_journal();
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &fake_connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn verify_argocd_sync_and_rollback_via_the_dedicated_operation_comparison() {
    use sauron::app::session::Scope;
    use sauron::kube::{discovery::Resource, mutation::verify};
    use sauron::mutation::{Verification, workflow};
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "argoproj.io/v1alpha1/applications".into(),
        namespace: "argocd".into(),
        name: "guestbook".into(),
        uid: "uid-1".into(),
    };
    let argocd_resource = Resource {
        api: {
            let mut a = resource().api;
            a.group = "argoproj.io".into();
            a.api_version = "argoproj.io/v1alpha1".into();
            a.kind = "Application".into();
            a.plural = "applications".into();
            a
        },
        namespaced: true,
        short_names: vec![],
        verbs: vec!["patch".into()],
    };

    // Plain sync: verified once ANY operationState is recorded.
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"argoproj.io/v1alpha1","kind":"Application","metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1"},"status":{"operationState":{"phase":"Running"}}}).to_string(),
        )
    })
    .await;
    let fake_connection = connection(server.client());
    let built = workflow::argocd_sync(scope.clone(), argocd_resource.clone(), 1)
        .expect("argocd_sync supported for Application");
    let outcome = verify(
        &fake_connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, Verification::Verified);

    // Sync with no operationState recorded yet: Pending, not a failure.
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"argoproj.io/v1alpha1","kind":"Application","metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1"}}).to_string(),
        )
    })
    .await;
    let fake_connection = connection(server.client());
    let outcome = verify(
        &fake_connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, Verification::Pending);

    // Rollback: verified only once the exact requested revision is echoed
    // back in status.operationState.operation.sync.revision.
    let built = workflow::argocd_rollback(scope, argocd_resource, "abc123", 2)
        .expect("argocd_rollback supported for Application");
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"argoproj.io/v1alpha1","kind":"Application","metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1"},"status":{"operationState":{"phase":"Running","operation":{"sync":{"revision":"abc123"}}}}}).to_string(),
        )
    })
    .await;
    let fake_connection = connection(server.client());
    let outcome = verify(
        &fake_connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, Verification::Verified);

    // Rollback with the WRONG revision echoed back: Pending, never a
    // false Verified.
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"argoproj.io/v1alpha1","kind":"Application","metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1"},"status":{"operationState":{"phase":"Running","operation":{"sync":{"revision":"different"}}}}}).to_string(),
        )
    })
    .await;
    let fake_connection = connection(server.client());
    let outcome = verify(
        &fake_connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, Verification::Pending);
}

fn modify_intent_with_payload(
    uid: &str,
    payload: serde_json::Value,
) -> (sauron::mutation::MutationIntent, serde_json::Value) {
    (
        mutation_intent(uid, sauron::mutation::MutationEffect::Modify),
        payload,
    )
}

#[tokio::test]
async fn verify_scale_confirms_observed_desired_replicas() {
    use sauron::kube::mutation::verify;
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"apps/v1","kind":"Deployment","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"},"spec":{"replicas":5}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (intent, payload) = modify_intent_with_payload("uid-1", json!({"spec":{"replicas":5}}));
    let outcome = verify(
        &connection,
        &intent,
        Some(&payload),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::Verification::Verified);
}

#[tokio::test]
async fn verify_cordon_confirms_observed_unschedulable_flip_with_zero_new_code() {
    use sauron::app::session::Scope;
    use sauron::kube::{discovery::Resource, mutation::verify};
    use sauron::mutation::workflow;
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"v1","kind":"Node","metadata":{"name":"node-1","uid":"uid-1"},"spec":{"unschedulable":true}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let node_scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/nodes".into(),
        namespace: String::new(),
        name: "node-1".into(),
        uid: "uid-1".into(),
    };
    let node_resource = Resource {
        api: {
            let mut a = resource().api;
            a.group = String::new();
            a.kind = "Node".into();
            a.plural = "nodes".into();
            a
        },
        namespaced: false,
        short_names: vec![],
        verbs: vec!["patch".into()],
    };
    let built = workflow::cordon(node_scope, node_resource, 1).expect("cordon supported for Node");
    let outcome = verify(
        &connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::Verification::Verified);
}

#[tokio::test]
async fn verify_uncordon_treats_an_omitted_false_field_as_verified_not_different() {
    // Kubernetes' own `omitempty` convention drops a `false` boolean field
    // from the serialized object entirely -- a real live-cluster finding,
    // not a hypothetical: `spec.unschedulable` is simply absent when a
    // Node is schedulable, never present as an explicit `false`.
    use sauron::app::session::Scope;
    use sauron::kube::{discovery::Resource, mutation::verify};
    use sauron::mutation::workflow;
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"v1","kind":"Node","metadata":{"name":"node-1","uid":"uid-1"},"spec":{}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let node_scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/nodes".into(),
        namespace: String::new(),
        name: "node-1".into(),
        uid: "uid-1".into(),
    };
    let node_resource = Resource {
        api: {
            let mut a = resource().api;
            a.group = String::new();
            a.kind = "Node".into();
            a.plural = "nodes".into();
            a
        },
        namespaced: false,
        short_names: vec![],
        verbs: vec!["patch".into()],
    };
    let built =
        workflow::uncordon(node_scope, node_resource, 1).expect("uncordon supported for Node");
    let outcome = verify(
        &connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        outcome,
        sauron::mutation::Verification::Verified,
        "an absent field must be treated the same as an explicit false"
    );
}

fn deployment_scope_and_resource(
    uid: &str,
) -> (
    sauron::app::session::Scope,
    sauron::kube::discovery::Resource,
) {
    use sauron::app::session::Scope;
    (
        Scope {
            epoch: 1,
            request: 1,
            context: "fake".into(),
            cluster: "fake".into(),
            resource: "apps/v1/deployments".into(),
            namespace: "sauron-m8b".into(),
            name: "m8b-deploy".into(),
            uid: uid.into(),
        },
        {
            let mut r = resource();
            r.api.group = "apps".into();
            r.api.kind = "Deployment".into();
            r.api.plural = "deployments".into();
            r
        },
    )
}

#[tokio::test]
async fn verify_set_image_confirms_the_named_container_and_leaves_others_untouched() {
    use sauron::kube::mutation::verify;
    use sauron::mutation::workflow;
    let server = Server::new(|_| {
        (
            200,
            json!({
                "apiVersion":"apps/v1","kind":"Deployment",
                "metadata":{"namespace":"sauron-m8b","name":"m8b-deploy","uid":"uid-1"},
                "spec":{"template":{"spec":{"containers":[
                    {"name":"web","image":"nginx:1.27"},
                    {"name":"sidecar","image":"envoy:1.30"}
                ]}}}
            })
            .to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (scope, resource) = deployment_scope_and_resource("uid-1");
    let current = vec![
        json!({"name":"web","image":"nginx:1.26"}),
        json!({"name":"sidecar","image":"envoy:1.30"}),
    ];
    let built = workflow::set_image(scope, resource, &current, Some("web"), "nginx:1.27", 1)
        .expect("set_image supported for Deployment");
    let outcome = verify(
        &connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::Verification::Verified);
}

#[tokio::test]
async fn verify_set_image_reports_observed_different_when_the_sidecar_unexpectedly_changed() {
    use sauron::kube::mutation::verify;
    use sauron::mutation::workflow;
    let server = Server::new(|_| {
        (
            200,
            json!({
                "apiVersion":"apps/v1","kind":"Deployment",
                "metadata":{"namespace":"sauron-m8b","name":"m8b-deploy","uid":"uid-1"},
                "spec":{"template":{"spec":{"containers":[
                    {"name":"web","image":"nginx:1.27"},
                    {"name":"sidecar","image":"envoy:1.31"}
                ]}}}
            })
            .to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (scope, resource) = deployment_scope_and_resource("uid-1");
    let current = vec![
        json!({"name":"web","image":"nginx:1.26"}),
        json!({"name":"sidecar","image":"envoy:1.30"}),
    ];
    let built = workflow::set_image(scope, resource, &current, Some("web"), "nginx:1.27", 1)
        .expect("set_image supported for Deployment");
    let outcome = verify(
        &connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        outcome,
        sauron::mutation::Verification::ObservedDifferent(_)
    ));
}

#[tokio::test]
async fn mutation_set_image_commit_sends_the_full_reconstructed_containers_array() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::Confirmation;
    use sauron::mutation::workflow;
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m8b","name":"m8b-deploy","uid":"uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(request.starts_with("PATCH"), "unexpected method: {request}");
            assert!(
                request.contains("\"sidecar\"") && request.contains("envoy:1.30"),
                "the unrelated sidecar container must still be present in the patch body: {request}"
            );
            assert!(
                request.contains("nginx:1.27"),
                "the exact requested image must be sent: {request}"
            );
            (200, json!({"apiVersion":"apps/v1","kind":"Deployment","metadata":{"namespace":"sauron-m8b","name":"m8b-deploy","uid":"uid-1"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let (scope, resource) = deployment_scope_and_resource("uid-1");
    let current = vec![
        json!({"name":"web","image":"nginx:1.26"}),
        json!({"name":"sidecar","image":"envoy:1.30"}),
    ];
    let built = workflow::set_image(scope, resource, &current, Some("web"), "nginx:1.27", 1)
        .expect("set_image supported for Deployment");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload,
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();
}

fn cronjob_and_job_resources() -> (
    sauron::app::session::Scope,
    sauron::kube::discovery::Resource,
    sauron::kube::discovery::Resource,
) {
    use sauron::app::session::Scope;
    let scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "batch/v1/cronjobs".into(),
        namespace: "sauron-m8b".into(),
        name: "nightly".into(),
        uid: "cronjob-uid-1".into(),
    };
    let mut cronjob_resource = resource();
    cronjob_resource.api.group = "batch".into();
    cronjob_resource.api.kind = "CronJob".into();
    cronjob_resource.api.plural = "cronjobs".into();
    let mut job_resource = resource();
    job_resource.api.group = "batch".into();
    job_resource.api.kind = "Job".into();
    job_resource.api.plural = "jobs".into();
    (scope, cronjob_resource, job_resource)
}

#[tokio::test]
async fn mutation_trigger_cronjob_requires_confirmation_and_posts_the_job_manifest() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::{Confirmation, PolicyDecision, policy, workflow};
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m8b","name":"nightly","uid":"cronjob-uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(request.starts_with("POST"), "unexpected method: {request}");
            assert!(
                request.contains("\"kind\":\"Job\"") && request.contains("nightly-trigger-1"),
                "the exact generated Job manifest must be posted: {request}"
            );
            (201, json!({"apiVersion":"batch/v1","kind":"Job","metadata":{"namespace":"sauron-m8b","name":"nightly-trigger-1","uid":"job-uid-1"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (scope, cronjob_resource, job_resource) = cronjob_and_job_resources();
    let template =
        json!({"template": {"spec": {"containers": [{"name":"worker","image":"busybox:1.37"}]}}});
    let built = workflow::trigger_cronjob(
        scope,
        cronjob_resource,
        job_resource,
        &template,
        "nightly-trigger-1",
        1,
    )
    .expect("trigger supported for CronJob");
    let evaluation = policy::evaluate(&verified_policy_context(), &built.intent);
    assert_eq!(
        evaluation.decision,
        PolicyDecision::RequireConfirmation,
        "Create must not silently bypass confirmation"
    );
    let (journal, dir) = test_journal();
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload,
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_trigger_cronjob_dry_run_never_commits() {
    use sauron::kube::mutation::preflight;
    use sauron::mutation::workflow;
    let server = Server::with_request(|request| {
        assert!(request.starts_with("POST"), "unexpected method: {request}");
        assert!(
            request.contains("dryRun=All"),
            "preflight must request a server dry-run: {request}"
        );
        (201, json!({"apiVersion":"batch/v1","kind":"Job","metadata":{"namespace":"sauron-m8b","name":"nightly-trigger-1","uid":"job-uid-1"}}).to_string())
    })
    .await;
    let connection = connection(server.client());
    let (scope, cronjob_resource, job_resource) = cronjob_and_job_resources();
    let template =
        json!({"template": {"spec": {"containers": [{"name":"worker","image":"busybox:1.37"}]}}});
    let built = workflow::trigger_cronjob(
        scope,
        cronjob_resource,
        job_resource,
        &template,
        "nightly-trigger-1",
        1,
    )
    .expect("trigger supported for CronJob");
    let (journal, dir) = test_journal();
    let outcome = preflight(
        &connection,
        &built.intent,
        built.payload.as_ref(),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn verify_trigger_cronjob_confirms_the_created_job_by_its_exact_name() {
    use sauron::kube::mutation::verify;
    use sauron::mutation::workflow;
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"batch/v1","kind":"Job","metadata":{"namespace":"sauron-m8b","name":"nightly-trigger-1","uid":"job-uid-1"}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (scope, cronjob_resource, job_resource) = cronjob_and_job_resources();
    let template =
        json!({"template": {"spec": {"containers": [{"name":"worker","image":"busybox:1.37"}]}}});
    let built = workflow::trigger_cronjob(
        scope,
        cronjob_resource,
        job_resource,
        &template,
        "nightly-trigger-1",
        1,
    )
    .expect("trigger supported for CronJob");
    let outcome = verify(
        &connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        outcome,
        sauron::mutation::Verification::Created(_)
    ));
}

#[tokio::test]
async fn verify_trigger_cronjob_reports_pending_when_the_job_is_not_yet_visible() {
    use sauron::kube::mutation::verify;
    use sauron::mutation::workflow;
    let server = Server::new(|_| (404, String::new())).await;
    let connection = connection(server.client());
    let (scope, cronjob_resource, job_resource) = cronjob_and_job_resources();
    let template =
        json!({"template": {"spec": {"containers": [{"name":"worker","image":"busybox:1.37"}]}}});
    let built = workflow::trigger_cronjob(
        scope,
        cronjob_resource,
        job_resource,
        &template,
        "nightly-trigger-1",
        1,
    )
    .expect("trigger supported for CronJob");
    let outcome = verify(
        &connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::Verification::Pending);
}

#[tokio::test]
async fn drain_orchestrates_cordon_then_a_mixed_batch_of_eviction_outcomes() {
    use sauron::app::session::Scope;
    use std::sync::atomic::{AtomicUsize, Ordering};
    // Three Pods on the node: one DaemonSet-owned (never attempted, excluded
    // during planning), one that evicts successfully, one denied by a PDB.
    let list_body = json!({
        "apiVersion":"v1","kind":"PodList",
        "items": [
            {"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"ds-pod","uid":"uid-ds","ownerReferences":[{"kind":"DaemonSet","name":"x","uid":"y"}]},"spec":{}},
            {"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"ok-pod","uid":"uid-ok"},"spec":{}},
            {"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"denied-pod","uid":"uid-denied"},"spec":{}},
        ],
    })
    .to_string();
    let eviction_attempts = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = eviction_attempts.clone();
    let server = Server::with_request(move |request| {
        if request.starts_with("PATCH") {
            // Cordon commit.
            (200, json!({"apiVersion":"v1","kind":"Node","metadata":{"name":"node-1","uid":"node-uid"}}).to_string())
        } else if request.contains("/eviction") {
            counter.fetch_add(1, Ordering::SeqCst);
            if request.contains("denied-pod") {
                (429, json!({"kind":"Status","apiVersion":"v1","status":"Failure","code":429}).to_string())
            } else {
                (200, json!({"kind":"Status","apiVersion":"v1","status":"Success"}).to_string())
            }
        } else if request.starts_with("GET") && request.contains("fieldSelector") {
            (200, list_body.clone())
        } else if request.starts_with("GET") && request.contains("nodes/node-1") {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"name":"node-1","uid":"node-uid","resourceVersion":"5"}}).to_string())
        } else {
            // Per-Pod revalidate/verify GET -- reuse the same UID either way.
            let (name, uid) = if request.contains("ok-pod") {
                ("ok-pod", "uid-ok")
            } else {
                ("denied-pod", "uid-denied")
            };
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"default","name":name,"uid":uid,"resourceVersion":"5"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let node_scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/nodes".into(),
        namespace: String::new(),
        name: "node-1".into(),
        uid: "node-uid".into(),
    };
    let mut node_resource = resource();
    node_resource.api.group = String::new();
    node_resource.api.kind = "Node".into();
    node_resource.api.plural = "nodes".into();
    node_resource.namespaced = false;
    let mut pod_resource = resource();
    pod_resource.api.kind = "Pod".into();
    pod_resource.api.plural = "pods".into();
    let report = sauron::kube::drain::drain(
        &connection,
        &verified_policy_context(),
        1,
        node_scope,
        node_resource,
        pod_resource,
        1,
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        report.cordon_outcome,
        sauron::mutation::MutationOutcome::Committed
    );
    assert_eq!(
        report.steps.len(),
        3,
        "every planned Pod gets exactly one row, never collapsed"
    );
    let ds_step = report.steps.iter().find(|s| s.name == "ds-pod").unwrap();
    assert!(matches!(
        ds_step.outcome,
        sauron::mutation::drain::StepOutcome::Excluded(
            sauron::mutation::drain::Exclusion::DaemonSetOwned
        )
    ));
    let ok_step = report.steps.iter().find(|s| s.name == "ok-pod").unwrap();
    assert!(matches!(
        ok_step.outcome,
        sauron::mutation::drain::StepOutcome::Attempted(
            sauron::mutation::MutationOutcome::Committed
        )
    ));
    let denied_step = report
        .steps
        .iter()
        .find(|s| s.name == "denied-pod")
        .unwrap();
    assert!(matches!(
        denied_step.outcome,
        sauron::mutation::drain::StepOutcome::Attempted(
            sauron::mutation::MutationOutcome::DisruptionBudgetDenied
        )
    ));
    // Exactly two eviction POSTs -- the DaemonSet-owned Pod's exclusion must
    // never even attempt a request, and a PDB denial must never be retried.
    assert_eq!(eviction_attempts.load(Ordering::SeqCst), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn drain_never_attempts_any_eviction_when_the_cordon_itself_fails() {
    use sauron::app::session::Scope;
    let server = Server::with_request(|request| {
        assert!(
            !request.contains("/eviction"),
            "a failed cordon must mean zero eviction attempts: {request}"
        );
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"name":"node-1","uid":"node-uid","resourceVersion":"5"}}).to_string(),
            )
        } else {
            (403, json!({"kind":"Status","apiVersion":"v1","status":"Failure","code":403}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let node_scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/nodes".into(),
        namespace: String::new(),
        name: "node-1".into(),
        uid: "node-uid".into(),
    };
    let mut node_resource = resource();
    node_resource.api.group = String::new();
    node_resource.api.kind = "Node".into();
    node_resource.api.plural = "nodes".into();
    node_resource.namespaced = false;
    let mut pod_resource = resource();
    pod_resource.api.kind = "Pod".into();
    pod_resource.api.plural = "pods".into();
    let report = sauron::kube::drain::drain(
        &connection,
        &verified_policy_context(),
        1,
        node_scope,
        node_resource,
        pod_resource,
        1,
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        report.cordon_outcome,
        sauron::mutation::MutationOutcome::Forbidden
    );
    assert!(
        report.steps.is_empty(),
        "no Pod list is even fetched when the cordon itself fails"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn drain_pre_cancelled_attempts_nothing_and_reports_zero_steps() {
    use sauron::app::session::Scope;
    let server = Server::new(|_| {
        panic!("a pre-cancelled drain must send zero requests of any kind, including the cordon")
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let node_scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/nodes".into(),
        namespace: String::new(),
        name: "node-1".into(),
        uid: "node-uid".into(),
    };
    let mut node_resource = resource();
    node_resource.api.group = String::new();
    node_resource.api.kind = "Node".into();
    node_resource.api.plural = "nodes".into();
    node_resource.namespaced = false;
    let mut pod_resource = resource();
    pod_resource.api.kind = "Pod".into();
    pod_resource.api.plural = "pods".into();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let report = sauron::kube::drain::drain(
        &connection,
        &verified_policy_context(),
        1,
        node_scope,
        node_resource,
        pod_resource,
        1,
        &journal,
        &cancel,
    )
    .await;
    // Cancellation never undoes anything already committed -- there is
    // nothing to undo here, since nothing was ever attempted: the cordon
    // itself is Cancelled, and Drain correctly never lists (let alone
    // evicts) any Pod on a Node it never even tried to cordon.
    assert_eq!(
        report.cordon_outcome,
        sauron::mutation::MutationOutcome::Cancelled
    );
    assert!(report.steps.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn drain_cancellation_mid_drain_stops_future_evictions_but_never_reverses_completed_steps() {
    use sauron::app::session::Scope;
    // Pods are evicted strictly sequentially (never concurrently), so
    // cancelling exactly when the SECOND Pod's eviction request arrives is
    // deterministic: pod-a's whole step (commit + verify) has already
    // fully resolved by then, and pod-c's step cannot even start until
    // pod-b's step (whatever its own outcome) has also fully resolved --
    // by which point the cancellation flag is unconditionally visible.
    // This is the non-racy replacement for an earlier, abandoned attempt
    // that cancelled from inside the Pod-list handler instead (see
    // docs/M8B_ACCEPTANCE.md's M8B.5 journal entry).
    let list_body = json!({
        "apiVersion":"v1","kind":"PodList",
        "items": [
            {"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"pod-a","uid":"uid-a"},"spec":{}},
            {"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"pod-b","uid":"uid-b"},"spec":{}},
            {"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":"pod-c","uid":"uid-c"},"spec":{}},
        ],
    })
    .to_string();
    let cancel = CancellationToken::new();
    let cancel_from_server = cancel.clone();
    let server = Server::with_request(move |request| {
        if request.starts_with("PATCH") {
            (200, json!({"apiVersion":"v1","kind":"Node","metadata":{"name":"node-1","uid":"node-uid"}}).to_string())
        } else if request.contains("/eviction") {
            if request.contains("pod-b") {
                cancel_from_server.cancel();
            }
            (200, json!({"kind":"Status","apiVersion":"v1","status":"Success"}).to_string())
        } else if request.starts_with("GET") && request.contains("fieldSelector") {
            (200, list_body.clone())
        } else if request.starts_with("GET") && request.contains("nodes/node-1") {
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"name":"node-1","uid":"node-uid","resourceVersion":"5"}}).to_string())
        } else {
            let (name, uid) = if request.contains("pod-a") {
                ("pod-a", "uid-a")
            } else {
                ("pod-b", "uid-b")
            };
            (200, json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"default","name":name,"uid":uid,"resourceVersion":"5"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let node_scope = Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/nodes".into(),
        namespace: String::new(),
        name: "node-1".into(),
        uid: "node-uid".into(),
    };
    let mut node_resource = resource();
    node_resource.api.group = String::new();
    node_resource.api.kind = "Node".into();
    node_resource.api.plural = "nodes".into();
    node_resource.namespaced = false;
    let mut pod_resource = resource();
    pod_resource.api.kind = "Pod".into();
    pod_resource.api.plural = "pods".into();
    let report = sauron::kube::drain::drain(
        &connection,
        &verified_policy_context(),
        1,
        node_scope,
        node_resource,
        pod_resource,
        1,
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(
        report.cordon_outcome,
        sauron::mutation::MutationOutcome::Committed
    );
    assert_eq!(report.steps.len(), 3);
    assert_eq!(
        report.steps[0].outcome,
        sauron::mutation::drain::StepOutcome::Attempted(
            sauron::mutation::MutationOutcome::Committed
        ),
        "pod-a's eviction fully completed before any cancellation -- no rollback"
    );
    assert!(
        matches!(
            report.steps[1].outcome,
            sauron::mutation::drain::StepOutcome::Attempted(_)
        ),
        "pod-b's eviction was already attempted when cancellation fired mid-request"
    );
    assert_eq!(
        report.steps[2].outcome,
        sauron::mutation::drain::StepOutcome::NotAttempted,
        "pod-c must never be attempted once cancelled -- future steps stop, past ones are never reversed"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_evict_pdb_denial_is_distinct_and_never_falls_back_to_delete() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::{Confirmation, workflow};
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m8b","name":"m8b-pod","uid":"uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(
                request.contains("/eviction"),
                "must POST to the eviction subresource, never a plain DELETE: {request}"
            );
            (
                429,
                json!({"kind":"Status","apiVersion":"v1","status":"Failure","code":429,"reason":"TooManyRequests"}).to_string(),
            )
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let scope = pod_scope("uid-1");
    let mut pod_resource = resource();
    pod_resource.api.kind = "Pod".into();
    pod_resource.api.plural = "pods".into();
    let built = workflow::evict(scope, pod_resource, 1).expect("evict is supported for Pod");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Strong,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload,
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        outcome,
        sauron::mutation::MutationOutcome::DisruptionBudgetDenied,
        "a PDB denial must be its own explicit outcome, never blurred with Conflict"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_evict_commits_via_the_eviction_subresource_and_verifies_deletion_in_progress() {
    use sauron::kube::mutation::{commit, verify};
    use sauron::mutation::{Confirmation, workflow};
    let server = Server::with_request(|request| {
        if request.starts_with("GET") && !request.contains("/eviction") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m8b","name":"m8b-pod","uid":"uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(
                request.contains("/eviction"),
                "must POST to the eviction subresource: {request}"
            );
            (200, json!({"kind":"Status","apiVersion":"v1","status":"Success"}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let scope = pod_scope("uid-1");
    let mut pod_resource = resource();
    pod_resource.api.kind = "Pod".into();
    pod_resource.api.plural = "pods".into();
    let built = workflow::evict(scope, pod_resource, 1).expect("evict is supported for Pod");
    let confirmation = Confirmation {
        request_id: built.intent.request_id,
        scope: built.intent.target.scope.clone(),
        effect: built.intent.effect,
        payload_sha256: built.intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Strong,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &built.intent,
        Some(&confirmation),
        built.payload.clone(),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    std::fs::remove_dir_all(&dir).ok();
    // Post-commit verification reuses verify_delete unchanged (a fresh GET,
    // never a second eviction attempt): no new code needed here at all.
    let _ = verify(
        &connection,
        &built.intent,
        built.payload.as_ref(),
        &CancellationToken::new(),
    )
    .await;
}

fn pod_scope(uid: &str) -> sauron::app::session::Scope {
    sauron::app::session::Scope {
        epoch: 1,
        request: 1,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/pods".into(),
        namespace: "sauron-m8b".into(),
        name: "m8b-pod".into(),
        uid: uid.into(),
    }
}

#[tokio::test]
async fn verify_restart_confirms_the_exact_template_annotation_value() {
    use sauron::kube::mutation::verify;
    let server = Server::new(|_| {
        (
            200,
            json!({
                "apiVersion":"apps/v1","kind":"Deployment",
                "metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"},
                "spec":{"template":{"metadata":{"annotations":{
                    "kubectl.kubernetes.io/restartedAt":"2026-09-18T00:00:00Z"
                }}}}
            })
            .to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (intent, payload) = modify_intent_with_payload(
        "uid-1",
        json!({"spec":{"template":{"metadata":{"annotations":{
            "kubectl.kubernetes.io/restartedAt":"2026-09-18T00:00:00Z"
        }}}}}),
    );
    let outcome = verify(
        &connection,
        &intent,
        Some(&payload),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::Verification::Verified);
}

#[tokio::test]
async fn verify_label_removal_confirms_the_key_is_gone_and_ignores_unrelated_metadata() {
    use sauron::kube::mutation::verify;
    let server = Server::new(|_| {
        (
            200,
            json!({
                "apiVersion":"v1","kind":"ConfigMap",
                "metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1",
                    "labels":{"other":"kept"}}
            })
            .to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (intent, payload) =
        modify_intent_with_payload("uid-1", json!({"metadata":{"labels":{"team":null}}}));
    let outcome = verify(
        &connection,
        &intent,
        Some(&payload),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::Verification::Verified);
}

#[tokio::test]
async fn verify_reports_observed_different_when_the_fresh_value_does_not_match() {
    use sauron::kube::mutation::verify;
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"apps/v1","kind":"Deployment","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"},"spec":{"replicas":2}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (intent, payload) = modify_intent_with_payload("uid-1", json!({"spec":{"replicas":5}}));
    let outcome = verify(
        &connection,
        &intent,
        Some(&payload),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(
        outcome,
        sauron::mutation::Verification::ObservedDifferent(_)
    ));
}

#[tokio::test]
async fn verify_reports_target_replaced_when_the_uid_changed_since_commit() {
    use sauron::kube::mutation::verify;
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"apps/v1","kind":"Deployment","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"different-uid"},"spec":{"replicas":5}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (intent, payload) = modify_intent_with_payload("uid-1", json!({"spec":{"replicas":5}}));
    let outcome = verify(
        &connection,
        &intent,
        Some(&payload),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::Verification::TargetReplaced);
}

#[tokio::test]
async fn verify_is_unknown_when_cancelled_never_treated_as_a_commit_failure() {
    use sauron::kube::mutation::verify;
    let server =
        Server::new(|_| panic!("a pre-cancelled verification must never reach the transport"))
            .await;
    let connection = connection(server.client());
    let (intent, payload) = modify_intent_with_payload("uid-1", json!({"spec":{"replicas":5}}));
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = verify(&connection, &intent, Some(&payload), &cancel).await;
    assert_eq!(outcome, sauron::mutation::Verification::Unknown);
}

#[tokio::test]
async fn verify_delete_reports_deletion_in_progress_when_timestamp_present() {
    use sauron::kube::mutation::verify;
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1","deletionTimestamp":"2026-09-18T00:00:00Z"}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Delete);
    let outcome = verify(&connection, &intent, None, &CancellationToken::new()).await;
    assert_eq!(outcome, sauron::mutation::Verification::DeletionInProgress);
}

#[tokio::test]
async fn verify_delete_reports_observed_gone_on_a_fresh_notfound() {
    use sauron::kube::mutation::verify;
    let server = Server::new(|_| (404, String::new())).await;
    let connection = connection(server.client());
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Delete);
    let outcome = verify(&connection, &intent, None, &CancellationToken::new()).await;
    assert_eq!(outcome, sauron::mutation::Verification::ObservedGone);
}

#[tokio::test]
async fn verify_delete_still_present_with_no_deletion_timestamp_is_pending_not_a_problem() {
    use sauron::kube::mutation::verify;
    let server = Server::new(|_| {
        (
            200,
            json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Delete);
    let outcome = verify(&connection, &intent, None, &CancellationToken::new()).await;
    assert_eq!(outcome, sauron::mutation::Verification::Pending);
}

#[tokio::test]
async fn commit_success_remains_success_even_when_verification_is_later_unknown() {
    use sauron::kube::mutation::{commit, verify};
    use sauron::mutation::Confirmation;
    let server = Server::with_request(|request| {
        if request.starts_with("GET") {
            (
                200,
                json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1","resourceVersion":"5"}}).to_string(),
            )
        } else {
            assert!(request.starts_with("PATCH"), "unexpected method: {request}");
            (200, json!({"apiVersion":"v1","kind":"ConfigMap","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1"}}).to_string())
        }
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
    let payload = json!({"metadata":{"annotations":{"m7-proof":"nonce"}}});
    let confirmation = Confirmation {
        request_id: intent.request_id,
        scope: intent.target.scope.clone(),
        effect: intent.effect,
        payload_sha256: intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &intent,
        Some(&confirmation),
        Some(payload.clone()),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Committed);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let verification = verify(&connection, &intent, Some(&payload), &cancel).await;
    assert_eq!(verification, sauron::mutation::Verification::Unknown);
    assert_eq!(
        outcome,
        sauron::mutation::MutationOutcome::Committed,
        "a later-unknown verification must never retroactively change the commit outcome"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_conflict_and_forbidden_are_explicit_never_forced() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::Confirmation;
    for (status, expected) in [
        (409, sauron::mutation::MutationOutcome::Conflict),
        (403, sauron::mutation::MutationOutcome::Forbidden),
    ] {
        let server = Server::with_request(move |request| {
            if request.starts_with("GET") {
                (
                    200,
                    json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1","resourceVersion":"5"}}).to_string(),
                )
            } else {
                (status, json!({"kind":"Status","apiVersion":"v1","status":"Failure","code":status}).to_string())
            }
        })
        .await;
        let connection = connection(server.client());
        let (journal, dir) = test_journal();
        let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
        let confirmation = Confirmation {
            request_id: intent.request_id,
            scope: intent.target.scope.clone(),
            effect: intent.effect,
            payload_sha256: intent.payload_sha256.clone(),
            requirement: sauron::mutation::ConfirmationRequirement::Standard,
        };
        let outcome = commit(
            &connection,
            &verified_policy_context(),
            1,
            &intent,
            Some(&confirmation),
            Some(json!({"metadata":{"annotations":{"m7-proof":"nonce"}}})),
            &journal,
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(outcome, expected);
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[tokio::test]
async fn mutation_epoch_change_before_commit_is_rejected_as_replaced() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::Confirmation;
    let server = Server::new(|_| panic!("an epoch mismatch must never reach the transport")).await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
    let confirmation = Confirmation {
        request_id: intent.request_id,
        scope: intent.target.scope.clone(),
        effect: intent.effect,
        payload_sha256: intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Standard,
    };
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        99, // current epoch differs from intent.target.scope.epoch == 1
        &intent,
        Some(&confirmation),
        Some(json!({"metadata":{"annotations":{"m7-proof":"nonce"}}})),
        &journal,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::TargetReplaced);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn mutation_cancellation_before_commit_sends_zero_writes() {
    use sauron::kube::mutation::commit;
    use sauron::mutation::Confirmation;
    let server = Server::with_request(|request| {
        assert!(request.starts_with("GET"), "cancellation before revalidation must never patch: {request}");
        (
            200,
            json!({"apiVersion":"meta.k8s.io/v1","kind":"PartialObjectMetadata","metadata":{"namespace":"sauron-m7","name":"m7-target","uid":"uid-1","resourceVersion":"5"}}).to_string(),
        )
    })
    .await;
    let connection = connection(server.client());
    let (journal, dir) = test_journal();
    let intent = mutation_intent("uid-1", sauron::mutation::MutationEffect::Modify);
    let confirmation = Confirmation {
        request_id: intent.request_id,
        scope: intent.target.scope.clone(),
        effect: intent.effect,
        payload_sha256: intent.payload_sha256.clone(),
        requirement: sauron::mutation::ConfirmationRequirement::Standard,
    };
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = commit(
        &connection,
        &verified_policy_context(),
        1,
        &intent,
        Some(&confirmation),
        Some(json!({"metadata":{"annotations":{"m7-proof":"nonce"}}})),
        &journal,
        &cancel,
    )
    .await;
    assert_eq!(outcome, sauron::mutation::MutationOutcome::Cancelled);
    std::fs::remove_dir_all(&dir).ok();
}

fn resource() -> Resource {
    Resource {
        api: ApiResource {
            group: "".into(),
            version: "v1".into(),
            api_version: "v1".into(),
            kind: "Pod".into(),
            plural: "pods".into(),
        },
        namespaced: true,
        short_names: vec!["po".into()],
        verbs: vec!["list".into(), "watch".into()],
    }
}

#[tokio::test]
async fn forward_rejects_forbidden_gone_replaced_and_terminating_targets() {
    use sauron::kube::forward::{self, ErrorKind, Ports, Progress};
    let target = Arc::new(Object::new(
        json!({"kind":"Pod","apiVersion":"v1","metadata":{"name":"p","namespace":"default","uid":"old"}}),
    ));
    for (code, body, expected) in [
        (
            403,
            json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"Forbidden","message":"secret credential sentinel","code":403}),
            ErrorKind::PermissionDenied,
        ),
        (
            404,
            json!({"kind":"Status","apiVersion":"v1","status":"Failure","reason":"NotFound","code":404}),
            ErrorKind::TargetGone,
        ),
        (
            200,
            json!({"kind":"Pod","apiVersion":"v1","metadata":{"uid":"new"}}),
            ErrorKind::TargetReplaced,
        ),
        (
            200,
            json!({"kind":"Pod","apiVersion":"v1","metadata":{"uid":"old","deletionTimestamp":"2026-09-16T10:00:00Z"}}),
            ErrorKind::TargetTerminating,
        ),
        (
            200,
            json!({"kind":"Pod","apiVersion":"v1","metadata":{"uid":"old"},"status":{"phase":"Failed"}}),
            ErrorKind::TargetEnded,
        ),
    ] {
        let server = Server::new(move |_| (code, body.to_string())).await;
        let mut connection = connection(server.client());
        connection.settings.readonly = false;
        let (tx, rx) = tokio::sync::watch::channel(Progress::default());
        let error = forward::run(
            connection,
            target.clone(),
            Ports {
                local: 0,
                remote: 80,
            },
            tx,
        )
        .await
        .expect_err("reject");
        assert_eq!(error.kind, expected);
        assert!(!error.message.contains("sentinel"));
        assert!(rx.borrow().local.is_none());
    }
}

#[tokio::test]
async fn forward_monitor_closes_listener_on_replacement_or_unverifiable_identity() {
    use sauron::kube::forward::{self, ErrorKind, Ports, Progress};
    use std::sync::atomic::{AtomicBool, Ordering};
    for (code, changed_body, expected) in [
        (
            200,
            json!({"kind":"Pod","apiVersion":"v1","metadata":{"uid":"new"}}),
            ErrorKind::TargetReplaced,
        ),
        (
            403,
            json!({"kind":"Status","apiVersion":"v1","reason":"Forbidden","code":403}),
            ErrorKind::PermissionDenied,
        ),
    ] {
        let changed = Arc::new(AtomicBool::new(false));
        let flag = changed.clone();
        let pod = json!({"kind":"Pod","apiVersion":"v1","metadata":{"name":"p","namespace":"default","uid":"old"}});
        let initial = pod.to_string();
        let server = Server::new(move |path| {
            assert!(!path.contains("portforward"));
            if flag.load(Ordering::SeqCst) {
                (code, changed_body.to_string())
            } else {
                (200, initial.clone())
            }
        })
        .await;
        let mut connection = connection(server.client());
        connection.settings.readonly = false;
        let (tx, mut rx) = tokio::sync::watch::channel(Progress::default());
        let worker = forward::run(
            connection,
            Arc::new(Object::new(pod)),
            Ports {
                local: 0,
                remote: 80,
            },
            tx,
        );
        let observer = async {
            rx.changed().await.expect("listening");
            let address = rx.borrow().local.expect("bound");
            changed.store(true, Ordering::SeqCst);
            address
        };
        let (result, address) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(worker, observer)
        })
        .await
        .expect("monitor deadline");
        assert_eq!(result.expect_err("must stop").kind, expected);
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
        let _reusable = TcpListener::bind(address).await.expect("listener released");
    }
}

#[tokio::test]
async fn forward_auto_loopback_conflict_cancellation_and_readonly() {
    use sauron::{
        app::session::{Kind, Scope, Sessions},
        kube::forward::{self, ErrorKind, Ports, Progress},
    };
    let pod = json!({"kind":"Pod","apiVersion":"v1","metadata":{"name":"p","namespace":"default","uid":"old"}});
    let response = pod.to_string();
    let server = Server::new(move |path| {
        assert!(!path.contains("portforward"));
        (200, response.clone())
    })
    .await;
    let target = Arc::new(Object::new(pod));
    let readonly = connection(server.client());
    let (tx, rx) = tokio::sync::watch::channel(Progress::default());
    assert_eq!(
        forward::run(
            readonly,
            target.clone(),
            Ports {
                local: 0,
                remote: 80
            },
            tx
        )
        .await
        .expect_err("readonly")
        .kind,
        ErrorKind::PermissionDenied
    );
    assert!(rx.borrow().local.is_none());
    let mut connection = connection(server.client());
    connection.settings.readonly = false;
    let mut sessions = Sessions::default();
    let scope = Scope {
        epoch: 0,
        request: 0,
        context: "fake".into(),
        cluster: "fake".into(),
        resource: "v1/pods".into(),
        namespace: "default".into(),
        name: "p".into(),
        uid: "old".into(),
    };
    let (tx, mut rx) = tokio::sync::watch::channel(Progress::default());
    let c = connection.clone();
    let object = target.clone();
    sessions
        .spawn(
            Kind::PortForward,
            scope,
            CancellationToken::new(),
            |_| async move {
                let _ = forward::run(
                    c,
                    object,
                    Ports {
                        local: 0,
                        remote: 80,
                    },
                    tx,
                )
                .await;
                sauron::app::session::Outcome::Completed
            },
        )
        .expect("spawn");
    tokio::time::timeout(Duration::from_secs(2), rx.changed())
        .await
        .expect("bounded")
        .expect("listening");
    let address = rx.borrow().local.expect("bound");
    assert_eq!(address.ip().to_string(), "127.0.0.1");
    assert_ne!(address.port(), 0);
    let (tx, _) = tokio::sync::watch::channel(Progress::default());
    assert_eq!(
        forward::run(
            connection,
            target,
            Ports {
                local: address.port(),
                remote: 80
            },
            tx
        )
        .await
        .expect_err("conflict")
        .kind,
        ErrorKind::PortInUse
    );
    sessions.shutdown().await;
    assert_eq!(sessions.active_count(), 0);
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    let _listener = TcpListener::bind(address)
        .await
        .expect("port immediately reusable");
}
fn crd_definitions_resource() -> Resource {
    Resource {
        api: ApiResource {
            group: "apiextensions.k8s.io".into(),
            version: "v1".into(),
            api_version: "apiextensions.k8s.io/v1".into(),
            kind: "CustomResourceDefinition".into(),
            plural: "customresourcedefinitions".into(),
        },
        namespaced: false,
        short_names: vec!["crd".into()],
        verbs: vec!["list".into(), "get".into()],
    }
}
fn widget_resource() -> Resource {
    Resource {
        api: ApiResource {
            group: "example.com".into(),
            version: "v1".into(),
            api_version: "example.com/v1".into(),
            kind: "Widget".into(),
            plural: "widgets".into(),
        },
        namespaced: true,
        short_names: vec![],
        verbs: vec!["list".into(), "watch".into()],
    }
}
fn connection_with(client: Client, resources: Vec<Resource>) -> Connection {
    Connection {
        client,
        context: "fake".into(),
        cluster: "fake".into(),
        namespace: "default".into(),
        contexts: vec![],
        catalog: Catalog {
            resources,
            warnings: vec![],
        },
        settings: Settings::default(),
        version: "fake".into(),
    }
}
fn connection(client: Client) -> Connection {
    Connection {
        client,
        context: "fake".into(),
        cluster: "fake".into(),
        namespace: "default".into(),
        contexts: vec![],
        catalog: Catalog {
            resources: vec![resource()],
            warnings: vec![],
        },
        settings: Settings::default(),
        version: "fake".into(),
    }
}
fn pod(uid: &str, version: &str, name: &str) -> Value {
    json!({"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"default","name":name,"uid":uid,"resourceVersion":version},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Pending"}})
}

#[tokio::test]
async fn paged_list_watch_replacement_and_cancel_with_full_queue() {
    let server=Server::new(|path|{
        if path.contains("watch=true") || path.contains("watch=1") {
            let events=[json!({"type":"MODIFIED","object":pod("new","3","a")}),json!({"type":"DELETED","object":pod("old","2","a")})];
            (200,events.iter().map(|e|format!("{e}\n")).collect())
        }else if path.contains("continue=next") {
            (200,json!({"apiVersion":"v1","kind":"PodList","metadata":{"resourceVersion":"2"},"items":[pod("b","2","b")]}).to_string())
        }else {
            (200,json!({"apiVersion":"v1","kind":"PodList","metadata":{"resourceVersion":"2","continue":"next"},"items":[pod("old","2","a")]}).to_string())
        }
    }).await;
    let (tx, mut rx) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(watch::run(
        connection(server.client()),
        resource(),
        Query {
            resource: "pods".into(),
            namespace: Some("default".into()),
            ..Default::default()
        },
        7,
        tx,
        cancel.clone(),
    ));
    let mut store = Store::new(10, 1_048_576);
    let mut saw_ready = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("watch event");
            assert_eq!(event.epoch, 7);
            match event.payload {
                Payload::Begin => store.begin(),
                Payload::Apply(o, init) => store.apply(o, init),
                Payload::Ready => {
                    store.finish();
                    saw_ready = true;
                    assert_eq!(store.objects.len(), 2);
                }
                Payload::Delete(o) => {
                    store.delete(&o);
                    break;
                }
                Payload::WatchError(e) => panic!("unexpected watch error: {e}"),
                _ => {}
            }
        }
    })
    .await
    .expect("bounded watch");
    assert!(saw_ready);
    assert_eq!(store.objects["default/a"].uid, "new");
    // Stop reading so the bounded channel can fill. Cancellation must interrupt send.
    tokio::task::yield_now().await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("cancelled promptly")
        .expect("task joined");
}

#[tokio::test]
async fn forbidden_is_visible_and_error_body_cannot_leak() {
    let server=Server::new(|_|(403,json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":"Forbidden","message":"Bearer TOP_SECRET","code":403}).to_string())).await;
    let (tx, mut rx) = mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(watch::run(
        connection(server.client()),
        resource(),
        Query::default(),
        1,
        tx,
        cancel.clone(),
    ));
    let error = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Payload::WatchError(e) = rx.recv().await.expect("event").payload {
                break e;
            }
        }
    })
    .await
    .expect("error delivered");
    assert!(error.contains("Forbidden"));
    assert!(error.contains("pods"));
    assert!(!error.contains("TOP_SECRET"));
    cancel.cancel();
    task.await.expect("join");
}

#[tokio::test]
async fn discovery_preserves_core_when_extension_is_forbidden() {
    let server=Server::new(|path|match path {
        "/api/v1"=>(200,json!({"apiVersion":"v1","kind":"APIResourceList","groupVersion":"v1","resources":[{"name":"pods","singularName":"pod","namespaced":true,"kind":"Pod","verbs":["get","list","watch"]}]}).to_string()),
        "/apis"=>(200,json!({"apiVersion":"v1","kind":"APIGroupList","groups":[{"name":"broken.test","versions":[{"groupVersion":"broken.test/v1","version":"v1"}],"preferredVersion":{"groupVersion":"broken.test/v1","version":"v1"}}]}).to_string()),
        _=>(403,json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":"Forbidden","message":"test","code":403}).to_string()),
    }).await;
    let catalog = sauron::kube::discovery::discover(&server.client(), Duration::from_secs(2))
        .await
        .expect("partial discovery");
    assert_eq!(catalog.warnings.len(), 1);
    assert!(catalog.resolve("pods", &BTreeMap::new()).is_ok());
}

#[tokio::test]
async fn server_selectors_are_preserved_on_list_and_watch_with_local_filter() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = seen.clone();
    let server = Server::new(move |path| {
        observed.lock().expect("paths").push(path.to_owned());
        assert!(path.contains("labelSelector=app%3Dapi"), "{path}");
        assert!(path.contains("fieldSelector=status.phase%3DRunning"), "{path}");
        assert!(!path.contains("restarts"), "local AST must never be pushed to server");
        if path.contains("watch=true") || path.contains("watch=1") {
            (200, format!("{}\n", json!({"type":"MODIFIED","object":pod("a","2","api")})))
        } else {
            (200, json!({"apiVersion":"v1","kind":"PodList","metadata":{"resourceVersion":"1"},"items":[pod("a","1","api")]}).to_string())
        }
    }).await;
    let (tx, mut rx) = mpsc::channel(4);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(watch::run(
        connection(server.client()),
        resource(),
        Query {
            resource: "v1/pods".into(),
            namespace: Some("default".into()),
            labels: Some("app=api".into()),
            fields: Some("status.phase=Running".into()),
        },
        1,
        tx,
        cancel.clone(),
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match rx.recv().await.expect("event").payload {
                Payload::Apply(object, false) => {
                    assert_eq!(
                        sauron::filters::Expr::parse("name=api AND restarts>1")
                            .expect("filter")
                            .evaluate(&object, chrono::Utc::now(), None),
                        sauron::filters::Truth::Unknown
                    );
                    break;
                }
                Payload::WatchError(error) => panic!("{error}"),
                _ => {}
            }
        }
    })
    .await
    .expect("watch delivered");
    cancel.cancel();
    task.await.expect("joined");
    assert!(seen.lock().expect("paths").len() >= 2);
}

fn selected_pod() -> Object {
    Object::new(pod("u1", "1", "api"))
}

fn statefulset_resource() -> Resource {
    Resource {
        api: ApiResource {
            group: "apps".into(),
            version: "v1".into(),
            api_version: "apps/v1".into(),
            kind: "StatefulSet".into(),
            plural: "statefulsets".into(),
        },
        namespaced: true,
        short_names: vec![],
        verbs: vec!["list".into(), "watch".into()],
    }
}
fn selected_statefulset() -> Object {
    Object::new(
        json!({"apiVersion":"apps/v1","kind":"StatefulSet","metadata":{"namespace":"default","name":"s","uid":"sts-uid","resourceVersion":"1"},"spec":{"replicas":1},"status":{"observedGeneration":0,"readyReplicas":1,"updatedReplicas":1}}),
    )
}

#[tokio::test]
async fn explain_correlates_only_verified_owned_pods_not_by_name_or_label() {
    let server = Server::new(|path| {
        if path.contains("/events") {
            (200, json!({"items":[]}).to_string())
        } else if path.contains("/namespaces/default/pods") {
            (200, json!({"apiVersion":"v1","kind":"PodList","metadata":{},"items":[
                {"apiVersion":"v1","kind":"Pod","metadata":{"name":"s-0","namespace":"default","uid":"owned-uid","ownerReferences":[{"uid":"sts-uid","kind":"StatefulSet","name":"s","apiVersion":"apps/v1","controller":true}]},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","containerStatuses":[{"name":"c","ready":false,"state":{"waiting":{"reason":"CrashLoopBackOff"}}}]}},
                {"apiVersion":"v1","kind":"Pod","metadata":{"name":"s-imposter","namespace":"default","uid":"other-uid"},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","containerStatuses":[{"name":"c","ready":false,"state":{"waiting":{"reason":"CrashLoopBackOff"}}}]}}
            ]}).to_string())
        } else {
            (200, serde_json::to_string(&selected_statefulset().value).expect("json"))
        }
    })
    .await;
    let text = evidence::document(
        &connection(server.client()),
        &statefulset_resource(),
        &selected_statefulset(),
        Action::Explain,
        false,
        None,
    )
    .await
    .expect("explain document");
    assert!(text.contains("s-0"), "{text}");
    assert!(text.contains("CrashLoopBackOff"), "{text}");
    assert!(
        !text.contains("s-imposter"),
        "a same-namespace Pod without a matching ownerReference must never be treated as owned: {text}"
    );
}

#[tokio::test]
async fn explain_partial_when_owned_pods_are_forbidden_but_still_reports_own_health() {
    let server = Server::new(|path| {
        if path.contains("/events") {
            (200, json!({"items":[]}).to_string())
        } else if path.contains("/namespaces/default/pods") {
            (403, json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":"Forbidden","message":"Bearer TOP_SECRET","code":403}).to_string())
        } else {
            (200, json!({"apiVersion":"apps/v1","kind":"StatefulSet","metadata":{"namespace":"default","name":"s","uid":"sts-uid","resourceVersion":"1"},"spec":{"replicas":2},"status":{"observedGeneration":0,"readyReplicas":1,"updatedReplicas":1}}).to_string())
        }
    })
    .await;
    let text = evidence::document(
        &connection(server.client()),
        &statefulset_resource(),
        &selected_statefulset(),
        Action::Explain,
        false,
        None,
    )
    .await
    .expect("explain still succeeds when owned Pods are forbidden");
    assert!(text.contains("PARTIAL EVIDENCE"), "{text}");
    assert!(text.contains("Forbidden"), "{text}");
    assert!(!text.contains("TOP_SECRET"), "{text}");
    assert!(text.contains("Progressing"), "{text}");
}

#[tokio::test]
async fn events_403_is_visible_and_never_leaks_secrets() {
    let server = Server::new(|path| {
        if path.contains("/events") {
            (403,json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":"Forbidden","message":"Bearer TOP_SECRET","code":403}).to_string())
        } else {
            (200, pod("u1", "1", "api").to_string())
        }
    })
    .await;
    let text = evidence::document(
        &connection(server.client()),
        &resource(),
        &selected_pod(),
        Action::Events,
        false,
        None,
    )
    .await
    .expect("Events document still succeeds when related Events are forbidden");
    assert!(text.contains("Forbidden"), "{text}");
    assert!(!text.contains("TOP_SECRET"), "{text}");
    assert!(text.contains("No Events returned"), "{text}");
}

#[tokio::test]
async fn events_partial_continue_token_is_reported() {
    let server = Server::new(|path| {
        if path.contains("/events") {
            (200, json!({"apiVersion":"v1","kind":"EventList","metadata":{"continue":"more"},"items":[
                {"type":"Normal","reason":"Scheduled","message":"ok","count":1,"lastTimestamp":"2026-09-15T10:00:00Z","involvedObject":{"uid":"u1"}}
            ]}).to_string())
        } else {
            (200, pod("u1", "1", "api").to_string())
        }
    })
    .await;
    let text = evidence::document(
        &connection(server.client()),
        &resource(),
        &selected_pod(),
        Action::Events,
        false,
        None,
    )
    .await
    .expect("document");
    assert!(
        text.contains("PARTIAL: related Events limited to 200"),
        "{text}"
    );
}

#[tokio::test]
async fn warning_only_filters_events_and_a_null_timestamp_field_falls_through() {
    let server = Server::new(|path| {
        if path.contains("/events") {
            (
                200,
                json!({"apiVersion":"v1","kind":"EventList","metadata":{},"items":[
                    // A present-but-null lastTimestamp (very common on real core v1 Events)
                    // must not win over the real eventTime later in the fallback chain.
                    {"type":"Normal","reason":"Pulled","message":"image pulled","count":1,
                     "lastTimestamp":null,"eventTime":"2026-09-15T10:05:00.000000Z",
                     "involvedObject":{"uid":"u1"}},
                    {"type":"Warning","reason":"BackOff","message":"crash looping","count":9,
                     "lastTimestamp":"2026-09-15T10:06:00Z",
                     "involvedObject":{"uid":"u1","fieldPath":"spec.containers{worker}"}},
                ]})
                .to_string(),
            )
        } else {
            (200, pod("u1", "1", "api").to_string())
        }
    })
    .await;
    let all = evidence::document(
        &connection(server.client()),
        &resource(),
        &selected_pod(),
        Action::Events,
        false,
        None,
    )
    .await
    .expect("document");
    assert!(all.contains("Pulled") && all.contains("BackOff"), "{all}");
    assert!(
        all.contains("2026-09-15T10:05:00"),
        "null lastTimestamp must fall through to eventTime: {all}"
    );
    assert!(
        !all.contains("null "),
        "a present-but-null field must never be displayed as a timestamp: {all}"
    );
    assert!(
        all.contains("(spec.containers{worker})"),
        "fieldPath should be shown: {all}"
    );

    let warnings_only = evidence::document(
        &connection(server.client()),
        &resource(),
        &selected_pod(),
        Action::Events,
        true,
        None,
    )
    .await
    .expect("document");
    assert!(!warnings_only.contains("Pulled"), "{warnings_only}");
    assert!(warnings_only.contains("BackOff"), "{warnings_only}");
    assert!(warnings_only.contains("(Warning only)"), "{warnings_only}");
}

#[tokio::test]
async fn printer_columns_are_fetched_live_from_the_crd_spec() {
    let server = Server::new(|path| {
        if path.contains("/customresourcedefinitions/widgets.example.com") {
            (200, json!({
                "apiVersion":"apiextensions.k8s.io/v1","kind":"CustomResourceDefinition",
                "metadata":{"name":"widgets.example.com"},
                "spec":{"group":"example.com","versions":[
                    {"name":"v1","additionalPrinterColumns":[
                        {"name":"Color","type":"string","jsonPath":".spec.color"},
                        {"name":"Replicas","type":"integer","jsonPath":".spec.replicas","priority":1}
                    ]}
                ]}
            }).to_string())
        } else {
            (404, json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":"NotFound","code":404}).to_string())
        }
    })
    .await;
    let connection = connection_with(
        server.client(),
        vec![crd_definitions_resource(), widget_resource()],
    );
    let columns = printer::fetch(&connection, &widget_resource())
        .await
        .expect("fetch");
    assert_eq!(columns.len(), 2, "{columns:?}");
    assert_eq!(columns[0].name, "Color");
    assert_eq!(columns[0].field.key, "field:/spec/color");
    assert_eq!(columns[1].name, "Replicas");
    assert_eq!(columns[1].priority, 1);
}

#[tokio::test]
async fn printer_columns_are_empty_not_an_error_for_a_non_crd_resource() {
    let server = Server::new(|_| {
        (404, json!({"apiVersion":"v1","kind":"Status","status":"Failure","reason":"NotFound","code":404}).to_string())
    })
    .await;
    let connection = connection_with(
        server.client(),
        vec![crd_definitions_resource(), widget_resource()],
    );
    let columns = printer::fetch(&connection, &widget_resource())
        .await
        .expect("a missing CRD is not an error, just no enrichment");
    assert!(columns.is_empty());
}

#[tokio::test]
async fn printer_columns_skip_the_network_entirely_for_core_resources() {
    let server =
        Server::new(|_| panic!("a core (empty-group) resource must never be looked up as a CRD"))
            .await;
    let connection = connection_with(
        server.client(),
        vec![resource(), crd_definitions_resource()],
    );
    let columns = printer::fetch(&connection, &resource())
        .await
        .expect("fetch");
    assert!(columns.is_empty());
}
