use crate::app::{bookmark::Bookmark, workspace::Workspace};
use crate::plugin::PluginConfig;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

/// M10.8: distinguishes "no version field present" (0 -- every pre-M10
/// `config.toml` on disk, and a freshly-`Default`-constructed `Config`)
/// from "this file was written by an M10-aware build" (`CONFIG_VERSION`).
/// Every field this milestone added has its own safe `#[serde(default)]`
/// already, so loading never actually branches on this number today --
/// it exists so a FUTURE change that alters a field's *meaning* (not
/// just adds one) has something concrete to check, matching
/// `mutation::journal::SCHEMA_VERSION`'s own "exists for the next
/// migration, not this one" precedent.
pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// M12.1: keyed by plugin name. Absent/malformed entries never crash
    /// startup (same `deny_unknown_fields`-per-entry/fail-safe-reload
    /// discipline `keys`/`theme` already established) -- see
    /// `plugin::PluginConfig`'s own doc comment for the trust contract.
    /// Default `Trust::Disabled` means a plugin merely *listed* here does
    /// not run; it must be explicitly set to `trust = "approved"`.
    pub plugins: BTreeMap<String, PluginConfig>,
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
            plugins: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Absent (0) on load means either a genuine pre-M10 file or a
    /// fresh `Default`; always written as `CONFIG_VERSION` on save. See
    /// `CONFIG_VERSION`'s own doc comment.
    #[serde(default)]
    pub version: u32,
    #[serde(flatten)]
    pub base: Settings,
    pub clusters: BTreeMap<String, toml::Value>,
    pub contexts: BTreeMap<String, toml::Value>,
    /// M10.4/M10.8: session-captured navigation views, keyed by name.
    /// Never contains anything mutation-authorization-shaped -- see
    /// `app::workspace::Workspace`'s own doc comment for why that's a
    /// structural guarantee, not just a convention honored here.
    #[serde(default)]
    pub workspaces: BTreeMap<String, Workspace>,
    /// M10.5/M10.8: bookmarked object references, keyed by name. Same
    /// "never an identity-authority token" guarantee as `workspaces`.
    #[serde(default)]
    pub bookmarks: BTreeMap<String, Bookmark>,
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
        // M10.8: absent on a pre-M10 file -- 0 via #[serde(default)], the
        // exact "no version present" case `CONFIG_VERSION`'s own doc
        // comment describes.
        let version = root
            .remove("version")
            .map(|v| v.try_into())
            .transpose()
            .map_err(|_| anyhow::anyhow!("invalid version field"))?
            .unwrap_or_default();
        let workspaces = root
            .remove("workspaces")
            .map(|v| v.try_into())
            .transpose()
            .map_err(|_| anyhow::anyhow!("invalid workspaces configuration"))?
            .unwrap_or_default();
        let bookmarks = root
            .remove("bookmarks")
            .map(|v| v.try_into())
            .transpose()
            .map_err(|_| anyhow::anyhow!("invalid bookmarks configuration"))?
            .unwrap_or_default();
        let base = toml::Value::Table(root)
            .try_into()
            .map_err(|_| anyhow::anyhow!("unknown or invalid application setting"))?;
        let config = Self {
            version,
            base,
            clusters,
            contexts,
            workspaces,
            bookmarks,
        };
        config.resolve("", "")?;
        Ok(config)
    }
    /// M10.8: the same default path `load()` itself uses when no
    /// explicit path is given -- exposed so `save()` (and its caller,
    /// which needs to know where an explicit `--config` path was NOT
    /// given) can target the identical file without duplicating this
    /// logic.
    pub fn default_path() -> PathBuf {
        directory().join("config.toml")
    }
    /// M10.8: the first *write* path to this file (every prior milestone
    /// only read it). Atomic: serializes to a temp file in the same
    /// directory, sets conservative permissions, then renames over the
    /// real path -- a crash or power loss mid-write leaves either the
    /// old file intact or the new one complete, never a half-written
    /// `config.toml`. `version` is always stamped to the current
    /// `CONFIG_VERSION` on every save, regardless of what was loaded.
    pub fn save(&self, path: Option<&std::path::Path>) -> Result<()> {
        let default = Self::default_path();
        let path = path.unwrap_or(&default);
        let dir = path
            .parent()
            .context("config path has no parent directory")?;
        std::fs::create_dir_all(dir).context("cannot create configuration directory")?;
        let mut to_write = self.clone();
        to_write.version = CONFIG_VERSION;
        let text = toml::to_string_pretty(&to_write)
            .context("cannot serialize configuration (this is a bug, not a user config error)")?;
        let temp_path = dir.join(format!(
            ".config.toml.tmp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::write(&temp_path, &text).context("cannot write temporary configuration file")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Conservative, consistent with credentials-adjacent file
            // handling elsewhere -- this file can contain cluster/context
            // names and (via workspaces/bookmarks) internal cluster
            // topology, even though it never contains credentials
            // themselves.
            let _ = std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&temp_path, path)
            .context("cannot atomically replace configuration file")?;
        Ok(())
    }

    pub fn resolve(&self, cluster: &str, context: &str) -> Result<Settings> {
        // Serialize via explicit fields so overrides retain absent-vs-default semantics.
        let mut value = toml::Value::try_from(serde_json::json!({
            "resource": self.base.resource, "readonly": self.base.readonly, "theme":self.base.theme,
            "max_objects":self.base.max_objects,"max_bytes":self.base.max_bytes,
            "request_timeout_secs":self.base.request_timeout_secs,"aliases":self.base.aliases,
            "favorite_namespaces":self.base.favorite_namespaces,"keys":self.base.keys,
            "plugins":self.base.plugins
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
        // M10.7: deliberately NOT validated here (unlike max_objects/
        // max_bytes/request_timeout_secs above, which are genuine
        // resource-safety bounds worth a hard startup failure) -- an
        // invalid theme name is purely cosmetic. `ui::Theme::named()`
        // already falls back to the default look for any unrecognized
        // name, and `State::new` surfaces a visible, non-fatal warning
        // for it, mirroring the same fail-safe pattern already
        // established for a malformed keymap config (M10.6). A typo in
        // `theme` must never be able to crash the whole application at
        // startup the way it used to (this `bail!` did exactly that).
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
    #[test]
    fn base_level_plugins_flow_through_resolve_default_trust_is_disabled() {
        let mut c = Config::default();
        c.base.plugins.insert(
            "sanitize".into(),
            crate::plugin::PluginConfig {
                executable: "/usr/local/bin/sanitize".into(),
                args: vec!["--format".into(), "json".into()],
                trust: crate::plugin::Trust::Disabled,
                timeout_secs: 10,
            },
        );
        let resolved = c.resolve("", "").expect("valid");
        let plugin = resolved
            .plugins
            .get("sanitize")
            .expect("base plugin present");
        assert_eq!(plugin.executable, "/usr/local/bin/sanitize");
        assert_eq!(plugin.trust, crate::plugin::Trust::Disabled);
    }
    #[test]
    fn a_context_layer_can_approve_a_plugin_the_base_left_disabled() {
        let mut c = Config::default();
        c.base.plugins.insert(
            "sanitize".into(),
            crate::plugin::PluginConfig {
                executable: "/usr/local/bin/sanitize".into(),
                args: vec![],
                trust: crate::plugin::Trust::Disabled,
                timeout_secs: 10,
            },
        );
        c.contexts.insert(
            "kind-sauron-test".into(),
            toml::from_str(
                "[plugins.sanitize]\nexecutable='/usr/local/bin/sanitize'\ntrust='approved'",
            )
            .expect("fixture"),
        );
        let resolved = c.resolve("", "kind-sauron-test").expect("valid");
        assert_eq!(
            resolved.plugins.get("sanitize").unwrap().trust,
            crate::plugin::Trust::Approved
        );
        // The base (no context) resolution must stay Disabled -- approval
        // is per-context, never globally implied by one context's config.
        let base_only = c.resolve("", "").expect("valid");
        assert_eq!(
            base_only.plugins.get("sanitize").unwrap().trust,
            crate::plugin::Trust::Disabled
        );
    }
    #[test]
    fn an_unrecognized_theme_never_fails_resolution_startup_must_not_crash_on_a_typo() {
        let mut c = Config::default();
        c.base.theme = "not-a-real-theme".into();
        let resolved = c.resolve("", "").expect(
            "a bad theme name must never fail resolution -- see this function's own comment",
        );
        assert_eq!(resolved.theme, "not-a-real-theme");
    }
    #[test]
    fn resource_bounds_still_hard_fail_unlike_the_purely_cosmetic_theme() {
        let mut c = Config::default();
        c.base.max_objects = 0;
        assert!(
            c.resolve("", "").is_err(),
            "genuine resource-safety bounds must remain a hard startup failure"
        );
    }
    fn scratch_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "sauron-config-test-{name}-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ))
    }
    fn workspace(name: &str) -> Workspace {
        Workspace {
            schema_version: 1,
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
    fn save_then_load_round_trips_workspaces_and_bookmarks_exactly() {
        let path = scratch_path("roundtrip");
        let mut c = Config::default();
        c.workspaces.insert("w1".into(), workspace("w1"));
        c.bookmarks.insert(
            "b1".into(),
            Bookmark {
                schema_version: 1,
                name: "b1".into(),
                context: "kind-sauron-test".into(),
                resource: "v1/pods".into(),
                namespace: "sauron-fixtures".into(),
                object_name: "web-1".into(),
                uid: "uid-1".into(),
            },
        );
        c.save(Some(&path)).expect("save");
        let loaded = Config::load(Some(&path)).expect("load");
        assert_eq!(loaded.version, CONFIG_VERSION);
        assert_eq!(loaded.workspaces.get("w1"), c.workspaces.get("w1"));
        assert_eq!(loaded.bookmarks.get("b1"), c.bookmarks.get("b1"));
        std::fs::remove_file(&path).ok();
    }
    #[test]
    fn a_pre_m10_config_file_with_no_new_fields_still_loads_with_empty_defaults() {
        let path = scratch_path("pre-m10");
        // Exactly the shape a pre-M10 config.toml has -- no version,
        // workspaces, or bookmarks keys at all.
        std::fs::write(&path, "theme = 'light'\nreadonly = false\n").expect("write fixture");
        let loaded = Config::load(Some(&path)).expect("a pre-M10 file must still load");
        assert_eq!(
            loaded.version, 0,
            "no version present is the documented pre-M10 signal"
        );
        assert!(loaded.workspaces.is_empty());
        assert!(loaded.bookmarks.is_empty());
        assert_eq!(loaded.base.theme, "light");
        std::fs::remove_file(&path).ok();
    }
    #[test]
    fn save_is_atomic_and_survives_being_called_repeatedly() {
        let path = scratch_path("atomic");
        let mut c = Config::default();
        for i in 0..5 {
            c.workspaces
                .insert(i.to_string(), workspace(&i.to_string()));
            c.save(Some(&path)).expect("save");
        }
        let loaded = Config::load(Some(&path)).expect("load");
        assert_eq!(loaded.workspaces.len(), 5);
        // No leftover temp files in the same directory.
        let dir = path.parent().unwrap();
        let leftover: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .contains(&path.file_name().unwrap().to_string_lossy().to_string())
                    && e.file_name().to_string_lossy().contains(".tmp-")
            })
            .collect();
        assert!(
            leftover.is_empty(),
            "no temp file should survive a successful save: {leftover:?}"
        );
        std::fs::remove_file(&path).ok();
    }
    #[test]
    fn a_genuinely_unknown_top_level_field_is_still_a_load_error() {
        let path = scratch_path("unknown-field");
        std::fs::write(&path, "this_field_does_not_exist = true\n").expect("write fixture");
        assert!(
            Config::load(Some(&path)).is_err(),
            "deny_unknown_fields must still reject a genuinely unrecognized field, \
             not silently ignore it"
        );
        std::fs::remove_file(&path).ok();
    }
}
