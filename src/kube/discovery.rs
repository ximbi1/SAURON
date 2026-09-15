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
#[derive(Clone, Default)]
pub struct Catalog {
    pub resources: Vec<Resource>,
    pub warnings: Vec<String>,
}
impl Catalog {
    pub fn resolve(&self, query: &str, aliases: &BTreeMap<String, String>) -> Result<Resource> {
        let builtin = match query {
            "po" => "pods",
            "dp" | "deploy" => "deployments",
            "svc" => "services",
            "no" => "nodes",
            "ns" => "namespaces",
            "cm" => "configmaps",
            "sts" => "statefulsets",
            "ds" => "daemonsets",
            "rs" => "replicasets",
            "pvc" => "persistentvolumeclaims",
            "pv" => "persistentvolumes",
            "crd" => "customresourcedefinitions",
            q => q,
        };
        let q = aliases.get(query).map(String::as_str).unwrap_or(builtin);
        let exact = self
            .resources
            .iter()
            .find(|r| r.id().eq_ignore_ascii_case(q) || r.qualified().eq_ignore_ascii_case(q));
        if let Some(r) = exact {
            return Ok(r.clone());
        }
        let matching: Vec<_> = self
            .resources
            .iter()
            .filter(|r| r.api.plural.eq_ignore_ascii_case(q) || r.api.kind.eq_ignore_ascii_case(q))
            .collect();
        if let Some(r) = matching.iter().find(|r| r.api.group.is_empty()) {
            return Ok((*r).clone());
        }
        if matching.len() == 1 {
            return Ok(matching[0].clone());
        }
        if matching.len() > 1 {
            bail!(
                "Ambiguous resource; use one of: {}",
                matching
                    .iter()
                    .map(|r| r.qualified())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        self.resources
            .iter()
            .find(|r| r.short_names.iter().any(|s| s == q))
            .cloned()
            .context(
                "Resource not found in discovery; use a plural, kind, shortname or plural.group",
            )
    }
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
}
