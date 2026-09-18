//! Renders a bounded `kube::relationships::report::Report` as grouped text.
//! Relationship does not imply cause; health is reused verbatim from
//! `resources::health`, never a second "graph health". Grouping is intrinsic
//! to provenance and direction, never a name/label heuristic.
use crate::graph::{Identity, Provenance};
use crate::kube::discovery::Resource;
use crate::kube::relationships::report::Report;

/// A related object the Adjacent document can jump to. `line` is the 0-based
/// index into the rendered text this target's row occupies, so `Follow` can
/// map the document's current scroll position back to a canonical identity.
#[derive(Clone)]
pub struct Target {
    pub line: usize,
    pub resource: Resource,
    pub namespace: String,
    pub name: String,
    pub uid: String,
}

const GROUPS: [&str; 5] = [
    "OWNED BY",
    "OWNS",
    "SELECTED BY",
    "REFERENCES",
    "REFERENCED BY",
];

/// Shared by Xray so the two views never diverge on what a direction/
/// provenance pair means.
pub(crate) fn label(outgoing: bool, provenance: Provenance) -> &'static str {
    match provenance {
        Provenance::OwnerReference => {
            if outgoing {
                "OWNED BY"
            } else {
                "OWNS"
            }
        }
        Provenance::SelectorMatch => "SELECTED BY",
        Provenance::ExplicitReference | Provenance::StatusReference => {
            if outgoing {
                "REFERENCES"
            } else {
                "REFERENCED BY"
            }
        }
    }
}

fn group(root: &Identity, from: &Identity, provenance: Provenance) -> &'static str {
    label(from == root, provenance)
}

pub fn report(report: &Report) -> (String, Vec<Target>) {
    let root = &report.root;
    let mut out = format!(
        "ADJACENT: {} {}/{}\nEvidence collected at {}\nRequests: {} · Candidates scanned: {}\n\n",
        root.resource,
        root.namespace,
        root.name,
        chrono::Utc::now().to_rfc3339(),
        report.requests,
        report.candidates,
    );
    if !report.issues.is_empty() {
        out.push_str("PARTIAL EVIDENCE\n");
        for issue in &report.issues {
            out.push_str(&format!("- {}: {:?}\n", issue.source, issue.reason));
        }
        out.push('\n');
    }
    let mut targets = Vec::new();
    for label in GROUPS {
        let mut rows: Vec<_> = report
            .graph
            .edges()
            .iter()
            .filter_map(|(edge, paths)| {
                let other = if &edge.from == root {
                    &edge.to
                } else if &edge.to == root {
                    &edge.from
                } else {
                    return None;
                };
                (group(root, &edge.from, edge.provenance) == label)
                    .then(|| (other.clone(), edge.provenance, paths.clone()))
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        out.push_str(&format!("{label}\n"));
        for (id, provenance, paths) in rows {
            let health = report
                .nodes
                .get(&id)
                .map(|n| n.health.status.clone())
                .unwrap_or_else(|| "UNKNOWN".into());
            let detail = match provenance {
                Provenance::StatusReference => " (status reference)",
                _ => "",
            };
            let via: Vec<&str> = paths.iter().map(String::as_str).collect();
            out.push_str(&format!(
                "  {} {}/{} [{health}]{detail} via {}\n",
                id.resource,
                id.namespace,
                id.name,
                via.join(", ")
            ));
            if let Some(node) = report.nodes.get(&id) {
                targets.push(Target {
                    line: out.matches('\n').count().saturating_sub(1),
                    resource: node.resource.clone(),
                    namespace: id.namespace.clone(),
                    name: id.name.clone(),
                    uid: id.uid.clone(),
                });
            }
        }
        out.push('\n');
    }
    out.push_str(
        "Scope: bounded direct relationships only (ownerReferences, explicit typed \
         references, selector matches, status-backed references). A related object \
         is not automatically the cause of a problem. See Xray for deeper traversal.\n",
    );
    (crate::safety::text(&out), targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        graph::{Edge, Graph, Limits},
        kube::relationships::report::{Issue, Node},
        resources::health::{Health, Severity},
    };
    use kube::core::ApiResource;
    use std::collections::BTreeMap;

    fn resource(kind: &str) -> Resource {
        Resource {
            api: ApiResource {
                group: "".into(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: kind.into(),
                plural: format!("{}s", kind.to_lowercase()),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec![],
        }
    }
    fn id(resource: &str, name: &str, uid: &str) -> Identity {
        Identity {
            scope: 1,
            resource: resource.into(),
            namespace: "n".into(),
            name: name.into(),
            uid: uid.into(),
        }
    }

    #[test]
    fn groups_by_direction_and_provenance_never_by_name() {
        let root = id("v1/pods", "p", "root");
        let owner = id("apps/v1/replicasets", "rs", "owner");
        let child = id("v1/pods", "child", "child-uid");
        let cm = id("v1/configmaps", "cm", "cm-uid");
        let mut graph = Graph::new(1, Limits::default());
        graph
            .insert(
                Edge {
                    from: root.clone(),
                    to: owner.clone(),
                    provenance: Provenance::OwnerReference,
                },
                "/metadata/ownerReferences/0",
            )
            .unwrap();
        graph
            .insert(
                Edge {
                    from: child.clone(),
                    to: root.clone(),
                    provenance: Provenance::OwnerReference,
                },
                "/metadata/ownerReferences/0",
            )
            .unwrap();
        graph
            .insert(
                Edge {
                    from: root.clone(),
                    to: cm.clone(),
                    provenance: Provenance::ExplicitReference,
                },
                "/spec/volumes/0/configMap/name",
            )
            .unwrap();
        let mut nodes = BTreeMap::new();
        for (identity, kind) in [
            (owner.clone(), "ReplicaSet"),
            (child.clone(), "Pod"),
            (cm.clone(), "ConfigMap"),
        ] {
            nodes.insert(
                identity,
                Node {
                    resource: resource(kind),
                    health: Health {
                        status: "Healthy".into(),
                        severity: Severity::Healthy,
                        evidence: vec![],
                    },
                },
            );
        }
        let rep = Report {
            root: root.clone(),
            graph,
            nodes,
            issues: vec![Issue {
                source: "reverse ownership limited".into(),
                reason: crate::evidence::Unknown::Partial,
            }],
            requests: 3,
            candidates: 5,
        };
        let (text, targets) = report(&rep);
        assert!(text.contains("OWNED BY"));
        assert!(text.contains("apps/v1/replicasets n/rs"));
        assert!(text.contains("OWNS"));
        assert!(text.contains("v1/pods n/child"));
        assert!(text.contains("REFERENCES"));
        assert!(text.contains("v1/configmaps n/cm"));
        assert!(!text.contains("REFERENCED BY"));
        assert!(!text.contains("SELECTED BY"));
        assert!(text.contains("PARTIAL EVIDENCE"));
        assert_eq!(targets.len(), 3);
        for target in &targets {
            let line = text.lines().nth(target.line).expect("line exists");
            assert!(line.contains(&target.name));
        }
    }

    #[test]
    fn selector_match_is_its_own_group_regardless_of_direction() {
        let root = id("v1/services", "svc", "svc-uid");
        let pod = id("v1/pods", "pod", "pod-uid");
        let mut graph = Graph::new(1, Limits::default());
        graph
            .insert(
                Edge {
                    from: pod.clone(),
                    to: root.clone(),
                    provenance: Provenance::SelectorMatch,
                },
                "/spec/selector = {}",
            )
            .unwrap();
        let mut nodes = BTreeMap::new();
        nodes.insert(
            pod.clone(),
            Node {
                resource: resource("Pod"),
                health: Health {
                    status: "Healthy".into(),
                    severity: Severity::Healthy,
                    evidence: vec![],
                },
            },
        );
        let rep = Report {
            root,
            graph,
            nodes,
            issues: vec![],
            requests: 1,
            candidates: 1,
        };
        let (text, targets) = report(&rep);
        assert!(text.contains("SELECTED BY"));
        assert_eq!(targets.len(), 1);
    }
}
