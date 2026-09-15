use ::kube::{
    Api, Client,
    core::{ApiResource, DynamicObject},
};
use anyhow::{Context, Result, bail};
use futures_util::{StreamExt, stream};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::APIResourceList;
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, Debug)]
pub struct Resource {
    pub api: ApiResource,
    pub namespaced: bool,
    pub short_names: Vec<String>,
    pub verbs: Vec<String>,
}
impl Resource {
    pub fn qualified(&self) -> String {
        if self.api.group.is_empty() {
            self.api.plural.clone()
        } else {
            format!("{}.{}", self.api.plural, self.api.group)
        }
    }
    pub fn id(&self) -> String {
        format!("{}/{}", self.api.api_version, self.api.plural)
    }
    pub fn api(&self, client: Client, namespace: Option<&str>) -> Api<DynamicObject> {
        match namespace.filter(|_| self.namespaced) {
            Some(ns) => Api::namespaced_with(client, ns, &self.api),
            None => Api::all_with(client, &self.api),
        }
    }
}
/// Aliases resolved before consulting live discovery at all, matching kubectl
/// precedent: these always win over a CRD declaring the same shortname. Shared
/// between `resolve` and `discover` (the latter warns on a live collision) so the
/// two stay in sync.
const BUILTIN_ALIASES: &[(&str, &str)] = &[
    ("po", "pods"),
    ("dp", "deployments"),
    ("deploy", "deployments"),
    ("svc", "services"),
    ("no", "nodes"),
    ("ns", "namespaces"),
    ("cm", "configmaps"),
    ("sts", "statefulsets"),
    ("ds", "daemonsets"),
    ("rs", "replicasets"),
    ("pvc", "persistentvolumeclaims"),
    ("pv", "persistentvolumes"),
    ("crd", "customresourcedefinitions"),
];

fn dedup_by_id(candidates: Vec<&Resource>) -> Vec<&Resource> {
    let mut out: Vec<&Resource> = Vec::new();
    for r in candidates {
        if !out.iter().any(|o| o.id() == r.id()) {
            out.push(r);
        }
    }
    out
}

#[derive(Clone, Default)]
pub struct Catalog {
    pub resources: Vec<Resource>,
    pub warnings: Vec<String>,
}
impl Catalog {
    /// Resolve a human-typed name (alias, shortname, plural, kind, or plural.group) to
    /// exactly one canonical `Resource` (GVK). Never silently prefers one match over
    /// another when more than one distinct resource matches — including when one of the
    /// matches happens to be a core/built-in resource — because that would let the same
    /// typed name quietly mean different things depending on what CRDs are installed.
    /// Callers must treat the returned `Resource` as the identity from here on and must
    /// not call `resolve` again against the same catalog to "refresh" it; re-resolving is
    /// only correct when moving to a genuinely different catalog (a context/cluster switch).
    pub fn resolve(&self, query: &str, aliases: &BTreeMap<String, String>) -> Result<Resource> {
        let builtin = BUILTIN_ALIASES
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(query))
            .map(|(_, target)| *target)
            .unwrap_or(query);
        let q = aliases.get(query).map(String::as_str).unwrap_or(builtin);
        // Explicit qualification (id form "version/plural" or "plural.group") is how a
        // user resolves ambiguity themselves; it always wins outright. Only actually
        // qualified input takes this path: a bare word like "widgets" must NOT match a
        // core resource's `qualified()` just because a core resource's qualified form has
        // no group suffix — that would silently prefer core the same way this function
        // exists to prevent for plural/kind/shortname matches below.
        if (q.contains('/') || q.contains('.'))
            && let Some(r) = self
                .resources
                .iter()
                .find(|r| r.id().eq_ignore_ascii_case(q) || r.qualified().eq_ignore_ascii_case(q))
        {
            return Ok(r.clone());
        }
        let by_plural_or_kind = dedup_by_id(
            self.resources
                .iter()
                .filter(|r| {
                    r.api.plural.eq_ignore_ascii_case(q) || r.api.kind.eq_ignore_ascii_case(q)
                })
                .collect(),
        );
        match by_plural_or_kind.len() {
            1 => return Ok(by_plural_or_kind[0].clone()),
            n if n > 1 => bail!(
                "{q:?} is ambiguous across API groups; qualify explicitly as plural.group: {}",
                by_plural_or_kind
                    .iter()
                    .map(|r| r.qualified())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            _ => {}
        }
        // Shortnames last, matching kubectl precedent: the small built-in alias table
        // above always wins for its 12 entries even if a CRD declares the same
        // shortname (discovery records that collision as a warning, not silently).
        // Among live discovery shortnames, case-insensitive and ambiguity-checked too.
        let by_shortname = dedup_by_id(
            self.resources
                .iter()
                .filter(|r| r.short_names.iter().any(|s| s.eq_ignore_ascii_case(q)))
                .collect(),
        );
        match by_shortname.len() {
            1 => return Ok(by_shortname[0].clone()),
            n if n > 1 => bail!(
                "shortname {q:?} is ambiguous across API groups; qualify explicitly as plural.group: {}",
                by_shortname
                    .iter()
                    .map(|r| r.qualified())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            _ => {}
        }
        if self.warnings.is_empty() {
            bail!("Resource not found in discovery; use a plural, kind, shortname or plural.group")
        }
        bail!(
            "Resource not found, but discovery was partial ({} warning(s)) so this is not \
             proof it doesn't exist; check discovery warnings and try plural.group",
            self.warnings.len()
        )
    }
}

/// One-shot bounded list of object names for `resource` (e.g. namespaces for a picker),
/// not a watch. Bounded to 500 like related-Events reads; `truncated` reports whether
/// the server indicated more exist via a `continue` token.
pub async fn list_names(
    connection: &super::Connection,
    resource: &Resource,
) -> Result<(Vec<String>, bool)> {
    let api = resource.api(connection.client.clone(), None);
    let params = ::kube::api::ListParams::default().limit(500);
    let list = tokio::time::timeout(connection.timeout(), api.list(&params))
        .await
        .context("List timed out")?
        .map_err(|e| {
            anyhow::anyhow!(crate::safety::api_error(
                &e,
                &format!("listing {}", resource.qualified())
            ))
        })?;
    let truncated = list
        .metadata
        .continue_
        .as_deref()
        .is_some_and(|s| !s.is_empty());
    let names = list
        .items
        .into_iter()
        .filter_map(|o| o.metadata.name)
        .collect();
    Ok((names, truncated))
}

pub async fn discover(client: &Client, timeout: Duration) -> Result<Catalog> {
    let core = tokio::time::timeout(timeout, client.list_core_api_resources("v1"))
        .await
        .context("Core discovery timed out")?
        .map_err(|e| anyhow::anyhow!(crate::safety::api_error(&e, "discovering core/v1")))?;
    let mut catalog = Catalog::default();
    append(&mut catalog.resources, core)?;
    let groups = match tokio::time::timeout(timeout, client.list_api_groups()).await {
        Ok(Ok(groups)) => groups,
        result => {
            catalog.warnings.push(match result {
                Ok(Err(e)) => crate::safety::api_error(&e, "listing API groups"),
                _ => "API group enumeration timed out".into(),
            });
            return Ok(catalog);
        }
    };
    // Server-preferred version first; retain other versions when a resource is absent there.
    let mut versions = Vec::new();
    for group in groups.groups {
        let preferred = group.preferred_version.map(|v| v.group_version);
        let mut v = group.versions;
        v.sort_by_key(|v| Some(&v.group_version) != preferred.as_ref());
        versions.extend(v.into_iter().map(|v| v.group_version));
    }
    let mut responses = stream::iter(versions.into_iter().map(|version| {
        let client = client.clone();
        async move {
            let result =
                tokio::time::timeout(timeout, client.list_api_group_resources(&version)).await;
            (version, result)
        }
    }))
    .buffered(4);
    while let Some((version, result)) = responses.next().await {
        match result {
            Ok(Ok(list)) => {
                if append(&mut catalog.resources, list).is_err() {
                    catalog
                        .warnings
                        .push(format!("Malformed API discovery for {version}"));
                }
            }
            Ok(Err(e)) => catalog.warnings.push(crate::safety::api_error(
                &e,
                &format!("discovering {version}"),
            )),
            Err(_) => catalog
                .warnings
                .push(format!("Discovery timed out for {version}")),
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    catalog
        .resources
        .retain(|r| seen.insert((r.api.group.clone(), r.api.plural.clone())));
    catalog.resources.sort_by_key(Resource::qualified);
    // A CRD (or any discovered resource) can declare a shortname that collides with the
    // small built-in alias table `resolve` always checks first; the built-in wins
    // deterministically (kubectl precedent), but that must be a visible warning, not a
    // silent dead end for whoever typed the CRD's own shortname expecting it to work.
    for (alias, target) in BUILTIN_ALIASES {
        for r in &catalog.resources {
            if r.short_names.iter().any(|s| s.eq_ignore_ascii_case(alias))
                && !r.api.plural.eq_ignore_ascii_case(target)
            {
                catalog.warnings.push(format!(
                    "shortname {alias:?} on {} is shadowed by the built-in alias to {target}; use plural.group to reach it",
                    r.qualified()
                ));
            }
        }
    }
    Ok(catalog)
}
fn append(out: &mut Vec<Resource>, list: APIResourceList) -> Result<()> {
    let (group, version) = list
        .group_version
        .split_once('/')
        .unwrap_or(("", &list.group_version));
    if version.is_empty() {
        bail!("missing group version");
    }
    for r in list.resources {
        if r.name.contains('/') || !r.verbs.iter().any(|v| v == "list") {
            continue;
        }
        out.push(Resource {
            api: ApiResource {
                group: group.into(),
                version: version.into(),
                api_version: list.group_version.clone(),
                kind: r.kind,
                plural: r.name,
            },
            namespaced: r.namespaced,
            short_names: r.short_names.unwrap_or_default(),
            verbs: r.verbs,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn core_wins_and_namespaces_have_correct_scope() {
        let c = Catalog {
            resources: vec![Resource {
                api: ApiResource {
                    group: "".into(),
                    version: "v1".into(),
                    api_version: "v1".into(),
                    kind: "Pod".into(),
                    plural: "pods".into(),
                },
                namespaced: true,
                short_names: vec!["po".into()],
                verbs: vec!["list".into()],
            }],
            warnings: vec![],
        };
        assert_eq!(
            c.resolve("po", &BTreeMap::new()).expect("pod").id(),
            "v1/pods"
        );
    }
    fn resource(group: &str, plural: &str, kind: &str, short_names: &[&str]) -> Resource {
        Resource {
            api: ApiResource {
                group: group.into(),
                version: "v1".into(),
                api_version: if group.is_empty() {
                    "v1".into()
                } else {
                    format!("{group}/v1")
                },
                kind: kind.into(),
                plural: plural.into(),
            },
            namespaced: true,
            short_names: short_names.iter().map(|s| s.to_string()).collect(),
            verbs: vec!["list".into()],
        }
    }
    #[test]
    fn po_and_pods_resolve_to_the_identical_resource() {
        let c = Catalog {
            resources: vec![resource("", "pods", "Pod", &["po"])],
            warnings: vec![],
        };
        let empty = BTreeMap::new();
        assert_eq!(
            c.resolve("po", &empty).expect("po").id(),
            c.resolve("pods", &empty).expect("pods").id()
        );
    }
    #[test]
    fn builtin_alias_wins_over_a_colliding_crd_shortname_regardless_of_case() {
        // Found live: a CRD declaring its own "po" shortname makes "PO" (but not "po")
        // fall through to genuine discovery-based ambiguity between it and pods, because
        // the built-in alias table was matched case-sensitively. The built-in must win
        // deterministically for any case, exactly like the lowercase form already does.
        let c = Catalog {
            resources: vec![
                resource("", "pods", "Pod", &["po"]),
                resource("custom.io", "portals", "Portal", &["po"]),
            ],
            warnings: vec![],
        };
        let empty = BTreeMap::new();
        for query in ["po", "PO", "Po", "pO"] {
            assert_eq!(
                c.resolve(query, &empty)
                    .unwrap_or_else(|e| panic!("{query}: {e}"))
                    .api
                    .plural,
                "pods",
                "{query}"
            );
        }
    }
    #[test]
    fn cross_group_plural_collision_is_rejected_even_though_one_is_core() {
        // A CRD that happens to also use the plural "widgets" as some core-ish resource
        // would previously be silently shadowed by a `group.is_empty()` preference.
        let c = Catalog {
            resources: vec![
                resource("", "widgets", "Widget", &[]),
                resource("custom.io", "widgets", "Widget", &[]),
            ],
            warnings: vec![],
        };
        let error = c.resolve("widgets", &BTreeMap::new()).unwrap_err();
        assert!(error.to_string().contains("ambiguous"), "{error}");
        assert!(error.to_string().contains("widgets.custom.io"), "{error}");
    }
    #[test]
    fn qualified_plural_group_disambiguates_a_cross_group_collision() {
        let c = Catalog {
            resources: vec![
                resource("", "widgets", "Widget", &[]),
                resource("custom.io", "widgets", "Widget", &[]),
            ],
            warnings: vec![],
        };
        assert_eq!(
            c.resolve("widgets.custom.io", &BTreeMap::new())
                .expect("qualified")
                .api
                .group,
            "custom.io"
        );
    }
    #[test]
    fn shortname_matching_is_case_insensitive() {
        let c = Catalog {
            resources: vec![resource("custom.io", "widgets", "Widget", &["wd"])],
            warnings: vec![],
        };
        assert_eq!(
            c.resolve("WD", &BTreeMap::new())
                .expect("uppercase shortname")
                .api
                .plural,
            "widgets"
        );
    }
    #[test]
    fn cross_group_shortname_collision_is_rejected() {
        let c = Catalog {
            resources: vec![
                resource("a.io", "widgets", "Widget", &["wd"]),
                resource("b.io", "wardens", "Warden", &["wd"]),
            ],
            warnings: vec![],
        };
        let error = c.resolve("wd", &BTreeMap::new()).unwrap_err();
        assert!(error.to_string().contains("ambiguous"), "{error}");
    }
    #[test]
    fn not_found_error_mentions_partial_discovery_when_warnings_exist() {
        let c = Catalog {
            resources: vec![],
            warnings: vec!["some group timed out".into()],
        };
        let error = c.resolve("nope", &BTreeMap::new()).unwrap_err();
        assert!(error.to_string().contains("partial"), "{error}");
    }
}
