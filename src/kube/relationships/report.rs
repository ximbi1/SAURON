//! One selected-object operation; no permanent graph cache or spawned workers.
use super::{candidates, children, fetch_target};
use crate::{
    evidence::Unknown,
    graph::{
        Edge, Graph, Identity, Limits, Provenance,
        references::{Target, extract},
    },
    kube::{Connection, discovery::Resource},
    resources::{Object, health::Health},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub struct Node {
    pub resource: Resource,
    pub health: Health,
}
pub struct Issue {
    pub source: String,
    pub reason: Unknown,
}
pub struct Report {
    pub root: Identity,
    pub graph: Graph,
    pub nodes: BTreeMap<Identity, Node>,
    pub issues: Vec<Issue>,
    pub requests: usize,
    pub candidates: usize,
}
impl Report {
    fn issue(&mut self, source: impl Into<String>, reason: Unknown) {
        if self.issues.len() == 63 {
            self.issues.push(Issue {
                source: "additional issues omitted at 64-entry bound".into(),
                reason: Unknown::Partial,
            });
        } else if self.issues.len() < 63 {
            self.issues.push(Issue {
                source: source.into(),
                reason,
            });
        }
    }
    fn available(&mut self, started: Instant) -> bool {
        if self.requests >= 48 || started.elapsed() >= Duration::from_secs(30) {
            self.issue("operation request/time budget", Unknown::Partial);
            false
        } else {
            true
        }
    }

    fn link(
        &mut self,
        root: &Identity,
        resource: &Resource,
        object: &Object,
        provenance: Provenance,
        path: &str,
        reverse: bool,
    ) {
        let id = match Identity::observed(root.scope, resource, object) {
            Ok(id) => id,
            Err(reason) => {
                self.issue("candidate identity", reason);
                return;
            }
        };
        let (from, to) = if reverse {
            (id.clone(), root.clone())
        } else {
            (root.clone(), id.clone())
        };
        match self.graph.insert(
            Edge {
                from,
                to,
                provenance,
            },
            path,
        ) {
            Ok(()) => {
                self.nodes.entry(id).or_insert_with(|| Node {
                    resource: resource.clone(),
                    health: object.health.clone(),
                });
            }
            Err(reason) => self.issue("candidate graph bound", reason),
        }
    }
}

fn self_target(resource: &Resource, object: &Object) -> Target {
    Target {
        api_version: resource.api.api_version.clone(),
        kind: resource.api.kind.clone(),
        namespace: object.namespace.clone(),
        name: object.name.clone(),
        expected_uid: Some(object.uid.clone()),
        provenance: Provenance::ExplicitReference,
    }
}

/// Re-read and UID-pin the root both before and after collection. A changed root
/// resourceVersion produces Stale (caller may Refresh), never a mixed-current graph.
pub async fn adjacent(
    connection: &Connection,
    scope: u64,
    resource: &Resource,
    selected: &Object,
    cancel: &CancellationToken,
) -> Result<Report, Unknown> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Unknown::Stale),
        result = tokio::time::timeout(Duration::from_secs(30), collect(connection, scope, resource, selected, cancel)) => result.unwrap_or(Err(Unknown::TimedOut)),
    }
}

async fn collect(
    connection: &Connection,
    scope: u64,
    resource: &Resource,
    selected: &Object,
    cancel: &CancellationToken,
) -> Result<Report, Unknown> {
    let started = Instant::now();
    let target = self_target(resource, selected);
    let (_, root, fresh) = fetch_target(connection, scope, selected, &target, cancel).await?;
    let mut report = Report {
        root: root.clone(),
        graph: Graph::new(scope, Limits::default()),
        nodes: BTreeMap::from([(
            root.clone(),
            Node {
                resource: resource.clone(),
                health: fresh.health.clone(),
            },
        )]),
        issues: vec![],
        requests: 1,
        candidates: 0,
    };
    let references = extract(&fresh);
    if references.partial {
        report.issue("reference extraction bound", Unknown::Partial);
    }
    if references.malformed {
        report.issue("reference fields", Unknown::Malformed);
    }
    for (reference, paths) in references.targets {
        if !report.available(started) {
            break;
        }
        if cancel.is_cancelled() {
            return Err(Unknown::Stale);
        }
        report.requests += 1;
        match fetch_target(connection, scope, &fresh, &reference, cancel).await {
            Ok((resource, id, object)) => {
                let edge = Edge {
                    from: root.clone(),
                    to: id.clone(),
                    provenance: reference.provenance,
                };
                let mut inserted = false;
                for path in paths {
                    match report.graph.insert(edge.clone(), &path) {
                        Ok(()) => inserted = true,
                        Err(reason) => report.issue("graph insertion", reason),
                    }
                }
                if inserted {
                    report.nodes.insert(
                        id,
                        Node {
                            resource,
                            health: object.health,
                        },
                    );
                }
            }
            Err(reason) => report.issue(
                format!(
                    "{} {}/{}",
                    reference.kind, reference.namespace, reference.name
                ),
                reason,
            ),
        }
    }
    // Explicit candidate set, not every discovered kind. Same-kind CRD ownership
    // is supported; unscanned kinds must remain visible as incomplete coverage.
    let mut scans = vec![resource.clone()];
    for id in ["v1/pods", "apps/v1/replicasets", "batch/v1/jobs"] {
        if let Some(r) = connection.catalog.resources.iter().find(|r| r.id() == id)
            && !scans.iter().any(|s| s.id() == id)
        {
            scans.push(r.clone());
        }
    }
    report.issue(
        "reverse ownership limited to selected kind, Pods, ReplicaSets and Jobs",
        Unknown::Partial,
    );
    for child_resource in scans {
        if !report.available(started) {
            break;
        }
        if cancel.is_cancelled() {
            return Err(Unknown::Stale);
        }
        report.requests += 1;
        match children(connection, &fresh, &child_resource, cancel).await {
            Ok((children, partial, inspected)) => {
                report.candidates += inspected;
                if partial {
                    report.issue(
                        format!("{} child scan", child_resource.id()),
                        Unknown::Partial,
                    );
                }
                for child in children {
                    let id = match Identity::observed(scope, &child_resource, &child) {
                        Ok(id) => id,
                        Err(reason) => {
                            report.issue("child identity", reason);
                            continue;
                        }
                    };
                    for (owner, paths) in extract(&child).targets {
                        if owner.provenance != Provenance::OwnerReference
                            || owner.expected_uid.as_ref() != Some(&root.uid)
                        {
                            continue;
                        }
                        for path in paths {
                            match report.graph.insert(
                                Edge {
                                    from: id.clone(),
                                    to: root.clone(),
                                    provenance: Provenance::OwnerReference,
                                },
                                &path,
                            ) {
                                Ok(()) => {
                                    report.nodes.insert(
                                        id.clone(),
                                        Node {
                                            resource: child_resource.clone(),
                                            health: child.health.clone(),
                                        },
                                    );
                                }
                                Err(reason) => report.issue("child graph bound", reason),
                            }
                        }
                    }
                }
            }
            Err(reason) => report.issue(format!("{} child scan", child_resource.id()), reason),
        }
    }
    network(connection, &fresh, &mut report, started, cancel).await?;
    reverse_references(connection, &fresh, &mut report, started, cancel).await?;
    report.requests += 1; // Reserved final validation is mandatory even at scan limit.
    let (_, current, object) = fetch_target(connection, scope, &fresh, &target, cancel).await?;
    if current != root {
        return Err(Unknown::TargetReplaced);
    }
    if object.version != fresh.version {
        return Err(Unknown::Stale);
    }
    Ok(report)
}

async fn network(
    connection: &Connection,
    fresh: &Object,
    report: &mut Report,
    started: Instant,
    cancel: &CancellationToken,
) -> Result<(), Unknown> {
    use crate::graph::references::network::selects;
    let scans: &[(&str, bool)] = match (fresh.api_version.as_str(), fresh.kind.as_str()) {
        ("v1", "Pod") => &[("v1/services", false)],
        ("v1", "Service") => &[
            ("v1/pods", false),
            ("discovery.k8s.io/v1/endpointslices", true),
        ],
        _ => &[],
    };
    for (gvr, metadata) in scans {
        if !report.available(started) {
            break;
        }
        if cancel.is_cancelled() {
            return Err(Unknown::Stale);
        }
        let Some(resource) = connection.catalog.resources.iter().find(|r| r.id() == *gvr) else {
            report.issue(*gvr, Unknown::Unavailable);
            continue;
        };
        report.requests += 1;
        match candidates(connection, resource, &fresh.namespace, *metadata, cancel).await {
            Ok((objects, partial)) => {
                report.candidates += objects.len();
                if partial {
                    report.issue(format!("{gvr} candidate page"), Unknown::Partial);
                }
                for object in objects {
                    let matched = if *metadata {
                        Ok(object
                            .value
                            .pointer("/metadata/labels/kubernetes.io~1service-name")
                            .and_then(serde_json::Value::as_str)
                            == Some(fresh.name.as_str()))
                    } else if fresh.kind == "Service" {
                        selects(fresh, &object)
                    } else {
                        selects(&object, fresh)
                    };
                    match matched {
                        Ok(true) => {
                            if *metadata {
                                let owners = extract(&object);
                                if owners.targets.keys().any(|r| {
                                    r.kind == "Service"
                                        && r.api_version == "v1"
                                        && r.name == fresh.name
                                        && r.expected_uid.as_ref().is_some_and(|u| u != &fresh.uid)
                                }) {
                                    report.issue(
                                        "EndpointSlice Service owner",
                                        Unknown::TargetReplaced,
                                    );
                                    continue;
                                }
                                report.link(
                                    &report.root.clone(),
                                    resource,
                                    &object,
                                    Provenance::ExplicitReference,
                                    "/metadata/labels/kubernetes.io~1service-name",
                                    true,
                                );
                            } else {
                                let selector = if fresh.kind == "Service" {
                                    &fresh.value["spec"]["selector"]
                                } else {
                                    &object.value["spec"]["selector"]
                                };
                                report.link(
                                    &report.root.clone(),
                                    resource,
                                    &object,
                                    Provenance::SelectorMatch,
                                    &format!("/spec/selector = {selector}"),
                                    fresh.kind == "Pod",
                                );
                            }
                        }
                        Ok(false) => {}
                        Err(reason) => report.issue(format!("{gvr} selector"), reason),
                    }
                }
            }
            Err(reason) => report.issue(format!("{gvr} candidate scan"), reason),
        }
    }
    Ok(())
}

/// Explicit bounded candidate list, not arbitrary CRD scanning: which built-in
/// workload kinds may declaratively mount/reference this ConfigMap/Secret/
/// ServiceAccount/PVC. A denied kind never removes evidence already found in
/// another one.
async fn reverse_references(
    connection: &Connection,
    fresh: &Object,
    report: &mut Report,
    started: Instant,
    cancel: &CancellationToken,
) -> Result<(), Unknown> {
    if !matches!(
        (fresh.api_version.as_str(), fresh.kind.as_str()),
        ("v1", "ConfigMap")
            | ("v1", "Secret")
            | ("v1", "ServiceAccount")
            | ("v1", "PersistentVolumeClaim")
    ) {
        return Ok(());
    }
    const SCANS: &[&str] = &[
        "v1/pods",
        "apps/v1/deployments",
        "apps/v1/statefulsets",
        "apps/v1/daemonsets",
        "batch/v1/jobs",
        "batch/v1/cronjobs",
    ];
    for gvr in SCANS {
        if !report.available(started) {
            break;
        }
        if cancel.is_cancelled() {
            return Err(Unknown::Stale);
        }
        let Some(candidate_resource) = connection.catalog.resources.iter().find(|r| r.id() == *gvr)
        else {
            report.issue(*gvr, Unknown::Unavailable);
            continue;
        };
        report.requests += 1;
        match candidates(
            connection,
            candidate_resource,
            &fresh.namespace,
            false,
            cancel,
        )
        .await
        {
            Ok((objects, partial)) => {
                report.candidates += objects.len();
                if partial {
                    report.issue(format!("{gvr} reverse reference page"), Unknown::Partial);
                }
                for object in objects {
                    let refs = extract(&object);
                    let path = refs.targets.iter().find_map(|(t, paths)| {
                        (t.kind == fresh.kind
                            && t.api_version == fresh.api_version
                            && t.namespace == fresh.namespace
                            && t.name == fresh.name)
                            .then(|| paths.iter().next().cloned())
                            .flatten()
                    });
                    if let Some(path) = path {
                        report.link(
                            &report.root.clone(),
                            candidate_resource,
                            &object,
                            Provenance::ExplicitReference,
                            &path,
                            true,
                        );
                    }
                }
            }
            Err(reason) => report.issue(format!("{gvr} reverse reference scan"), reason),
        }
    }
    Ok(())
}
