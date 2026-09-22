//! Blast radius = a read-only safety lens over the exact same bounded
//! relationship graph Adjacent/Xray already collect
//! (`kube::relationships::report::xray`), grouped by `graph::Provenance`
//! instead of navigation direction. It is not a causal or predictive
//! engine: every row is labeled by exactly how the relationship is known,
//! never merged into one undifferentiated "affected" bucket, and language
//! is constrained to what the evidence actually supports. No mutation is
//! ever executed here -- this module only renders text. See
//! `docs/M11_ACCEPTANCE.md`'s M11.5 section for the frozen contract.
use crate::adjacent::Target;
use crate::graph::{Bound, Identity, Provenance};
use crate::kube::relationships::report::Report;

/// Deliberately distinct wording from `adjacent::label` (which is tuned for
/// navigation: "OWNED BY"/"OWNS"). This module's audience is a safety
/// decision, not a navigation choice, so the same `Provenance` values get
/// safety-oriented category names instead -- one enum, two presentations,
/// never a second relationship taxonomy.
fn category(provenance: Provenance) -> &'static str {
    match provenance {
        Provenance::OwnerReference => "VERIFIED OWNERSHIP/DEPENDENCY",
        Provenance::ExplicitReference => "EXPLICIT REFERENCE",
        Provenance::SelectorMatch => "SELECTOR-DERIVED (INFERENCE)",
        Provenance::StatusReference => "STATUS-REPORTED",
    }
}

pub fn report(report: &Report) -> (String, Vec<Target>) {
    let root = &report.root;
    let mut out = format!(
        "BLAST RADIUS: {} {}/{}\nEvidence collected at {}\nRequests: {} \u{b7} Candidates scanned: {}\n\n",
        root.resource,
        root.namespace,
        root.name,
        chrono::Utc::now().to_rfc3339(),
        report.requests,
        report.candidates,
    );
    if !report.graph.partial.is_empty() {
        let mut bounds: Vec<&Bound> = report.graph.partial.iter().collect();
        bounds.sort_by_key(|b| format!("{b:?}"));
        out.push_str("PARTIAL: bound reached (");
        out.push_str(
            &bounds
                .iter()
                .map(|b| format!("{b:?}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str(") -- this is not the complete relationship set.\n\n");
    }
    if !report.issues.is_empty() {
        out.push_str("PARTIAL EVIDENCE\n");
        for issue in &report.issues {
            out.push_str(&format!("- {}: {:?}\n", issue.source, issue.reason));
        }
        out.push('\n');
    }
    let mut targets = Vec::new();
    out.push_str("DIRECTLY TARGETED\n");
    let root_health = report
        .nodes
        .get(root)
        .map(|n| n.health.status.clone())
        .unwrap_or_else(|| "UNKNOWN".into());
    out.push_str(&format!(
        "  {} {}/{} [{root_health}]\n\n",
        root.resource, root.namespace, root.name
    ));
    if let Some(node) = report.nodes.get(root) {
        targets.push(Target {
            line: out.matches('\n').count().saturating_sub(2),
            resource: node.resource.clone(),
            namespace: root.namespace.clone(),
            name: root.name.clone(),
            uid: root.uid.clone(),
        });
    }
    for provenance in [
        Provenance::OwnerReference,
        Provenance::ExplicitReference,
        Provenance::SelectorMatch,
        Provenance::StatusReference,
    ] {
        let mut rows: Vec<(Identity, bool, &std::collections::BTreeSet<String>)> = report
            .graph
            .edges()
            .iter()
            .filter(|(edge, _)| edge.provenance == provenance)
            .filter_map(|(edge, paths)| {
                if &edge.from == root {
                    Some((edge.to.clone(), true, paths))
                } else if &edge.to == root {
                    Some((edge.from.clone(), false, paths))
                } else {
                    None
                }
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        out.push_str(&format!("{}\n", category(provenance)));
        for (other, outgoing, paths) in rows {
            let health = report
                .nodes
                .get(&other)
                .map(|n| n.health.status.clone())
                .unwrap_or_else(|| "UNKNOWN".into());
            let via: Vec<&str> = paths.iter().map(String::as_str).collect();
            let direction = if outgoing {
                "structurally related to"
            } else {
                "structurally relates to"
            };
            out.push_str(&format!(
                "  {} {}/{} [{health}] -- {direction} the target via {}\n",
                other.resource,
                other.namespace,
                other.name,
                via.join(", ")
            ));
            if let Some(node) = report.nodes.get(&other) {
                targets.push(Target {
                    line: out.matches('\n').count().saturating_sub(1),
                    resource: node.resource.clone(),
                    namespace: other.namespace.clone(),
                    name: other.name.clone(),
                    uid: other.uid.clone(),
                });
            }
        }
        out.push('\n');
    }
    out.push_str(
        "This is a relationship summary, not a prediction: a relationship shown here is \
         not proof of cause, guaranteed impact, or future failure. Selector-derived rows \
         are inference (label matching), never verified ownership. No action is taken by \
         this view; any real change still goes through the normal mutation preview/\
         confirmation flow.\n",
    );
    (crate::safety::text(&out), targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, Graph, Limits};
    use crate::kube::discovery::Resource;
    use crate::kube::relationships::report::Node;
    use crate::resources::health::{Health, Severity};
    use kube::core::ApiResource;
    use std::collections::BTreeMap;

    fn id(kind: &str, name: &str, uid: &str) -> Identity {
        Identity {
            scope: 0,
            resource: kind.to_lowercase(),
            namespace: "default".into(),
            name: name.into(),
            uid: uid.into(),
        }
    }
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
    fn node(kind: &str, severity: Severity) -> Node {
        Node {
            resource: resource(kind),
            health: Health {
                status: format!("{severity:?}"),
                severity,
                evidence: vec![],
            },
        }
    }

    fn base_report(root: Identity) -> Report {
        Report {
            root,
            graph: Graph::new(0, Limits::default()),
            nodes: BTreeMap::new(),
            issues: vec![],
            requests: 1,
            candidates: 1,
        }
    }

    #[test]
    fn owner_chain_is_grouped_separately_from_selector_matches() {
        let root = id("Deployment", "web", "web-uid");
        let rs = id("ReplicaSet", "web-rs", "rs-uid");
        let svc_pod = id("Pod", "other-pod", "svc-pod-uid");
        let mut r = base_report(root.clone());
        r.nodes
            .insert(root.clone(), node("Deployment", Severity::Healthy));
        r.nodes
            .insert(rs.clone(), node("ReplicaSet", Severity::Healthy));
        r.nodes
            .insert(svc_pod.clone(), node("Pod", Severity::Warning));
        r.graph
            .insert(
                Edge {
                    from: rs.clone(),
                    to: root.clone(),
                    provenance: Provenance::OwnerReference,
                },
                "ownerReferences",
            )
            .unwrap();
        r.graph
            .insert(
                Edge {
                    from: root.clone(),
                    to: svc_pod.clone(),
                    provenance: Provenance::SelectorMatch,
                },
                "spec.selector",
            )
            .unwrap();
        let (text, _) = report(&r);
        assert!(text.contains("VERIFIED OWNERSHIP/DEPENDENCY"));
        assert!(text.contains("SELECTOR-DERIVED (INFERENCE)"));
        let owner_pos = text.find("VERIFIED OWNERSHIP/DEPENDENCY").unwrap();
        let selector_pos = text.find("SELECTOR-DERIVED (INFERENCE)").unwrap();
        assert_ne!(owner_pos, selector_pos, "must be two distinct sections");
    }

    #[test]
    fn never_uses_causal_or_predictive_language() {
        let root = id("Pod", "p", "p-uid");
        let mut r = base_report(root.clone());
        r.nodes
            .insert(root.clone(), node("Pod", Severity::Critical));
        let (text, _) = report(&r);
        // "guaranteed"/"cause" deliberately excluded from this list: the
        // report's own disclaimer legitimately says "not... guaranteed
        // impact" and "not proof of cause" -- negated safety language, not
        // a violation. These are the actual affirmative-claim shapes to
        // forbid.
        for banned in [
            "will fail",
            "will be down",
            "users affected",
            "is the cause",
        ] {
            assert!(
                !text.to_lowercase().contains(banned),
                "output must never claim causality/prediction: found {banned:?}"
            );
        }
        assert!(text.contains("not a prediction"));
        assert!(text.contains("not proof of cause"));
    }

    #[test]
    fn bound_hit_is_visible() {
        let root = id("Pod", "p", "p-uid");
        let mut r = base_report(root.clone());
        r.graph.partial.insert(Bound::Nodes);
        let (text, _) = report(&r);
        assert!(text.contains("PARTIAL: bound reached"));
        assert!(text.contains("Nodes"));
    }

    #[test]
    fn no_bound_hit_produces_no_partial_banner() {
        let root = id("Pod", "p", "p-uid");
        let r = base_report(root);
        let (text, _) = report(&r);
        assert!(!text.contains("PARTIAL: bound reached"));
    }

    #[test]
    fn directly_targeted_always_appears_first_and_is_navigable() {
        let root = id("Pod", "p", "p-uid");
        let mut r = base_report(root.clone());
        r.nodes.insert(root.clone(), node("Pod", Severity::Healthy));
        let (text, targets) = report(&r);
        assert!(text.starts_with("BLAST RADIUS"));
        let directly_targeted_pos = text.find("DIRECTLY TARGETED").unwrap();
        assert!(
            directly_targeted_pos < text.len() / 2,
            "root section near the top"
        );
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].uid, "p-uid");
    }
}
