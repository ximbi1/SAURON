//! M10.4: a workspace is a named, persisted navigation intent -- the
//! serializable analog of `state::HistoryEntry`, minus `selected` (UID-
//! scoped to one already-loaded incarnation, never durable across a
//! session) and minus any live `Resource` (a workspace stores only the
//! resource's qualified id string and always re-resolves it fresh on
//! open, never assumes yesterday's GVK metadata is still valid -- it may
//! be reopened after the catalog changed, or even in a different process
//! entirely once M10.8 persists it to disk).
//!
//! **MUST NOT carry**: `mutation_test_cluster_verified`, any
//! `Confirmation`, any active `Workflow`/`BulkWorkflow`/`DrainWorkflow`,
//! any UID treated as authoritative, any credential/secret/kubeconfig
//! material. Opening a workspace is navigation/state restoration only,
//! never trust restoration -- this is why `Workspace` has no field that
//! could even hold any of the above, not just a convention to remember.
//!
//! M10.8: `Serialize`/`Deserialize` (`deny_unknown_fields`, matching
//! `Settings`' own convention) make this the actual on-disk shape too,
//! persisted through `config::Config`'s own `workspaces` field -- there
//! is no separate "persisted workspace" struct to keep in sync with
//! this one.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Bumped whenever a field's *meaning* changes (not merely added) --
/// mirrors `mutation::journal::SCHEMA_VERSION`'s own precedent. A field
/// that is only ever added, with a safe default, does not need a bump
/// (see M10_ACCEPTANCE.md's Architectural question 6).
pub const WORKSPACE_SCHEMA_VERSION: u32 = 1;

/// A concrete, deliberately small bound -- a workspace list is meant to
/// be human-scannable, matching every other M10 bound's own reasoning
/// (`selection::MAX_SELECTION`, the bulk preview render bound). Exceeding
/// it is an explicit refusal, never silent eviction of an older entry.
pub const MAX_WORKSPACES: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    #[serde(default = "workspace_schema_version_default")]
    pub schema_version: u32,
    pub name: String,
    pub context: String,
    #[serde(default)]
    pub namespace: Option<String>,
    /// Qualified resource id (e.g. `"v1/pods"`, matching `Resource::id()`)
    /// -- never a live `Resource`. Always re-resolved against the current
    /// catalog on open, exactly like `finish_history`'s own
    /// `crossed_catalog` re-resolve path.
    pub resource: String,
    #[serde(default)]
    pub labels: Option<String>,
    #[serde(default)]
    pub fields: Option<String>,
    #[serde(default)]
    pub filter_text: String,
    pub sort: String,
    #[serde(default)]
    pub descending: bool,
}
fn workspace_schema_version_default() -> u32 {
    WORKSPACE_SCHEMA_VERSION
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceError {
    BoundExceeded { max: usize },
    NameTooLong,
    NotFound,
}
impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceError::BoundExceeded { max } => write!(
                f,
                "workspace list is already at its bound ({max}); delete one first"
            ),
            WorkspaceError::NameTooLong => write!(f, "workspace name must be 64 bytes or fewer"),
            WorkspaceError::NotFound => write!(f, "no workspace with that name"),
        }
    }
}

const MAX_NAME_BYTES: usize = 64;

/// Saves (or overwrites, by name) into a bounded map -- an overwrite of
/// an EXISTING name never counts against the bound (it's a replace, not
/// a growth), matching `Selection::add`'s own "idempotent, never
/// silently grows past what the user actually intends" reasoning.
pub fn save(
    workspaces: &mut BTreeMap<String, Workspace>,
    workspace: Workspace,
) -> Result<(), WorkspaceError> {
    if workspace.name.len() > MAX_NAME_BYTES {
        return Err(WorkspaceError::NameTooLong);
    }
    if !workspaces.contains_key(&workspace.name) && workspaces.len() >= MAX_WORKSPACES {
        return Err(WorkspaceError::BoundExceeded {
            max: MAX_WORKSPACES,
        });
    }
    workspaces.insert(workspace.name.clone(), workspace);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(name: &str) -> Workspace {
        Workspace {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            name: name.into(),
            context: "kind-sauron-test".into(),
            namespace: Some("sauron-fixtures".into()),
            resource: "v1/pods".into(),
            labels: None,
            fields: None,
            filter_text: String::new(),
            sort: "NAME".into(),
            descending: false,
        }
    }

    #[test]
    fn save_then_overwrite_by_name_never_counts_twice_against_the_bound() {
        let mut map = BTreeMap::new();
        save(&mut map, workspace("a")).unwrap();
        assert_eq!(map.len(), 1);
        let mut updated = workspace("a");
        updated.sort = "AGE".into();
        save(&mut map, updated).unwrap();
        assert_eq!(map.len(), 1, "same-name save overwrites, never duplicates");
        assert_eq!(map["a"].sort, "AGE");
    }

    #[test]
    fn bound_is_refused_explicitly_never_silently_evicting_an_older_entry() {
        let mut map = BTreeMap::new();
        for i in 0..MAX_WORKSPACES {
            save(&mut map, workspace(&i.to_string())).unwrap();
        }
        assert_eq!(map.len(), MAX_WORKSPACES);
        let over = workspace("over");
        assert_eq!(
            save(&mut map, over),
            Err(WorkspaceError::BoundExceeded {
                max: MAX_WORKSPACES
            })
        );
        assert_eq!(
            map.len(),
            MAX_WORKSPACES,
            "a refused save must never mutate the map"
        );
        assert!(!map.contains_key("over"));
    }

    #[test]
    fn overwriting_an_existing_name_is_still_allowed_exactly_at_the_bound() {
        let mut map = BTreeMap::new();
        for i in 0..MAX_WORKSPACES {
            save(&mut map, workspace(&i.to_string())).unwrap();
        }
        let mut updated = workspace("0");
        updated.sort = "AGE".into();
        assert!(
            save(&mut map, updated).is_ok(),
            "overwriting an existing name at the bound must still succeed"
        );
        assert_eq!(map.len(), MAX_WORKSPACES);
        assert_eq!(map["0"].sort, "AGE");
    }

    #[test]
    fn overly_long_name_is_refused_explicitly() {
        let mut map = BTreeMap::new();
        let mut long = workspace(&"x".repeat(65));
        long.name = "x".repeat(65);
        assert_eq!(save(&mut map, long), Err(WorkspaceError::NameTooLong));
        assert!(map.is_empty());
    }
}
