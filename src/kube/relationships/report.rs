//! One selected-object operation; no permanent graph cache or spawned workers.
use super::{children, fetch_target};
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
