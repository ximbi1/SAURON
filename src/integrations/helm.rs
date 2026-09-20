//! M9.5: Helm read-only inspection. Helm itself is a client-side tool --
//! there is no controller and no CRD; a "release" is just a Kubernetes
//! `Secret` (or `ConfigMap`, for the older storage driver) of type
//! `helm.sh/release.v1`, named `sh.helm.release.v1.<release>.v<revision>`,
//! whose `data.release` field is `base64(base64(gzip(json)))` -- one
//! base64 layer is Kubernetes' own wire encoding for byte fields
//! (transparent to any client reading raw JSON, as this app always
//! does), the other is Helm's own storage encoding on top of that.
//!
//! Because M9.0's discovery model is keyed on CRD group/kind presence,
//! and Helm has neither, this module does NOT use `discover()` --
//! "is Helm available" is not a meaningful question the way "is Flux
//! installed" is.
//!
//! **Security is the load-bearing contract of this module, restated
//! from docs/M9_ACCEPTANCE.md's own M9.5 journal entry**: SAUR-ON's
//! generic object/evidence pipeline (`resources::Object::new`'s own
//! unconditional `safety::redact`, `kube::relationships::fetch_target`'s
//! metadata-only Secret fetch) never reads, stores, propagates, or
//! renders Secret bodies, and that stays true without exception. The
//! *only* place a Helm release Secret's body is ever read is
//! `kube::helm::read_release` -- a single, dedicated, bounded, TOCTOU-
//! safe, non-caching function invoked only on an explicit user request
//! against an already-identified release Secret. This module is that
//! function's decode/sanitize logic: everything here is pure (no
//! network), and the only thing that may cross this module's boundary
//! outward is a `HelmReleaseView` -- already redacted, already
//! summarized. The raw decoded release record (`decode_release`'s own
//! return value) is deliberately not `pub`: nothing outside this module
//! (and `kube::helm`, which immediately sanitizes and drops it) can ever
//! hold an unredacted release.
use serde::Deserialize;
use serde_json::Value;
use std::io::Read;

pub const RELEASE_SECRET_TYPE: &str = "helm.sh/release.v1";

/// Mirrors `safety::redact`'s own sensitive-key vocabulary, applied here
/// to arbitrary Helm `values` structure rather than a fixed Secret
/// schema -- deliberately the same list, not a second one to keep in
/// sync, extended with a few Helm-values-specific spellings. Matching is
/// case-insensitive and substring-based, never an exact-key allowlist,
/// so `dbPassword`/`DB_PASSWORD`/`rootPassword` are all caught.
const SENSITIVE_KEY_SUBSTRINGS: &[&str] = &[
    "password",
    "token",
    "secret",
    "credential",
    "private-key",
    "privatekey",
    "apikey",
    "api-key",
    "api_key",
    "passphrase",
    "key",
];

/// Bounded independently of the HTTP response cap (`read_bounded`'s own
/// 2MB single-object cap) -- gzip can expand compressed bytes by a large
/// factor, so the DEcompressed size needs its own ceiling regardless of
/// how small the compressed/encoded input was.
const MAX_DECOMPRESSED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// `data.release` field absent -- not a Helm release record at all,
    /// or a storage shape this app does not recognize.
    MissingField,
    /// Either base64 layer failed to decode.
    InvalidEncoding,
    /// Gzip decompression failed, or exceeded the bounded size cap
    /// (refused before completing -- never partially decompressed and
    /// silently truncated as if that were the real content).
    InvalidOrOversizedCompression,
    /// Decompressed successfully but the bytes are not valid JSON, or
    /// not a JSON object -- never guessed at a partial structure.
    MalformedRelease,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DecodeError::MissingField => "no data.release field (not a Helm release record)",
            DecodeError::InvalidEncoding => "release data is not validly base64-encoded",
            DecodeError::InvalidOrOversizedCompression => {
                "release data failed to decompress, or exceeded the bounded size cap"
            }
            DecodeError::MalformedRelease => "decompressed content is not a valid release record",
        };
        write!(f, "{s}")
    }
}

fn bounded_gunzip(bytes: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = decoder
            .read(&mut chunk)
            .map_err(|_| DecodeError::InvalidOrOversizedCompression)?;
        if n == 0 {
            break;
        }
        if out.len().saturating_add(n) > MAX_DECOMPRESSED_BYTES {
            return Err(DecodeError::InvalidOrOversizedCompression);
        }
        out.extend_from_slice(&chunk[..n]);
    }
    Ok(out)
}

/// Decodes a Helm release `Secret`'s (or `ConfigMap`'s) own `data.release`
/// field into the release record Helm itself wrote. `pub(crate)`, never
/// `pub`: the only caller outside this module is `kube::helm::
/// read_release`, which immediately sanitizes the result and drops this
/// raw value before returning -- nothing else in the app may hold an
/// unredacted release record. Never guesses at a partial structure on
/// any failure -- every distinct failure mode is its own `DecodeError`
/// variant, never collapsed into "couldn't read it".
pub(crate) fn decode_release(storage_object: &Value) -> Result<Value, DecodeError> {
    use base64::Engine;
    let raw = storage_object
        .pointer("/data/release")
        .and_then(Value::as_str)
        .ok_or(DecodeError::MissingField)?;
    let engine = base64::engine::general_purpose::STANDARD;
    let once = engine
        .decode(raw)
        .map_err(|_| DecodeError::InvalidEncoding)?;
    let twice_str = std::str::from_utf8(&once).map_err(|_| DecodeError::InvalidEncoding)?;
    let gzip_bytes = engine
        .decode(twice_str.trim())
        .map_err(|_| DecodeError::InvalidEncoding)?;
    let json_bytes = bounded_gunzip(&gzip_bytes)?;
    let value: Value =
        serde_json::from_slice(&json_bytes).map_err(|_| DecodeError::MalformedRelease)?;
    if !value.is_object() {
        return Err(DecodeError::MalformedRelease);
    }
    Ok(value)
}

fn mask_sensitive(value: &mut Value, key_hint: Option<&str>) {
    let sensitive = key_hint.is_some_and(|k| {
        let lower = k.to_ascii_lowercase();
        SENSITIVE_KEY_SUBSTRINGS.iter().any(|s| lower.contains(s))
    });
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if sensitive
                    || SENSITIVE_KEY_SUBSTRINGS
                        .iter()
                        .any(|s| k.to_ascii_lowercase().contains(s))
                {
                    *v = Value::String("<redacted>".into());
                } else {
                    mask_sensitive(v, Some(k));
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                mask_sensitive(item, key_hint);
            }
        }
        Value::String(_) if sensitive => {
            *value = Value::String("<redacted>".into());
        }
        _ => {}
    }
}

/// A safe-by-default rendering of `release.config` (the user-supplied
/// values) -- every key matching a sensitive-looking substring is masked
/// recursively, at any depth, never assumed safe just because it is
/// nested. This is the ONLY values view this module ever produces; there
/// is no "show me the raw values" escape hatch anywhere in this crate.
fn redacted_values(release: &Value) -> Value {
    let mut values = release.pointer("/config").cloned().unwrap_or(Value::Null);
    mask_sensitive(&mut values, None);
    values
}

/// One resource identity extracted from a rendered manifest document --
/// metadata/ownership only, exactly like the rest of this app's
/// Adjacent/Xray model. Never the document body: a `Secret` manifest
/// entry here is `{kind: "Secret", name: "...", namespace: "..."}` and
/// nothing else -- its own `data`/`stringData` are never even parsed,
/// let alone included.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestResource {
    pub api_version: String,
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,
}

/// Splits `release.manifest` (a multi-document YAML string Helm itself
/// rendered) and extracts only `apiVersion`/`kind`/`metadata.name`/
/// `metadata.namespace` per document -- bounded to 200 documents like
/// every other bounded list in this app. A document that fails to parse
/// or is missing required identity fields is skipped, never fabricated;
/// the caller-visible count reflects only what was actually extracted.
fn manifest_resources(release: &Value) -> Vec<ManifestResource> {
    let Some(manifest) = release.pointer("/manifest").and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for document in serde_yaml_ng::Deserializer::from_str(manifest) {
        if out.len() >= 200 {
            break;
        }
        let Ok(doc) = Value::deserialize(document) else {
            continue;
        };
        let (Some(kind), Some(name)) = (
            doc.get("kind").and_then(Value::as_str),
            doc.pointer("/metadata/name").and_then(Value::as_str),
        ) else {
            continue;
        };
        let api_version = doc
            .get("apiVersion")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let namespace = doc
            .pointer("/metadata/namespace")
            .and_then(Value::as_str)
            .map(str::to_string);
        out.push(ManifestResource {
            api_version,
            kind: kind.to_string(),
            namespace,
            name: name.to_string(),
        });
    }
    out
}

/// The ONLY thing that may cross this module's boundary outward. Every
/// field here is already safe to store transiently, render, or hold
/// across an `.await` point -- there is no field carrying raw/unredacted
/// data, and no way to reconstruct the original release record from it.
/// Deliberately excludes Notes -- see `sanitize`'s own doc comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelmReleaseView {
    pub name: String,
    pub namespace: String,
    pub revision: Option<i64>,
    pub status: String,
    pub chart_name: String,
    pub chart_version: String,
    pub app_version: Option<String>,
    /// Already redacted -- see `redacted_values`.
    pub values: Value,
    /// Identity/ownership only -- see `ManifestResource`.
    pub resources: Vec<ManifestResource>,
}

/// Converts a freshly decoded release record into the sanitized view
/// this app is allowed to keep/render. Called exactly once, immediately
/// after `decode_release`, by `kube::helm::read_release` -- the raw
/// `release: &Value` argument is never retained by the caller past this
/// call.
///
/// **Notes are deliberately excluded**: `info.notes` is freeform chart-
/// author text (NOTES.txt), and real-world charts commonly interpolate
/// generated credentials directly into it (e.g. an auto-created
/// database password echoed back to the installer at install time).
/// Unlike `values`/`manifest`, there is no structured key to redact by
/// -- it is arbitrary prose -- so no robust, general sanitization
/// contract exists for it. Per this document's own security review, the
/// safe choice is to omit it entirely rather than risk a false sense of
/// safety from a heuristic (e.g. regex-scrubbing "password:") that a
/// differently-worded chart could trivially evade.
pub fn sanitize(release: &Value) -> HelmReleaseView {
    HelmReleaseView {
        name: release
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string(),
        namespace: release
            .get("namespace")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string(),
        revision: release.get("version").and_then(Value::as_i64),
        status: release
            .pointer("/info/status")
            .and_then(Value::as_str)
            .unwrap_or("(not reported)")
            .to_string(),
        chart_name: release
            .pointer("/chart/metadata/name")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string(),
        chart_version: release
            .pointer("/chart/metadata/version")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string(),
        app_version: release
            .pointer("/chart/metadata/appVersion")
            .and_then(Value::as_str)
            .map(str::to_string),
        values: redacted_values(release),
        resources: manifest_resources(release),
    }
}

/// Pure rendering of an already-sanitized view -- never touches a raw
/// release record, cannot leak what it was never given.
pub fn status_report(view: &HelmReleaseView) -> String {
    let mut out = format!("HELM RELEASE: {} ({})\n\n", view.name, view.namespace);
    out.push_str(&format!(
        "revision: {}\n",
        view.revision
            .map_or("(not reported)".into(), |r| r.to_string())
    ));
    out.push_str(&format!("status: {}\n", view.status));
    out.push_str(&format!(
        "chart: {}-{}\n",
        view.chart_name, view.chart_version
    ));
    if let Some(app_version) = &view.app_version {
        out.push_str(&format!("app version: {app_version}\n"));
    }

    out.push_str("\nVALUES (redacted -- sensitive-looking keys masked, never shown raw):\n");
    match serde_json::to_string_pretty(&view.values) {
        Ok(text) if view.values != Value::Null => {
            for line in text.lines() {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
        _ => out.push_str("  (none reported)\n"),
    }

    out.push_str(&format!(
        "\nMANIFEST -- owned resource identities only, never bodies ({}):\n",
        view.resources.len()
    ));
    for r in view.resources.iter().take(50) {
        let ns = r.namespace.as_deref().unwrap_or("");
        out.push_str(&format!("  - {} {}/{}\n", r.kind, ns, r.name));
    }
    if view.resources.len() > 50 {
        out.push_str(&format!(
            "  ... {} more (bounded preview)\n",
            view.resources.len() - 50
        ));
    }

    out.push_str(
        "\nNotes are deliberately never shown here: NOTES.txt is freeform chart-author\n\
         text that commonly interpolates generated secrets (e.g. an auto-created\n\
         password echoed back to the installer) -- unlike structured values/manifest\n\
         fields, there is no reliable key-based way to redact arbitrary prose, so this\n\
         view omits it entirely rather than risk leaking one. This is Helm's own\n\
         reported release record; SAURON does not compute a second health score here.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;

    fn encode_release(release_json: &Value) -> Value {
        let json_bytes = serde_json::to_vec(release_json).unwrap();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&json_bytes).unwrap();
        let gzip_bytes = encoder.finish().unwrap();
        let engine = base64::engine::general_purpose::STANDARD;
        let inner_b64 = engine.encode(&gzip_bytes);
        let outer_b64 = engine.encode(inner_b64.as_bytes());
        serde_json::json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "type": RELEASE_SECRET_TYPE,
            "metadata": {"name": "sh.helm.release.v1.demo.v1", "namespace": "sauron-m9"},
            "data": {"release": outer_b64},
        })
    }

    fn sample_release() -> Value {
        serde_json::json!({
            "name": "demo",
            "namespace": "sauron-m9",
            "version": 1,
            "info": {
                "status": "deployed",
                "notes": "Your generated password is: hunter2-notes-secret",
            },
            "chart": {"metadata": {"name": "nginx", "version": "1.2.3", "appVersion": "1.25"}},
            "config": {"replicaCount": 1, "auth": {"password": "hunter2"}, "mode": "plainValue"},
            "manifest": "apiVersion: v1\nkind: Service\nmetadata:\n  name: demo-svc\n  namespace: sauron-m9\n---\napiVersion: v1\nkind: Secret\nmetadata:\n  name: demo-secret\n  namespace: sauron-m9\ndata:\n  password: aHVudGVyMg==\n",
        })
    }

    #[test]
    fn decode_release_round_trips_a_real_double_base64_gzip_record() {
        let storage = encode_release(&sample_release());
        let decoded = decode_release(&storage).expect("decode");
        assert_eq!(decoded["name"], "demo");
        assert_eq!(decoded["version"], 1);
    }

    #[test]
    fn decode_release_reports_each_failure_mode_distinctly_never_partial_garbage() {
        assert_eq!(
            decode_release(&serde_json::json!({"data": {}})),
            Err(DecodeError::MissingField)
        );
        assert_eq!(
            decode_release(&serde_json::json!({"data": {"release": "not-base64!!!"}})),
            Err(DecodeError::InvalidEncoding)
        );
        let engine = base64::engine::general_purpose::STANDARD;
        let garbage_gzip = engine.encode(engine.encode(b"not gzip data"));
        assert_eq!(
            decode_release(&serde_json::json!({"data": {"release": garbage_gzip}})),
            Err(DecodeError::InvalidOrOversizedCompression)
        );
    }

    #[test]
    fn decode_release_refuses_a_decompression_bomb_beyond_the_bounded_cap() {
        let huge = vec![0u8; MAX_DECOMPRESSED_BYTES + 1024];
        let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&huge).unwrap();
        let gzip_bytes = encoder.finish().unwrap();
        let engine = base64::engine::general_purpose::STANDARD;
        let inner_b64 = engine.encode(&gzip_bytes);
        let outer_b64 = engine.encode(inner_b64.as_bytes());
        let storage = serde_json::json!({"data": {"release": outer_b64}});
        assert_eq!(
            decode_release(&storage),
            Err(DecodeError::InvalidOrOversizedCompression)
        );
    }

    #[test]
    fn sanitize_masks_sensitive_keys_at_any_depth_never_the_rest() {
        let release = sample_release();
        let view = sanitize(&release);
        assert_eq!(view.values["auth"]["password"], "<redacted>");
        assert_eq!(view.values["mode"], "plainValue");
        assert_eq!(view.values["replicaCount"], 1);
    }

    #[test]
    fn sanitize_extracts_manifest_identity_only_never_secret_data() {
        let release = sample_release();
        let view = sanitize(&release);
        assert_eq!(view.resources.len(), 2);
        let service = view.resources.iter().find(|r| r.kind == "Service").unwrap();
        assert_eq!(service.name, "demo-svc");
        let secret = view.resources.iter().find(|r| r.kind == "Secret").unwrap();
        assert_eq!(secret.name, "demo-secret");
        assert_eq!(secret.namespace.as_deref(), Some("sauron-m9"));
    }

    #[test]
    fn sanitize_never_carries_notes_into_the_view() {
        let release = sample_release();
        let view = sanitize(&release);
        // HelmReleaseView has no `notes` field at all -- this is a
        // compile-time guarantee, not just a runtime check. The runtime
        // check below is a defense-in-depth restatement.
        let debug = format!("{view:?}");
        assert!(!debug.contains("hunter2-notes-secret"));
    }

    #[test]
    fn status_report_never_contains_the_raw_secret_value_or_notes() {
        let view = sanitize(&sample_release());
        let report = status_report(&view);
        assert!(!report.contains("hunter2"));
        assert!(report.contains("<redacted>"));
        assert!(report.contains("plainValue"));
        assert!(
            report.contains("Service /sauron-m9/demo-svc")
                || report.contains("Service sauron-m9/demo-svc")
        );
        assert!(report.contains("Notes are deliberately never shown"));
    }

    #[test]
    fn status_report_shows_an_explicit_state_when_values_are_absent() {
        let mut release = sample_release();
        release.as_object_mut().unwrap().remove("config");
        let view = sanitize(&release);
        let report = status_report(&view);
        assert!(report.contains("(none reported)"));
    }
}
