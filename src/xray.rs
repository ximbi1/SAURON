//! Xray = bounded, cycle-safe multi-hop traversal rendered as depth-grouped
//! text, reusing Adjacent's exact direction/provenance vocabulary and the
//! same deterministic health -- never a second, parallel "graph health" and
//! never a claim that a related object is the cause of a problem.
use crate::adjacent::{self, Target};
use crate::graph::{Identity, Provenance};
use crate::kube::relationships::report::Report;
use std::collections::BTreeSet;

pub fn report(report: &Report, requested_depth: usize) -> (String, Vec<Target>) {
    let root = &report.root;
    let requested_depth = requested_depth.max(1);
    let mut out = format!(
        "XRAY: {} {}/{} (up to {requested_depth} hops)\nEvidence collected at {}\nRequests: {} · Candidates scanned: {}\n\n",
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
    let mut visited: BTreeSet<Identity> = BTreeSet::from([root.clone()]);
    let mut frontier = vec![root.clone()];
    let mut targets = Vec::new();
    for hop in 1..=requested_depth {
        let mut rows: Vec<_> = frontier
            .iter()
            .flat_map(|parent| {
                let visited = &visited;
                report
                    .graph
                    .edges()
                    .iter()
                    .filter_map(move |(edge, paths)| {
                        if &edge.from == parent && !visited.contains(&edge.to) {
                            Some((
                                parent.clone(),
                                edge.to.clone(),
                                true,
                                edge.provenance,
                                paths.clone(),
                            ))
                        } else if &edge.to == parent && !visited.contains(&edge.from) {
                            Some((
                                parent.clone(),
                                edge.from.clone(),
                                false,
                                edge.provenance,
                                paths.clone(),
                            ))
                        } else {
                            None
                        }
                    })
            })
            .collect();
        // Deterministic dedup: the same child can be reachable from several
        // parents/edges at this hop; keep only the first (BTree-ordered) sighting.
        rows.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
        let mut next = Vec::new();
        let mut seen_this_hop = BTreeSet::new();
        let mut printed_header = false;
        for (parent, child, outgoing, provenance, paths) in rows {
            if visited.contains(&child) || !seen_this_hop.insert(child.clone()) {
                continue;
            }
            if !printed_header {
                out.push_str(&format!("HOP {hop}\n"));
                printed_header = true;
            }
            let health = report
                .nodes
                .get(&child)
                .map(|n| n.health.status.clone())
                .unwrap_or_else(|| "UNKNOWN".into());
            let detail = match provenance {
                Provenance::StatusReference => " (status reference)",
                _ => "",
            };
            let via: Vec<&str> = paths.iter().map(String::as_str).collect();
            out.push_str(&format!(
                "  [{}] {} {}/{} [{health}]{detail} via {} (from {}/{})\n",
                adjacent::label(outgoing, provenance),
                child.resource,
                child.namespace,
                child.name,
                via.join(", "),
                parent.namespace,
                parent.name,
            ));
            if let Some(node) = report.nodes.get(&child) {
                targets.push(Target {
                    line: out.matches('\n').count().saturating_sub(1),
                    resource: node.resource.clone(),
                    namespace: child.namespace.clone(),
                    name: child.name.clone(),
                    uid: child.uid.clone(),
                });
            }
            next.push(child);
        }
        if printed_header {
            out.push('\n');
        }
        if next.is_empty() {
            break;
        }
        visited.extend(next.iter().cloned());
        frontier = next;
    }
    out.push_str(
        "Scope: bounded traversal only; relationship does not imply cause. Health is \
         reused verbatim from the same deterministic rules shown elsewhere, never a \
         second graph-derived score.\n",
    );
    (crate::safety::text(&out), targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        graph::{Edge, Graph, Limits},
        kube::discovery::Resource,
        kube::relationships::report::{Node, Report},
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
    fn healthy(kind: &str) -> Node {
        Node {
            resource: resource(kind),
            health: Health {
                status: "Healthy".into(),
                severity: Severity::Healthy,
                evidence: vec![],
            },
        }
    }

    #[test]
    fn two_hop_traversal_is_grouped_by_distance_and_cycle_safe() {
        let root = id("apps/v1/deployments", "dep", "dep-uid");
        let rs = id("apps/v1/replicasets", "rs", "rs-uid");
        let pod = id("v1/pods", "pod", "pod-uid");
        let mut graph = Graph::new(1, Limits::default());
        graph
            .insert(
                Edge {
                    from: rs.clone(),
                    to: root.clone(),
                    provenance: Provenance::OwnerReference,
                },
                "/metadata/ownerReferences/0",
            )
            .unwrap();
        graph
            .insert(
                Edge {
                    from: pod.clone(),
                    to: rs.clone(),
                    provenance: Provenance::OwnerReference,
                },
                "/metadata/ownerReferences/0",
            )
            .unwrap();
        let mut nodes = BTreeMap::new();
        nodes.insert(rs.clone(), healthy("ReplicaSet"));
        nodes.insert(pod.clone(), healthy("Pod"));
        let rep = Report {
            root: root.clone(),
            graph,
            nodes,
            issues: vec![],
            requests: 3,
            candidates: 3,
        };
        let (text, targets) = report(&rep, 1);
        assert!(text.contains("HOP 1"));
        assert!(text.contains("apps/v1/replicasets n/rs"));
        assert!(!text.contains("HOP 2"));
        assert!(!text.contains("v1/pods n/pod"), "depth 1 stops at the RS");
        assert_eq!(targets.len(), 1);

        let (text, targets) = report(&rep, 2);
        assert!(text.contains("HOP 1"));
        assert!(text.contains("HOP 2"));
        assert!(text.contains("v1/pods n/pod"));
        assert_eq!(targets.len(), 2);
        // Depth requested beyond what the bounded report actually captured
        // must not panic or fabricate a third hop.
        let (deeper, _) = report(&rep, 3);
        assert!(!deeper.contains("HOP 3"));
    }
}
