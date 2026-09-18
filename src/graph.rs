//! On-demand topology metadata, not another Kubernetes object store.
pub mod references;
use crate::{
    evidence::{Observation, Origin, Unknown},
    kube::discovery::Resource,
    resources::Object,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// `scope` is the originating runtime epoch, never a human context alias alone.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Identity {
    pub scope: u64,
    pub resource: String,
    pub namespace: String,
    pub name: String,
    pub uid: String,
}
impl Identity {
    pub fn observed(scope: u64, resource: &Resource, object: &Object) -> Result<Self, Unknown> {
        if object.uid.is_empty() || object.name.is_empty() {
            return Err(Unknown::NotReported);
        }
        if object.api_version != resource.api.api_version
            || object.kind != resource.api.kind
            || (resource.namespaced && object.namespace.is_empty())
            || (!resource.namespaced && !object.namespace.is_empty())
        {
            return Err(Unknown::Malformed);
        }
        Ok(Self {
            scope,
            resource: resource.id(),
            namespace: object.namespace.clone(),
            name: object.name.clone(),
            uid: object.uid.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Provenance {
    OwnerReference,
    ExplicitReference,
    SelectorMatch,
    StatusReference,
}

/// Direction is intrinsic: child→owner, referencing→referenced, Service→Pod.
/// Reverse presentation must not change the underlying provenance.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub from: Identity,
    pub to: Identity,
    pub provenance: Provenance,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub nodes: usize,
    pub edges: usize,
    pub depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            nodes: 128,
            edges: 256,
            depth: 2,
        }
    }
}

/// Fixed-size reasons avoid an unbounded error/provenance buffer under fanout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    Nodes,
    Edges,
    Depth,
    Evidence,
}

pub struct Graph {
    scope: u64,
    limits: Limits,
    nodes: BTreeSet<Identity>,
    edges: BTreeMap<Edge, BTreeSet<String>>,
    pub partial: BTreeSet<Bound>,
    pub observed: Observation,
}
impl Graph {
    pub fn new(scope: u64, limits: Limits) -> Self {
        Self {
            scope,
            limits,
            nodes: BTreeSet::new(),
            edges: BTreeMap::new(),
            partial: BTreeSet::new(),
            observed: Observation::new(Origin::ObjectApi, None),
        }
    }

    pub fn nodes(&self) -> &BTreeSet<Identity> {
        &self.nodes
    }
    pub fn edges(&self) -> &BTreeMap<Edge, BTreeSet<String>> {
        &self.edges
    }

    pub fn insert(&mut self, edge: Edge, path: &str) -> Result<(), Unknown> {
        if edge.from.scope != self.scope || edge.to.scope != self.scope {
            return Err(Unknown::Stale);
        }
        if edge.from.uid.is_empty() || edge.to.uid.is_empty() || path.is_empty() {
            return Err(Unknown::NotReported);
        }
        // Never insert half an edge when a node budget is exhausted.
        let added: BTreeSet<_> = [&edge.from, &edge.to]
            .into_iter()
            .filter(|id| !self.nodes.contains(*id))
            .collect();
        if self.nodes.len() + added.len() > self.limits.nodes {
            self.partial.insert(Bound::Nodes);
            return Err(Unknown::Partial);
        }
        if !self.edges.contains_key(&edge) && self.edges.len() >= self.limits.edges {
            self.partial.insert(Bound::Edges);
            return Err(Unknown::Partial);
        }
        if path.len() > 1024
            || self
                .edges
                .get(&edge)
                .is_some_and(|paths| paths.len() >= 16 && !paths.contains(path))
        {
            self.partial.insert(Bound::Evidence);
            return Err(Unknown::Partial);
        }
        self.nodes.insert(edge.from.clone());
        self.nodes.insert(edge.to.clone());
        self.edges.entry(edge).or_default().insert(path.into());
        Ok(())
    }

    /// Deterministic undirected adjacency traversal; edges retain their direction.
    /// Resolver must independently enforce fetch budgets before building this graph.
    pub fn traverse(&mut self, root: &Identity) -> Vec<(Identity, usize)> {
        if root.scope != self.scope || root.uid.is_empty() || self.limits.nodes == 0 {
            return vec![];
        }
        let mut visited = BTreeSet::from([root.clone()]);
        let mut queue = VecDeque::from([(root.clone(), 0)]);
        let mut out = Vec::new();
        while let Some((id, depth)) = queue.pop_front() {
            let neighbors: BTreeSet<_> = self
                .edges
                .keys()
                .filter_map(|e| {
                    if e.from == id {
                        Some(e.to.clone())
                    } else if e.to == id {
                        Some(e.from.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for next in neighbors {
                if visited.contains(&next) {
                    continue;
                }
                if depth >= self.limits.depth {
                    self.partial.insert(Bound::Depth);
                } else if visited.len() >= self.limits.nodes {
                    self.partial.insert(Bound::Nodes);
                } else {
                    visited.insert(next.clone());
                    queue.push_back((next, depth + 1));
                }
            }
            out.push((id, depth));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(uid: &str) -> Identity {
        Identity {
            scope: 1,
            resource: "v1/pods".into(),
            namespace: "ns".into(),
            name: "same-slot".into(),
            uid: uid.into(),
        }
    }
    fn edge(a: &str, b: &str) -> Edge {
        Edge {
            from: id(a),
            to: id(b),
            provenance: Provenance::OwnerReference,
        }
    }
    #[test]
    fn incarnation_scope_and_resource_are_distinct() {
        assert_ne!(id("old"), id("new"));
        let mut other = id("old");
        other.scope = 2;
        assert_ne!(id("old"), other);
        let mut g = Graph::new(1, Limits::default());
        assert_eq!(
            g.insert(
                Edge {
                    from: other,
                    ..edge("old", "new")
                },
                "metadata.ownerReferences"
            ),
            Err(Unknown::Stale)
        );
        assert!(g.nodes().is_empty());
    }
    #[test]
    fn dedup_provenance_and_cycles_are_deterministic() {
        let mut g = Graph::new(1, Limits::default());
        for e in [
            edge("b", "c"),
            edge("c", "a"),
            edge("a", "b"),
            edge("a", "b"),
        ] {
            g.insert(e, "metadata.ownerReferences[0]").unwrap();
        }
        assert_eq!(g.edges().len(), 3);
        assert_eq!(
            g.traverse(&id("a")),
            vec![(id("a"), 0), (id("b"), 1), (id("c"), 1)]
        );
        assert!(g.partial.is_empty());
        g.insert(
            Edge {
                provenance: Provenance::SelectorMatch,
                ..edge("a", "b")
            },
            "spec.selector",
        )
        .unwrap();
        assert_eq!(g.edges().len(), 4);
    }
    #[test]
    fn budgets_are_atomic_and_partial() {
        let mut g = Graph::new(
            1,
            Limits {
                nodes: 2,
                edges: 1,
                depth: 0,
            },
        );
        g.insert(edge("a", "b"), "reference").unwrap();
        assert_eq!(g.insert(edge("a", "c"), "reference"), Err(Unknown::Partial));
        assert_eq!(g.nodes().len(), 2);
        assert_eq!(g.insert(edge("b", "a"), "reference"), Err(Unknown::Partial));
        assert_eq!(g.traverse(&id("a")), vec![(id("a"), 0)]);
        assert_eq!(
            g.partial,
            BTreeSet::from([Bound::Nodes, Bound::Edges, Bound::Depth])
        );
    }

    #[test]
    fn canonical_identity_validates_gvk_and_namespace() {
        use kube::core::ApiResource;
        use serde_json::json;
        let resource = Resource {
            api: ApiResource {
                group: String::new(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "Pod".into(),
                plural: "pods".into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec![],
        };
        let object = Object::new(
            json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"p","namespace":"n","uid":"u"}}),
        );
        let identity = Identity::observed(1, &resource, &object).unwrap();
        assert_eq!(identity.resource, "v1/pods");
        let mut invalid = object.clone();
        invalid.namespace.clear();
        assert_eq!(
            Identity::observed(1, &resource, &invalid),
            Err(Unknown::Malformed)
        );
        invalid = object.clone();
        invalid.kind = "Secret".into();
        assert_eq!(
            Identity::observed(1, &resource, &invalid),
            Err(Unknown::Malformed)
        );
        invalid = object;
        invalid.uid.clear();
        assert_eq!(
            Identity::observed(1, &resource, &invalid),
            Err(Unknown::NotReported)
        );
    }

    #[test]
    fn evidence_is_bounded_and_deduplicated() {
        let mut g = Graph::new(1, Limits::default());
        for n in 0..16 {
            g.insert(edge("a", "b"), &format!("spec.path[{n}]"))
                .unwrap();
        }
        g.insert(edge("a", "b"), "spec.path[0]").unwrap();
        assert_eq!(
            g.insert(edge("a", "b"), "spec.extra"),
            Err(Unknown::Partial)
        );
        assert_eq!(g.edges().values().next().unwrap().len(), 16);
        assert!(g.partial.contains(&Bound::Evidence));
        let mut empty = Graph::new(
            1,
            Limits {
                nodes: 0,
                ..Limits::default()
            },
        );
        assert!(empty.traverse(&id("a")).is_empty());
    }
}
