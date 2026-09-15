//! Data leaving transport is untrusted. No mutations are exposed in this release.
use serde_json::Value;

pub const READONLY: bool = true;

pub fn redact(value: &mut Value) {
    let secret = value.get("kind").and_then(Value::as_str) == Some("Secret");
    if let Some(map) = value.as_object_mut() {
        if secret {
            for key in ["data", "stringData"] {
                if let Some(values) = map.get_mut(key).and_then(Value::as_object_mut) {
                    for v in values.values_mut() {
                        *v = Value::String("<redacted>".into());
                    }
                }
            }
        }
        for (key, v) in map.iter_mut() {
            let lower = key.to_ascii_lowercase();
            if lower == "managedfields" {
                *v = Value::Null;
            } else if [
                "token",
                "password",
                "credential",
                "private-key",
                "apikey",
                "api-key",
                "last-applied-configuration",
            ]
            .iter()
            .any(|s| lower.contains(s))
            {
                *v = Value::String("<redacted>".into());
            } else {
                redact(v);
            }
        }
    } else if let Some(values) = value.as_array_mut() {
        for v in values {
            redact(v);
        }
    }
}

/// Strip terminal controls, including escape introducers, from all external strings.
/// ESC sequences become inert visible text. Never emit OSC/CSI from cluster data.
pub fn text(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// Do not render arbitrary server error bodies or credential-plugin stderr.
pub fn api_error(error: &::kube::Error, operation: &str) -> String {
    match error {
        ::kube::Error::Api(e) => {
            let reason = match e.code {
                401 => "Unauthorized: check the configured credentials",
                403 => "Forbidden: the current identity is not permitted",
                404 => "Not found: object or API is unavailable",
                409 => "Conflict: resource changed; refresh and retry",
                410 => "Resource version expired; relisting",
                429 => "API rate limit; retrying with backoff",
                500..=599 => "API server unavailable",
                _ => "API rejected the request",
            };
            format!("{reason} ({}) while {}", e.code, text(operation))
        }
        _ => format!(
            "Transport/authentication error while {}; check connectivity, TLS and credential plugin configuration",
            text(operation)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_and_embedded_last_applied_are_redacted() {
        let mut v = serde_json::json!({"kind":"Secret","data":{"a":"SECRETVALUE"},"metadata":{"annotations":{"kubectl.kubernetes.io/last-applied-configuration":"SECRETVALUE"}}});
        redact(&mut v);
        assert!(!v.to_string().contains("SECRETVALUE"));
        assert_eq!(text("x\x1b]52;c;payload\x07\r\ny"), "x]52;c;payload\ny");
    }
}
