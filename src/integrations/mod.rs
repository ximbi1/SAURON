//! M9.0: shared identity/capability-discovery model for first-class GitOps/
//! package-manager integrations (Flux, Argo CD, Helm). Pure and network-
//! free -- discovery itself already happened via `kube::discovery::Catalog`
//! at connect time, exactly like every other resource kind's own presence;
//! this module only interprets that already-fetched catalog. Deliberately
//! NOT a generic "provider framework": each integration's actual read/
//! action logic lives in its own module (M9.1+), sharing only what
//! genuinely overlaps here. See docs/M9_ACCEPTANCE.md's M9.0 contract for
//! the full rationale, including why bounded reads, namespace/context
//! awareness, and journal correlation need zero new code here -- they
//! already exist and are simply reused by later slices.
pub mod flux;

use crate::evidence::Unknown;
use crate::kube::discovery::{Catalog, Resource};

/// A closed, explicit set -- never a free-form string, so a typo can never
/// silently create a fourth "integration".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Integration {
    Flux,
    ArgoCd,
    Helm,
}
impl Integration {
    pub fn label(self) -> &'static str {
        match self {
            Integration::Flux => "Flux",
            Integration::ArgoCd => "Argo CD",
            Integration::Helm => "Helm",
        }
    }
}

/// One CRD/API kind an integration might expose. `label` is the short name
/// shown in reports -- currently always identical to `kind`, kept as its
/// own field so a future integration can use a friendlier label without
/// changing the lookup identity.
#[derive(Clone, Copy, Debug)]
pub struct ExpectedKind {
    pub label: &'static str,
    pub group: &'static str,
    pub kind: &'static str,
}
impl ExpectedKind {
    pub const fn new(label: &'static str, group: &'static str, kind: &'static str) -> Self {
        Self { label, group, kind }
    }
}

/// The result of checking one integration's expected kinds against a live
/// `Catalog`. Every entry is either a real discovered `Resource` or
/// explicitly absent -- never inferred from names alone.
#[derive(Clone, Debug)]
pub struct Discovery {
    pub integration: Integration,
    pub found: Vec<(ExpectedKind, Option<Resource>)>,
}
impl Discovery {
    /// `None` means every expected kind was found -- fully available.
    /// `Some(Unsupported)` means none were found. `Some(Partial)` means
    /// some but not all (e.g. Flux's core controllers installed without
    /// the optional image-automation CRDs) -- a partial installation must
    /// never silently degrade to either extreme.
    pub fn state(&self) -> Option<Unknown> {
        if self.found.is_empty() {
            return Some(Unknown::Unsupported);
        }
        let present = self.found.iter().filter(|(_, r)| r.is_some()).count();
        match present {
            0 => Some(Unknown::Unsupported),
            n if n == self.found.len() => None,
            _ => Some(Unknown::Partial),
        }
    }
    /// The discovered `Resource` for one expected kind, by its label --
    /// `None` if that specific kind is absent (even when other kinds in
    /// the same integration are present, i.e. under `Partial`).
    pub fn resource(&self, label: &str) -> Option<&Resource> {
        self.found
            .iter()
            .find(|(k, _)| k.label == label)
            .and_then(|(_, r)| r.as_ref())
    }
}

/// Pure: `catalog` was already populated by a real discovery call at
/// connect time -- this never issues a request itself, matching every
/// other resource kind's own zero-extra-network presence check.
pub fn discover(
    catalog: &Catalog,
    integration: Integration,
    expected: &[ExpectedKind],
) -> Discovery {
    Discovery {
        integration,
        found: expected
            .iter()
            .map(|k| (*k, catalog.group_kind(k.group, k.kind).cloned()))
            .collect(),
    }
}

pub mod view {
    use super::*;

    /// Read-only capability report -- never issues a request, never claims
    /// installed/absent from names alone (every line traces to a real
    /// discovered `Resource` or an explicit absence). Mirrors
    /// `mutation::view::policy_report`'s own role: a small, pure text
    /// renderer every later integration slice's own capability view
    /// reuses verbatim.
    pub fn discovery_report(discovery: &Discovery) -> String {
        let mut out = format!("{} CAPABILITIES\n\n", discovery.integration.label());
        match discovery.state() {
            None => out.push_str("STATE: Available -- every expected kind was discovered\n\n"),
            Some(Unknown::Unsupported) => out.push_str(
                "STATE: Unsupported -- no expected CRD/API was found on this cluster\n\n",
            ),
            Some(Unknown::Partial) => {
                out.push_str("STATE: Partial -- some expected CRD/API kinds are missing\n\n")
            }
            Some(other) => out.push_str(&format!("STATE: {other:?}\n\n")),
        }
        for (kind, resource) in &discovery.found {
            match resource {
                Some(r) => {
                    out.push_str(&format!("  [present] {} ({})\n", kind.label, r.qualified()))
                }
                None => out.push_str(&format!(
                    "  [absent]  {} ({}/{})\n",
                    kind.label, kind.group, kind.kind
                )),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kube::core::ApiResource;

    fn resource(group: &str, plural: &str, kind: &str) -> Resource {
        Resource {
            api: ApiResource {
                group: group.into(),
                version: "v1".into(),
                api_version: format!("{group}/v1"),
                kind: kind.into(),
                plural: plural.into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec!["list".into()],
        }
    }

    const FLUX_KINDS: &[ExpectedKind] = &[
        ExpectedKind::new(
            "Kustomization",
            "kustomize.toolkit.fluxcd.io",
            "Kustomization",
        ),
        ExpectedKind::new("HelmRelease", "helm.toolkit.fluxcd.io", "HelmRelease"),
        ExpectedKind::new("GitRepository", "source.toolkit.fluxcd.io", "GitRepository"),
    ];

    #[test]
    fn absent_integration_is_unsupported_never_healthy_or_zero() {
        let catalog = Catalog::default();
        let discovery = discover(&catalog, Integration::Flux, FLUX_KINDS);
        assert_eq!(discovery.state(), Some(Unknown::Unsupported));
        assert!(discovery.found.iter().all(|(_, r)| r.is_none()));
        assert!(discovery.resource("Kustomization").is_none());
    }

    #[test]
    fn fully_available_integration_is_none_state() {
        let catalog = Catalog {
            resources: vec![
                resource(
                    "kustomize.toolkit.fluxcd.io",
                    "kustomizations",
                    "Kustomization",
                ),
                resource("helm.toolkit.fluxcd.io", "helmreleases", "HelmRelease"),
                resource(
                    "source.toolkit.fluxcd.io",
                    "gitrepositories",
                    "GitRepository",
                ),
            ],
            warnings: vec![],
        };
        let discovery = discover(&catalog, Integration::Flux, FLUX_KINDS);
        assert_eq!(discovery.state(), None);
        assert_eq!(
            discovery
                .resource("Kustomization")
                .expect("present")
                .api
                .plural,
            "kustomizations"
        );
    }

    #[test]
    fn partial_installation_is_never_silently_collapsed_either_direction() {
        let catalog = Catalog {
            resources: vec![resource(
                "kustomize.toolkit.fluxcd.io",
                "kustomizations",
                "Kustomization",
            )],
            warnings: vec![],
        };
        let discovery = discover(&catalog, Integration::Flux, FLUX_KINDS);
        assert_eq!(discovery.state(), Some(Unknown::Partial));
        assert!(discovery.resource("Kustomization").is_some());
        assert!(discovery.resource("HelmRelease").is_none());
        assert!(discovery.resource("GitRepository").is_none());
    }

    #[test]
    fn an_unrelated_crd_sharing_a_kind_name_but_not_group_never_counts_as_present() {
        // A CRD from a different group that happens to reuse the Kind name
        // "HelmRelease" must never be mistaken for Flux's own -- discovery
        // is by (group, kind), never by kind alone.
        let catalog = Catalog {
            resources: vec![resource("other.example.com", "helmreleases", "HelmRelease")],
            warnings: vec![],
        };
        let discovery = discover(&catalog, Integration::Flux, FLUX_KINDS);
        assert!(discovery.resource("HelmRelease").is_none());
        assert_eq!(discovery.state(), Some(Unknown::Unsupported));
    }

    #[test]
    fn discovery_report_never_claims_presence_without_a_real_resource() {
        let catalog = Catalog {
            resources: vec![resource(
                "kustomize.toolkit.fluxcd.io",
                "kustomizations",
                "Kustomization",
            )],
            warnings: vec![],
        };
        let discovery = discover(&catalog, Integration::Flux, FLUX_KINDS);
        let report = view::discovery_report(&discovery);
        assert!(report.contains("Flux CAPABILITIES"));
        assert!(report.contains("STATE: Partial"));
        assert!(report.contains("[present] Kustomization"));
        assert!(report.contains("[absent]  HelmRelease"));
        assert!(report.contains("[absent]  GitRepository"));
    }

    #[test]
    fn integration_labels_are_explicit_and_distinct() {
        assert_eq!(Integration::Flux.label(), "Flux");
        assert_eq!(Integration::ArgoCd.label(), "Argo CD");
        assert_eq!(Integration::Helm.label(), "Helm");
    }
}
