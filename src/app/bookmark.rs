//! M10.5: a bookmark is a persisted reference to one specific Kubernetes
//! object, as a navigation aid -- never an identity-authority token.
//! Persists GVK (as a qualified id string, same convention as
//! `app::workspace::Workspace::resource`) + namespace + name + the
//! last-observed UID. Opening a bookmark never authorizes mutation: it
//! is exactly equivalent to navigating there manually and selecting the
//! row by hand, nothing more.
use crate::resources::SharedObject;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BOOKMARK_SCHEMA_VERSION: u32 = 1;
/// Deliberately larger than `workspace::MAX_WORKSPACES` -- bookmarking
/// individual objects is a lighter-weight, more frequent action than
/// saving a whole view, but still bounded, never unbounded growth.
pub const MAX_BOOKMARKS: usize = 200;
const MAX_NAME_BYTES: usize = 64;

/// M10.8: `Serialize`/`Deserialize` make this the actual on-disk shape,
/// persisted through `config::Config`'s own `bookmarks` field -- same
/// convention as `workspace::Workspace`, no separate persisted-copy type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bookmark {
    #[serde(default = "bookmark_schema_version_default")]
    pub schema_version: u32,
    pub name: String,
    pub context: String,
    /// Qualified resource id (e.g. `"v1/pods"`) -- never a live
    /// `Resource`, same reasoning as `workspace::Workspace::resource`.
    pub resource: String,
    pub namespace: String,
    pub object_name: String,
    /// The UID observed at save time -- a starting point for comparison
    /// on reopen, never treated as still-authoritative by itself. See
    /// `Status`.
    pub uid: String,
}
fn bookmark_schema_version_default() -> u32 {
    BOOKMARK_SCHEMA_VERSION
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookmarkError {
    BoundExceeded { max: usize },
    NameTooLong,
    NotFound,
}
impl std::fmt::Display for BookmarkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BookmarkError::BoundExceeded { max } => {
                write!(
                    f,
                    "bookmark list is already at its bound ({max}); delete one first"
                )
            }
            BookmarkError::NameTooLong => write!(f, "bookmark name must be 64 bytes or fewer"),
            BookmarkError::NotFound => write!(f, "no bookmark with that name"),
        }
    }
}

/// Same-name save/overwrite never counts against the bound -- mirrors
/// `workspace::save`'s own exact reasoning.
pub fn save(
    bookmarks: &mut BTreeMap<String, Bookmark>,
    bookmark: Bookmark,
) -> Result<(), BookmarkError> {
    if bookmark.name.len() > MAX_NAME_BYTES {
        return Err(BookmarkError::NameTooLong);
    }
    if !bookmarks.contains_key(&bookmark.name) && bookmarks.len() >= MAX_BOOKMARKS {
        return Err(BookmarkError::BoundExceeded { max: MAX_BOOKMARKS });
    }
    bookmarks.insert(bookmark.name.clone(), bookmark);
    Ok(())
}

/// The one thing this module exists to get right: never collapse
/// "still exactly the object I bookmarked" and "a different object now
/// happens to have the same name" into the same rendered state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Same namespace/name, and the UID still matches.
    Exact,
    /// Same namespace/name is present, but the UID has changed --
    /// deleted and recreated (or a genuinely different object was
    /// created with the same name). Never silently treated as the
    /// original object.
    Replaced,
    /// No object with this namespace/name is present in the current,
    /// synced row set.
    Missing,
    /// The current view's own context/resource/namespace scope does not
    /// match this bookmark's own scope (or the list has not synced yet)
    /// -- status genuinely cannot be determined from what's currently
    /// loaded, never guessed at.
    NotCurrentlyViewed,
}

/// Pure: never issues a request. `current_namespace` of `None` means an
/// all-namespaces view, which still legitimately contains the
/// bookmark's own namespace's objects -- only a *different*, specific
/// namespace view genuinely cannot tell.
pub fn status(
    bookmark: &Bookmark,
    current_context: &str,
    current_resource_id: &str,
    current_namespace: Option<&str>,
    synced: bool,
    rows: &[SharedObject],
) -> Status {
    let scope_matches = synced
        && current_context == bookmark.context
        && current_resource_id == bookmark.resource
        && current_namespace.is_none_or(|ns| ns == bookmark.namespace);
    if !scope_matches {
        return Status::NotCurrentlyViewed;
    }
    match rows
        .iter()
        .find(|o| o.namespace == bookmark.namespace && o.name == bookmark.object_name)
    {
        None => Status::Missing,
        Some(o) if o.uid == bookmark.uid => Status::Exact,
        Some(_) => Status::Replaced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::Object;
    use serde_json::json;

    fn bookmark(name: &str) -> Bookmark {
        Bookmark {
            schema_version: BOOKMARK_SCHEMA_VERSION,
            name: name.into(),
            context: "kind-sauron-test".into(),
            resource: "v1/pods".into(),
            namespace: "sauron-fixtures".into(),
            object_name: "web-1".into(),
            uid: "uid-1".into(),
        }
    }
    fn object(ns: &str, name: &str, uid: &str) -> SharedObject {
        std::sync::Arc::new(Object::new(json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"namespace":ns,"name":name,"uid":uid}
        })))
    }

    #[test]
    fn save_then_overwrite_by_name_never_counts_twice_against_the_bound() {
        let mut map = BTreeMap::new();
        save(&mut map, bookmark("a")).unwrap();
        let mut updated = bookmark("a");
        updated.uid = "uid-2".into();
        save(&mut map, updated).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["a"].uid, "uid-2");
    }

    #[test]
    fn bound_is_refused_explicitly_never_silently_evicting_an_older_entry() {
        let mut map = BTreeMap::new();
        for i in 0..MAX_BOOKMARKS {
            save(&mut map, bookmark(&i.to_string())).unwrap();
        }
        assert_eq!(
            save(&mut map, bookmark("over")),
            Err(BookmarkError::BoundExceeded { max: MAX_BOOKMARKS })
        );
        assert_eq!(map.len(), MAX_BOOKMARKS);
        assert!(!map.contains_key("over"));
    }

    #[test]
    fn status_is_exact_when_uid_still_matches() {
        let b = bookmark("a");
        let rows = [object("sauron-fixtures", "web-1", "uid-1")];
        assert_eq!(
            status(
                &b,
                "kind-sauron-test",
                "v1/pods",
                Some("sauron-fixtures"),
                true,
                &rows
            ),
            Status::Exact
        );
    }

    #[test]
    fn status_is_replaced_never_exact_when_same_name_different_uid() {
        let b = bookmark("a");
        let rows = [object("sauron-fixtures", "web-1", "uid-2")];
        assert_eq!(
            status(
                &b,
                "kind-sauron-test",
                "v1/pods",
                Some("sauron-fixtures"),
                true,
                &rows
            ),
            Status::Replaced,
            "a same-name, different-UID object must never be reported as the original"
        );
    }

    #[test]
    fn status_is_missing_when_absent_from_synced_rows() {
        let b = bookmark("a");
        let rows: [SharedObject; 0] = [];
        assert_eq!(
            status(
                &b,
                "kind-sauron-test",
                "v1/pods",
                Some("sauron-fixtures"),
                true,
                &rows
            ),
            Status::Missing
        );
    }

    #[test]
    fn status_is_not_currently_viewed_when_scope_differs_or_unsynced() {
        let b = bookmark("a");
        let rows = [object("sauron-fixtures", "web-1", "uid-1")];
        assert_eq!(
            status(
                &b,
                "other-context",
                "v1/pods",
                Some("sauron-fixtures"),
                true,
                &rows
            ),
            Status::NotCurrentlyViewed
        );
        assert_eq!(
            status(
                &b,
                "kind-sauron-test",
                "v1/deployments",
                Some("sauron-fixtures"),
                true,
                &rows
            ),
            Status::NotCurrentlyViewed
        );
        assert_eq!(
            status(
                &b,
                "kind-sauron-test",
                "v1/pods",
                Some("other-ns"),
                true,
                &rows
            ),
            Status::NotCurrentlyViewed
        );
        assert_eq!(
            status(
                &b,
                "kind-sauron-test",
                "v1/pods",
                Some("sauron-fixtures"),
                false,
                &rows
            ),
            Status::NotCurrentlyViewed,
            "an unsynced list must never be treated as authoritative"
        );
    }

    #[test]
    fn status_all_namespaces_view_still_resolves_a_specific_bookmarked_namespace() {
        let b = bookmark("a");
        let rows = [object("sauron-fixtures", "web-1", "uid-1")];
        assert_eq!(
            status(&b, "kind-sauron-test", "v1/pods", None, true, &rows),
            Status::Exact,
            "an all-namespaces view still legitimately contains the bookmark's own namespace"
        );
    }
}
