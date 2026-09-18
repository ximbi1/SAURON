//! Bounded read-only relationship resolution. No tasks are spawned here: callers
//! own the future and its cancellation, just as for document evidence collection.
pub mod report;
use super::{
    Connection,
    discovery::{Catalog, Resource},
};
use crate::{
    evidence::Unknown,
    graph::{Identity, Provenance, references::Target},
    resources::Object,
};
use http_body_util::BodyExt;
use tokio_util::sync::CancellationToken;

/// kube's request builder preserves native metadata negotiation, while streaming
/// the body ourselves enforces a byte cap before JSON allocation. API error bodies
/// are never read or included in reports.
async fn read_bounded(
    connection: &Connection,
    url: &str,
    name: Option<&str>,
    metadata_only: bool,
) -> Result<serde_json::Value, Unknown> {
    let builder = kube::core::Request::new(url);
    let request = match name {
        Some(name) if metadata_only => builder.get_metadata(name, &Default::default()),
        Some(name) => builder.get(name, &Default::default()),
        None => builder.list_metadata(&kube::api::ListParams::default().limit(200)),
    }
    .map_err(|_| Unknown::Malformed)?;
    let response = connection
        .client
        .send(request.map(kube::client::Body::from))
        .await
        .map_err(api_reason)?;
    match response.status().as_u16() {
        200..=299 => {}
        401 | 403 => return Err(Unknown::Forbidden),
        404 => return Err(Unknown::NotFound),
        406 => return Err(Unknown::Unsupported),
        _ => return Err(Unknown::TransportError),
    }
    let max = if name.is_some() {
        2 * 1024 * 1024
    } else {
        8 * 1024 * 1024
    };
    let mut body = response.into_body();
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| Unknown::TransportError)?;
        if let Some(data) = frame.data_ref() {
            if bytes.len().saturating_add(data.len()) > max {
                return Err(Unknown::Partial);
            }
            bytes.extend_from_slice(data);
        }
    }
    serde_json::from_slice(&bytes).map_err(|_| Unknown::Malformed)
}

/// Exact GVK lookup, never human alias resolution. Duplicate served descriptions
/// with different canonical resources are ambiguous and must not pick a winner.
pub fn resolve_target(catalog: &Catalog, target: &Target) -> Result<Resource, Unknown> {
    let mut candidates = catalog
        .resources
        .iter()
        .filter(|r| r.api.api_version == target.api_version && r.api.kind == target.kind);
    let resource = candidates.next().ok_or(Unknown::Unavailable)?;
    if candidates.any(|other| other.id() != resource.id()) {
        return Err(Unknown::Unsupported);
    }
    Ok(resource.clone())
}

pub fn target_namespace(
    source: &Object,
    target: &Target,
    resource: &Resource,
) -> Result<Option<String>, Unknown> {
    if !resource.namespaced {
        return Ok(None);
    }
    if target.provenance == Provenance::OwnerReference && source.namespace.is_empty() {
        return Err(Unknown::Malformed);
    }
    let ns = if target.provenance == Provenance::OwnerReference {
        &source.namespace
    } else {
        &target.namespace
    };
    if ns.is_empty() {
        return Err(Unknown::NotReported);
    }
    Ok(Some(ns.clone()))
}

pub fn validate_target(
    scope: u64,
    resource: &Resource,
    target: &Target,
    object: &Object,
    namespace: Option<&str>,
) -> Result<Identity, Unknown> {
    let id = Identity::observed(scope, resource, object)?;
    if id.name != target.name || id.namespace != namespace.unwrap_or_default() {
        return Err(Unknown::Malformed);
    }
    if target
        .expected_uid
        .as_ref()
        .is_some_and(|uid| uid != &id.uid)
    {
        return Err(Unknown::TargetReplaced);
    }
    Ok(id)
}

fn api_reason(error: kube::Error) -> Unknown {
    match error {
        kube::Error::Api(response) => match response.code {
            401 | 403 => Unknown::Forbidden,
            404 => Unknown::NotFound,
            406 => Unknown::Unsupported,
            _ => Unknown::TransportError,
        },
        _ => Unknown::TransportError,
    }
}

/// Scan one explicitly chosen child GVR, one page only. The caller chooses the
/// bounded resource set; no implicit cluster-wide discovery fanout lives here.
pub async fn children(
    connection: &Connection,
    owner: &Object,
    resource: &Resource,
    cancel: &CancellationToken,
) -> Result<(Vec<Object>, bool, usize), Unknown> {
    if owner.uid.is_empty() {
        return Err(Unknown::NotReported);
    }
    // A namespaced owner cannot own cluster-scoped dependents.
    if !owner.namespace.is_empty() && !resource.namespaced {
        return Ok((vec![], false, 0));
    }
    // Cluster owners may have namespaced dependents, but M6 deliberately does not
    // silently widen a namespace-limited scan to the whole cluster.
    if owner.namespace.is_empty() && resource.namespaced {
        return Err(Unknown::Unsupported);
    }
    let namespace = resource.namespaced.then_some(owner.namespace.as_str());
    let api = resource.api(connection.client.clone(), namespace);
    let read = async {
        // Ownership requires only metadata. Particularly important if the caller
        // explicitly asks for Secret children: no Secret payload is requested.
        let value = read_bounded(connection, api.resource_url(), None, true).await?;
        let list: kube::core::ObjectList<kube::core::PartialObjectMeta<kube::core::DynamicObject>> =
            serde_json::from_value(value).map_err(|_| Unknown::Malformed)?;
        let mut partial = list
            .metadata
            .continue_
            .as_deref()
            .is_some_and(|s| !s.is_empty())
            || list.items.len() > 200;
        let inspected = list.items.len().min(200);
        let mut children = Vec::new();
        for child in list.items.into_iter().take(200) {
            if child.metadata.namespace.as_deref().unwrap_or_default()
                != namespace.unwrap_or_default()
            {
                continue;
            }
            let owned = child
                .metadata
                .owner_references
                .as_ref()
                .is_some_and(|refs| {
                    refs.iter().any(|r| {
                        r.uid == owner.uid
                            && r.name == owner.name
                            && r.kind == owner.kind
                            && r.api_version == owner.api_version
                    })
                });
            if !owned {
                continue;
            }
            if children.len() >= 50 {
                partial = true;
                break;
            }
            children.push(Object::new(serde_json::json!({"apiVersion":resource.api.api_version,"kind":resource.api.kind,"metadata":child.metadata})));
        }
        children
            .sort_by(|a, b| (&a.namespace, &a.name, &a.uid).cmp(&(&b.namespace, &b.name, &b.uid)));
        Ok((children, partial, inspected))
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Unknown::Stale),
        result = tokio::time::timeout(connection.timeout(), read) => result.unwrap_or(Err(Unknown::TimedOut)),
    }
}

/// Secret data is never requested; unsupported metadata negotiation stays UNKNOWN.
/// Return a redacted Object for shared health/projections, not a permanent cache.
pub async fn fetch_target(
    connection: &Connection,
    scope: u64,
    source: &Object,
    target: &Target,
    cancel: &CancellationToken,
) -> Result<(Resource, Identity, Object), Unknown> {
    let resource = resolve_target(&connection.catalog, target)?;
    let namespace = target_namespace(source, target, &resource)?;
    let api = resource.api(connection.client.clone(), namespace.as_deref());
    let read = async {
        let value = if resource.api.api_version == "v1" && resource.api.kind == "Secret" {
            let metadata =
                read_bounded(connection, api.resource_url(), Some(&target.name), true).await?;
            if metadata["kind"] != "PartialObjectMetadata" {
                return Err(Unknown::Unsupported);
            }
            serde_json::json!({"apiVersion": resource.api.api_version, "kind":resource.api.kind, "metadata":metadata["metadata"]})
        } else {
            read_bounded(connection, api.resource_url(), Some(&target.name), false).await?
        };
        let object = Object::new(value);
        let id = validate_target(scope, &resource, target, &object, namespace.as_deref())?;
        Ok((resource, id, object))
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Unknown::Stale),
        result = tokio::time::timeout(connection.timeout(), read) => result.unwrap_or(Err(Unknown::TimedOut)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kube::core::ApiResource;
    use serde_json::json;
    fn resource(namespaced: bool) -> Resource {
        Resource {
            api: ApiResource {
                group: "example.io".into(),
                version: "v1".into(),
                api_version: "example.io/v1".into(),
                kind: "Parent".into(),
                plural: "parents".into(),
            },
            namespaced,
            short_names: vec![],
            verbs: vec![],
        }
    }
    fn target() -> Target {
        Target {
            api_version: "example.io/v1".into(),
            kind: "Parent".into(),
            namespace: "ns".into(),
            name: "p".into(),
            expected_uid: Some("old".into()),
            provenance: Provenance::OwnerReference,
        }
    }
    #[test]
    fn owner_scope_and_uid_are_authoritative() {
        let source = Object::new(json!({"metadata":{"namespace":"ns"}}));
        assert_eq!(
            target_namespace(&source, &target(), &resource(true)),
            Ok(Some("ns".into()))
        );
        assert_eq!(
            target_namespace(&source, &target(), &resource(false)),
            Ok(None)
        );
        let cluster = Object::new(json!({}));
        assert_eq!(
            target_namespace(&cluster, &target(), &resource(true)),
            Err(Unknown::Malformed)
        );
        let replacement = Object::new(
            json!({"apiVersion":"example.io/v1","kind":"Parent","metadata":{"namespace":"ns","name":"p","uid":"new"}}),
        );
        assert_eq!(
            validate_target(1, &resource(true), &target(), &replacement, Some("ns")),
            Err(Unknown::TargetReplaced)
        );
    }
    #[test]
    fn canonical_gvk_resolution_never_guesses_plural_or_group() {
        let mut catalog = Catalog {
            resources: vec![resource(true)],
            warnings: vec![],
        };
        assert_eq!(
            resolve_target(&catalog, &target()).unwrap().id(),
            "example.io/v1/parents"
        );
        let mut duplicate = resource(true);
        duplicate.api.plural = "otherparents".into();
        catalog.resources.push(duplicate);
        assert!(matches!(
            resolve_target(&catalog, &target()),
            Err(Unknown::Unsupported)
        ));
    }
}
