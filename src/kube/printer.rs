//! CRD `additionalPrinterColumns` -> live table columns.
//!
//! Deliberately NOT server-side Table content negotiation (`Accept:
//! application/json;as=Table;...`): that conversion is a one-shot, non-watchable
//! snapshot, and its `columnDefinitions` carry no JSONPath a live-watched object could
//! be re-evaluated against later -- it only tells you what kubectl would have printed
//! for the objects in that one response. It does not fit a continuously live table.
//! A CRD's own `additionalPrinterColumns`, by contrast, gives a `jsonPath` we can
//! apply to every live-watched object ourselves, so columns stay genuinely live. This
//! is the researched, deliberate choice for M3 item 5; server Table conversion is
//! documented as considered and deferred, not implemented, in `docs/M3_ACCEPTANCE.md`.
use super::{Connection, discovery::Resource};
use crate::filters::value::{Field, Kind};
use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct PrinterColumn {
    pub name: String,
    pub field: Field,
    /// 0 always shown; >0 only in wide mode, matching kubectl's own convention.
    pub priority: i64,
}

/// Fetch and parse `additionalPrinterColumns` for the CRD version currently being
/// watched. Only ever meaningful for a genuine CRD (non-empty API group); returns an
/// empty list -- not an error -- for a built-in/aggregated resource that isn't one, so
/// callers can treat "no CRD found" and "no printer columns declared" identically: no
/// enrichment, generic/curated columns still apply.
pub async fn fetch(connection: &Connection, resource: &Resource) -> Result<Vec<PrinterColumn>> {
    if resource.api.group.is_empty() {
        return Ok(vec![]);
    }
    let Ok(crd_kind) = connection
        .catalog
        .resolve("customresourcedefinitions", &connection.settings.aliases)
    else {
        return Ok(vec![]);
    };
    let api = crd_kind.api(connection.client.clone(), None);
    let name = resource.qualified();
    let crd = match tokio::time::timeout(connection.timeout(), api.get(&name)).await {
        Ok(Ok(crd)) => crd,
        Ok(Err(::kube::Error::Api(e))) if e.code == 404 => return Ok(vec![]),
        Ok(Err(e)) => {
            anyhow::bail!(crate::safety::api_error(
                &e,
                &format!("reading CustomResourceDefinition {name} for printer columns")
            ))
        }
        Err(_) => anyhow::bail!("CustomResourceDefinition read timed out"),
    };
    let versions = crd
        .data
        .pointer("/spec/versions")
        .and_then(Value::as_array)
        .context("CustomResourceDefinition has no spec.versions")?;
    let version = versions
        .iter()
        .find(|v| v.get("name").and_then(Value::as_str) == Some(resource.api.version.as_str()))
        .context("Served version not found in CustomResourceDefinition spec")?;
    let columns = version
        .pointer("/additionalPrinterColumns")
        .and_then(Value::as_array)
        .map(|cols| cols.iter().filter_map(parse_column).collect())
        .unwrap_or_default();
    Ok(columns)
}

/// A CRD's `jsonPath` is a deliberately restricted JSONPath subset (Kubernetes itself
/// documents this: simple dotted field access and plain numeric array indices only).
/// Anything outside that -- wildcards, filters, slices, `..` -- is rejected rather than
/// guessed at; that column is silently omitted rather than shown with a wrong value.
fn json_path_to_pointer(path: &str) -> Option<String> {
    let path = path.strip_prefix('.').unwrap_or(path);
    if path.is_empty() {
        return None;
    }
    let mut pointer = String::new();
    for segment in path.split('.') {
        // A valid simple path has exactly one field name per segment (the single
        // leading dot is already stripped above); an empty segment here means an
        // internal ".." (recursive descent), which is outside the safe subset.
        let (name, indices) = split_indices(segment)?;
        if name.is_empty() {
            return None;
        }
        if !is_safe_field_name(name) {
            return None;
        }
        pointer.push('/');
        pointer.push_str(&escape_pointer(name));
        for index in indices {
            pointer.push('/');
            pointer.push_str(&index);
        }
    }
    (!pointer.is_empty()).then_some(pointer)
}

/// Splits `foo[0][1]` into (`"foo"`, `["0", "1"]`); rejects anything but a plain
/// non-negative integer inside brackets (no `*`, no `?(...)`, no negative index).
fn split_indices(segment: &str) -> Option<(&str, Vec<String>)> {
    let Some(bracket) = segment.find('[') else {
        return Some((segment, vec![]));
    };
    let name = &segment[..bracket];
    let mut indices = Vec::new();
    let mut rest = &segment[bracket..];
    while let Some(stripped) = rest.strip_prefix('[') {
        let close = stripped.find(']')?;
        let index = &stripped[..close];
        if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        indices.push(index.to_owned());
        rest = &stripped[close + 1..];
    }
    rest.is_empty().then_some((name, indices))
}

fn is_safe_field_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn escape_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn parse_column(raw: &Value) -> Option<PrinterColumn> {
    let name = raw.get("name")?.as_str()?.to_owned();
    let json_path = raw.get("jsonPath")?.as_str()?;
    let pointer = json_path_to_pointer(json_path)?;
    let kind = match raw.get("type").and_then(Value::as_str) {
        Some("integer") => Kind::Integer,
        Some("number") => Kind::Number,
        Some("boolean") => Kind::Bool,
        _ => Kind::Text,
    };
    let priority = raw.get("priority").and_then(Value::as_i64).unwrap_or(0);
    Some(PrinterColumn {
        name,
        field: Field {
            key: format!("field:{pointer}"),
            kind,
        },
        priority,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simple_dotted_paths_convert_to_json_pointer() {
        assert_eq!(
            json_path_to_pointer(".spec.replicas"),
            Some("/spec/replicas".into())
        );
        assert_eq!(
            json_path_to_pointer(".status.conditions[0].type"),
            Some("/status/conditions/0/type".into())
        );
        assert_eq!(json_path_to_pointer("."), None);
    }
    #[test]
    fn unsafe_json_path_subsets_are_rejected_not_guessed_at() {
        for path in [
            ".status.conditions[?(@.type==\"Ready\")].status",
            ".spec.items[*]",
            "..deep",
            ".spec.items[-1]",
            ".spec.items[abc]",
        ] {
            assert_eq!(json_path_to_pointer(path), None, "{path}");
        }
    }
    #[test]
    fn parse_column_reads_name_type_priority_and_defaults() {
        let raw = serde_json::json!({"name":"Replicas","type":"integer","jsonPath":".spec.replicas","priority":1});
        let col = parse_column(&raw).expect("column");
        assert_eq!(col.name, "Replicas");
        assert_eq!(col.field.kind, Kind::Integer);
        assert_eq!(col.field.key, "field:/spec/replicas");
        assert_eq!(col.priority, 1);

        let no_priority = serde_json::json!({"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"});
        assert_eq!(parse_column(&no_priority).expect("column").priority, 0);
    }
    #[test]
    fn a_column_with_an_unsupported_json_path_is_omitted_not_guessed() {
        let raw =
            serde_json::json!({"name":"Bad","type":"string","jsonPath":".spec.items[*].name"});
        assert!(parse_column(&raw).is_none());
    }
}
