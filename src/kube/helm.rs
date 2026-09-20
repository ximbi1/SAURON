//! M9.5: the ONE place in this crate allowed to read a Helm release
//! Secret's body. See `integrations::helm`'s own module doc comment for
//! the full security rationale this module implements:
//!
//! "SAUR-ON's generic object/evidence pipeline never reads, stores,
//! propagates, or renders Secret bodies. A single explicit, bounded
//! Helm-release reader may transiently fetch one exact Secret body only
//! when the user explicitly requests Helm inspection of a previously
//! identified Helm release."
//!
//! `read_release` is invoked only by `app::open_helm_view`'s own explicit
//! user-triggered spawn -- never from a watch/list/generic evidence path.
//! It performs one bounded GET, re-verifies identity (UID) and type
//! against the *freshly fetched* object (never trusting the caller's own
//! already-selected, possibly-stale copy) before ever decoding, and
//! returns only a sanitized `HelmReleaseView`. The raw fetched JSON, the
//! decoded release record, and any intermediate bytes are all local to
//! this function and are dropped when it returns -- never cached,
//! journaled, or logged.
use super::{Connection, relationships::read_bounded};
use crate::app::session::Scope;
use crate::integrations::helm::{self, DecodeError, HelmReleaseView, RELEASE_SECRET_TYPE};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelmReadError {
    /// The Secret resource kind is not present on this cluster (should
    /// never happen in practice -- Secret is a core API -- but discovery
    /// absence is still handled explicitly, never assumed).
    Unsupported,
    NotFound,
    Forbidden,
    /// The object at this namespace/name still exists but its UID no
    /// longer matches what was selected -- it was deleted and replaced
    /// (or the selection was stale). Fails closed rather than decoding
    /// a different Secret than the one the user actually picked.
    TargetReplaced,
    /// Fetched fresh, but its `type` is not `helm.sh/release.v1` -- never
    /// decoded, regardless of what the name looks like.
    WrongType,
    TransportError,
    TimedOut,
    Cancelled,
    Decode(DecodeError),
}
impl std::fmt::Display for HelmReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HelmReadError::Unsupported => {
                write!(f, "Secret resource is not available on this cluster")
            }
            HelmReadError::NotFound => write!(f, "Secret no longer exists"),
            HelmReadError::Forbidden => write!(f, "not permitted to read this Secret"),
            HelmReadError::TargetReplaced => write!(
                f,
                "selected object was deleted and replaced (UID mismatch) -- refusing to read a different object"
            ),
            HelmReadError::WrongType => write!(
                f,
                "Secret is not type {RELEASE_SECRET_TYPE} (fetched fresh, not from the cached selection)"
            ),
            HelmReadError::TransportError => write!(f, "transport error reading Secret"),
            HelmReadError::TimedOut => write!(f, "timed out reading Secret"),
            HelmReadError::Cancelled => write!(f, "cancelled"),
            HelmReadError::Decode(e) => write!(f, "{e}"),
        }
    }
}

/// `scope` identifies the exact object the user selected (namespace/name/
/// uid) -- used only to know WHAT to fetch and WHAT to compare against,
/// never as authorization by itself: the actual re-fetched object is
/// independently re-verified below before any decode is attempted.
pub async fn read_release(
    connection: &Connection,
    scope: &Scope,
    cancel: &CancellationToken,
) -> Result<HelmReleaseView, HelmReadError> {
    let resource = connection
        .catalog
        .group_kind("", "Secret")
        .ok_or(HelmReadError::Unsupported)?;
    let api = resource.api(connection.client.clone(), Some(&scope.namespace));
    let read = async {
        let value = read_bounded(connection, api.resource_url(), Some(&scope.name), false)
            .await
            .map_err(classify)?;
        if value.pointer("/metadata/uid").and_then(|v| v.as_str()) != Some(scope.uid.as_str()) {
            return Err(HelmReadError::TargetReplaced);
        }
        if value.get("type").and_then(|v| v.as_str()) != Some(RELEASE_SECRET_TYPE) {
            return Err(HelmReadError::WrongType);
        }
        let release = helm::decode_release(&value).map_err(HelmReadError::Decode)?;
        Ok(helm::sanitize(&release))
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(HelmReadError::Cancelled),
        result = tokio::time::timeout(connection.timeout(), read) => result.unwrap_or(Err(HelmReadError::TimedOut)),
    }
}

fn classify(unknown: crate::evidence::Unknown) -> HelmReadError {
    use crate::evidence::Unknown;
    match unknown {
        Unknown::NotFound => HelmReadError::NotFound,
        Unknown::Forbidden => HelmReadError::Forbidden,
        Unknown::TimedOut => HelmReadError::TimedOut,
        Unknown::Stale => HelmReadError::Cancelled,
        _ => HelmReadError::TransportError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_never_embeds_raw_payload_content() {
        // Every variant's Display is a fixed, enum-tag-derived message --
        // none of them format a field carrying raw Secret/release bytes,
        // so there is no way for a real secret value to ever reach a
        // rendered error string. This is a structural guarantee (no
        // variant even has such a field) restated as a runtime check.
        let messages = [
            HelmReadError::Unsupported.to_string(),
            HelmReadError::NotFound.to_string(),
            HelmReadError::Forbidden.to_string(),
            HelmReadError::TargetReplaced.to_string(),
            HelmReadError::WrongType.to_string(),
            HelmReadError::TransportError.to_string(),
            HelmReadError::TimedOut.to_string(),
            HelmReadError::Cancelled.to_string(),
            HelmReadError::Decode(DecodeError::MalformedRelease).to_string(),
        ];
        for m in messages {
            assert!(!m.contains("hunter2"));
            assert!(!m.to_lowercase().contains("password"));
        }
    }
}
