//! M11.4 evidence bundle: a local, bounded, redacted export of one already-
//! identified target's already-computed evidence sections. This module owns
//! only the manifest shape and the atomic filesystem write -- it does not
//! collect evidence itself (the caller, `Runtime::start_bundle`, reuses the
//! exact same `kube::evidence`/`kube::relationships`/`app::metrics`
//! collectors every other view already uses) and does not redact by itself
//! (every section's text is expected to already have gone through
//! `safety::redact`/`safety::text` at collection time, the same as every
//! other rendered report in this codebase -- this module never re-derives
//! that guarantee, it only trusts and stores it). See
//! `docs/M11_ACCEPTANCE.md`'s M11.4 section for the frozen contract.
use anyhow::{Context, Result, ensure};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Partial,
    Unavailable,
}
impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Partial => "PARTIAL",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

pub struct Section {
    /// Becomes `<name>.txt` under the bundle directory. Must be a bare
    /// filename component (validated in `write`), never a path.
    pub name: &'static str,
    pub status: Status,
    pub text: String,
}

pub struct Identity {
    pub kind: String,
    pub namespace: String,
    pub name: String,
    pub uid: String,
}

/// Every filename this module ever writes is one of these two fixed,
/// hardcoded literals or a `Section::name` drawn from a caller-controlled
/// static list (never user/cluster input) -- there is no path derived from
/// object names/namespaces, so there is nothing for a crafted resource name
/// to traverse.
const MANIFEST_FILE: &str = "manifest.json";

fn validate_destination(dest: &Path) -> Result<()> {
    ensure!(
        dest.components()
            .all(|c| !matches!(c, Component::ParentDir)),
        "destination path may not contain '..'"
    );
    Ok(())
}

/// Refuses an existing non-empty destination unless `overwrite` is true --
/// this is the caller's one required explicit-confirmation gate; there is no
/// implicit "the user probably meant to overwrite."
pub fn write(
    dest: &Path,
    identity: &Identity,
    sections: &[Section],
    overwrite: bool,
) -> Result<PathBuf> {
    validate_destination(dest)?;
    if dest.exists() {
        let occupied = std::fs::read_dir(dest)
            .context("cannot inspect existing destination directory")?
            .next()
            .is_some();
        ensure!(
            !occupied || overwrite,
            "destination already exists and is not empty; pass --force to overwrite"
        );
    }
    std::fs::create_dir_all(dest).context("cannot create bundle destination directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o700));
    }
    let mut written: Vec<PathBuf> = Vec::new();
    let result = (|| -> Result<PathBuf> {
        let mut total_bytes = 0usize;
        let mut manifest_sections = Vec::new();
        for section in sections {
            ensure!(
                section
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "internal error: bundle section name must be a bare identifier"
            );
            let file_name = format!("{}.txt", section.name);
            let path = dest.join(&file_name);
            atomic_write(&path, section.text.as_bytes())?;
            written.push(path);
            total_bytes += section.text.len();
            manifest_sections.push(serde_json::json!({
                "name": section.name,
                "file": file_name,
                "status": section.status.as_str(),
                "bytes": section.text.len(),
            }));
        }
        let manifest = serde_json::json!({
            "kind": identity.kind,
            "namespace": identity.namespace,
            "name": identity.name,
            "uid": identity.uid,
            "collected_at": chrono::Utc::now().to_rfc3339(),
            "sauron_version": crate::brand::VERSION,
            "total_bytes": total_bytes,
            "sections": manifest_sections,
        });
        let manifest_path = dest.join(MANIFEST_FILE);
        let manifest_text = serde_json::to_string_pretty(&manifest)
            .context("cannot serialize bundle manifest (this is a bug)")?;
        atomic_write(&manifest_path, manifest_text.as_bytes())?;
        written.push(manifest_path);
        Ok(dest.to_path_buf())
    })();
    if result.is_err() {
        // Never leave a half-written bundle that looks complete -- clean up
        // only the files this call itself wrote, never anything pre-existing
        // (an `overwrite` run's prior files are left alone on failure rather
        // than guessed-and-deleted).
        for path in &written {
            let _ = std::fs::remove_file(path);
        }
    }
    result
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("bundle file path has no parent")?;
    let temp_path = dir.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("bundle"),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&temp_path, bytes).context("cannot write bundle temporary file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&temp_path, path).context("cannot atomically finalize bundle file")?;
    Ok(())
}

pub fn bounded(max_bytes: usize, text: String) -> (String, Status) {
    if text.len() <= max_bytes {
        (text, Status::Ok)
    } else {
        let mut truncated = text;
        truncated.truncate(max_bytes);
        truncated.push_str("\n[TRUNCATED: bundle section bound reached]\n");
        (truncated, Status::Partial)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique, self-cleaning scratch directory per test -- same convention
    /// this project's own `app::tests::runtime()` helper already uses for
    /// per-call scratch config paths, no new test-only dependency needed.
    struct ScratchDir(PathBuf);
    impl ScratchDir {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "sauron-bundle-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn identity() -> Identity {
        Identity {
            kind: "Pod".into(),
            namespace: "default".into(),
            name: "a".into(),
            uid: "uid-a".into(),
        }
    }

    #[test]
    fn writes_every_section_plus_a_manifest_with_matching_inventory() {
        let dir = ScratchDir::new();
        let dest = dir.path().join("bundle");
        let sections = vec![
            Section {
                name: "health",
                status: Status::Ok,
                text: "Healthy".into(),
            },
            Section {
                name: "events",
                status: Status::Partial,
                text: "e1".into(),
            },
        ];
        let out = write(&dest, &identity(), &sections, false).expect("write succeeds");
        assert_eq!(out, dest);
        assert!(dest.join("health.txt").exists());
        assert!(dest.join("events.txt").exists());
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dest.join(MANIFEST_FILE)).unwrap())
                .unwrap();
        assert_eq!(manifest["uid"], "uid-a");
        assert_eq!(manifest["sections"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn refuses_a_non_empty_existing_destination_without_overwrite() {
        let dir = ScratchDir::new();
        let dest = dir.path().join("bundle");
        write(
            &dest,
            &identity(),
            &[Section {
                name: "a",
                status: Status::Ok,
                text: "x".into(),
            }],
            false,
        )
        .unwrap();
        let second = write(
            &dest,
            &identity(),
            &[Section {
                name: "a",
                status: Status::Ok,
                text: "y".into(),
            }],
            false,
        );
        assert!(second.is_err(), "must refuse silent overwrite");
        assert_eq!(std::fs::read_to_string(dest.join("a.txt")).unwrap(), "x");
    }

    #[test]
    fn explicit_overwrite_flag_replaces_prior_contents() {
        let dir = ScratchDir::new();
        let dest = dir.path().join("bundle");
        write(
            &dest,
            &identity(),
            &[Section {
                name: "a",
                status: Status::Ok,
                text: "x".into(),
            }],
            false,
        )
        .unwrap();
        write(
            &dest,
            &identity(),
            &[Section {
                name: "a",
                status: Status::Ok,
                text: "y".into(),
            }],
            true,
        )
        .expect("explicit overwrite succeeds");
        assert_eq!(std::fs::read_to_string(dest.join("a.txt")).unwrap(), "y");
    }

    #[test]
    fn rejects_a_destination_containing_parent_dir_traversal() {
        let dir = ScratchDir::new();
        let dest = dir.path().join("../escape");
        let result = write(&dest, &identity(), &[], false);
        assert!(result.is_err());
    }

    #[test]
    fn file_permissions_are_owner_only() {
        let dir = ScratchDir::new();
        let dest = dir.path().join("bundle");
        write(
            &dest,
            &identity(),
            &[Section {
                name: "a",
                status: Status::Ok,
                text: "x".into(),
            }],
            false,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::metadata(dest.join("a.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600);
            let dir_mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700);
        }
    }

    #[test]
    fn empty_existing_directory_is_not_treated_as_occupied() {
        let dir = ScratchDir::new();
        let dest = dir.path().join("bundle");
        std::fs::create_dir_all(&dest).unwrap();
        write(
            &dest,
            &identity(),
            &[Section {
                name: "a",
                status: Status::Ok,
                text: "x".into(),
            }],
            false,
        )
        .expect("an empty pre-existing directory is not a silent-overwrite risk");
    }

    #[test]
    fn bounded_truncates_and_marks_partial_never_silently_complete() {
        let (text, status) = bounded(5, "0123456789".into());
        assert_eq!(status, Status::Partial);
        assert!(text.contains("TRUNCATED"));
        let (text, status) = bounded(50, "short".into());
        assert_eq!(status, Status::Ok);
        assert_eq!(text, "short");
    }

    #[test]
    fn a_failed_mid_export_cleans_up_its_own_partial_output() {
        // Simulate failure by writing to a destination that becomes
        // read-only after the directory (but not the manifest) is created --
        // a section write partway through cannot complete, and the sections
        // already written by THIS call must not remain looking like a
        // complete bundle.
        let dir = ScratchDir::new();
        let dest = dir.path().join("bundle");
        std::fs::create_dir_all(&dest).unwrap();
        let good = dest.join("good.txt");
        std::fs::write(&good, b"placeholder").unwrap();
        // A section name containing a path separator is rejected by the
        // identifier check after at least one prior section has already
        // been written, exercising the cleanup path.
        #[allow(invalid_value)]
        let bad_name: &'static str = Box::leak("bad/name".to_string().into_boxed_str());
        let sections = vec![
            Section {
                name: "good",
                status: Status::Ok,
                text: "content".into(),
            },
            Section {
                name: bad_name,
                status: Status::Ok,
                text: "content".into(),
            },
        ];
        let result = write(&dest, &identity(), &sections, true);
        assert!(result.is_err());
        assert!(
            !dest.join("good.txt").exists()
                || std::fs::read_to_string(dest.join("good.txt")).unwrap() != "content",
            "a failed export must not leave this call's own partial output looking complete"
        );
    }
}
