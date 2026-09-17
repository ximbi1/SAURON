use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub namespace: Option<String>,
    pub resource: String,
    pub readonly: bool,
    pub theme: String,
    pub max_objects: usize,
    pub max_bytes: usize,
    pub request_timeout_secs: u64,
    pub aliases: BTreeMap<String, String>,
    pub favorite_namespaces: Vec<String>,
    pub keys: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            namespace: None,
            // A product default is an explicit core resource, not user alias input.
            // metrics.k8s.io also serves a plural named pods.
            resource: "v1/pods".into(),
            readonly: true,
            theme: "ember".into(),
            max_objects: 20_000,
            max_bytes: 128 * 1024 * 1024,
            request_timeout_secs: 10,
            aliases: BTreeMap::new(),
            favorite_namespaces: Vec::new(),
            keys: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(flatten)]
    pub base: Settings,
    pub clusters: BTreeMap<String, toml::Value>,
    pub contexts: BTreeMap<String, toml::Value>,
}

pub fn directory() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join(crate::brand::CONFIG_DIR)
}

impl Config {
    pub fn load(path: Option<&std::path::Path>) -> Result<Self> {
        let default = directory().join("config.toml");
        let path = path.unwrap_or(&default);
        let contents = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && path == default => {
                return Ok(Self::default());
            }
            Err(e) => return Err(e).context("cannot read application configuration"),
        };
        // Errors deliberately omit the TOML source because it can contain credentials.
        let value: toml::Value = toml::from_str(&contents).map_err(|_| {
            anyhow::anyhow!("invalid TOML configuration (source omitted for privacy)")
        })?;
        let mut root = value
            .as_table()
            .cloned()
            .context("configuration must be a TOML table")?;
        let clusters = root
            .remove("clusters")
            .map(|v| v.try_into())
            .transpose()
            .map_err(|_| anyhow::anyhow!("invalid clusters configuration"))?
            .unwrap_or_default();
        let contexts = root
            .remove("contexts")
            .map(|v| v.try_into())
            .transpose()
            .map_err(|_| anyhow::anyhow!("invalid contexts configuration"))?
            .unwrap_or_default();
        let base = toml::Value::Table(root)
            .try_into()
            .map_err(|_| anyhow::anyhow!("unknown or invalid application setting"))?;
        let config = Self {
            base,
            clusters,
            contexts,
        };
        config.resolve("", "")?;
        Ok(config)
    }

    pub fn resolve(&self, cluster: &str, context: &str) -> Result<Settings> {
        // Serialize via explicit fields so overrides retain absent-vs-default semantics.
        let mut value = toml::Value::try_from(serde_json::json!({
            "resource": self.base.resource, "readonly": self.base.readonly, "theme":self.base.theme,
            "max_objects":self.base.max_objects,"max_bytes":self.base.max_bytes,
            "request_timeout_secs":self.base.request_timeout_secs,"aliases":self.base.aliases,
            "favorite_namespaces":self.base.favorite_namespaces,"keys":self.base.keys
        }))?;
        if let Some(ns) = &self.base.namespace {
            value["namespace"] = toml::Value::String(ns.clone());
        }
        for layer in [self.clusters.get(cluster), self.contexts.get(context)]
            .into_iter()
            .flatten()
        {
            merge(&mut value, layer.clone());
        }
        let settings: Settings = value
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid cluster/context settings"))?;
        if !(1..=100_000).contains(&settings.max_objects)
            || !(1_048_576..=1_073_741_824).contains(&settings.max_bytes)
        {
            bail!("max_objects must be 1..100000 and max_bytes 1 MiB..1 GiB");
        }
        if !(1..=120).contains(&settings.request_timeout_secs) {
            bail!("request_timeout_secs must be 1..120");
        }
        if !["ember", "light", "mono"].contains(&settings.theme.as_str()) {
            bail!("theme must be ember, light or mono");
        }
        Ok(settings)
    }
}

fn merge(base: &mut toml::Value, layer: toml::Value) {
    match (base, layer) {
        (toml::Value::Table(a), toml::Value::Table(b)) => {
            for (k, v) in b {
                if let Some(old) = a.get_mut(&k) {
                    merge(old, v);
                } else {
                    a.insert(k, v);
                }
            }
        }
        (base, v) => *base = v,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_resource_is_explicit_core_identity_not_ambiguous_alias() {
        assert_eq!(Settings::default().resource, "v1/pods");
    }
    #[test]
    fn context_overrides_cluster_and_maps_merge() {
        let mut c = Config::default();
        c.base.aliases.insert("p".into(), "pods".into());
        c.clusters.insert(
            "cluster".into(),
            toml::from_str("theme='light'\n[aliases]\nd='deployments'").expect("fixture"),
        );
        c.contexts.insert(
            "team/prod".into(),
            toml::from_str("theme='mono'").expect("fixture"),
        );
        let resolved = c.resolve("cluster", "team/prod").expect("valid");
        assert_eq!(resolved.theme, "mono");
        assert_eq!(resolved.aliases.len(), 2);
    }
}
