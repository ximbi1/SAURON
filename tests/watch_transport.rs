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
                let request = String::from_utf8_lossy(&data);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (code, body) = handler(path);
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
        Client::try_from(Config::new(self.url.parse().expect("URI"))).expect("client")
    }
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
