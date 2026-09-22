//! M12.1: the first local-subprocess boundary this codebase has ever had.
//! `kube::exec`/`kube::shell` proxy through the Kubernetes exec WebSocket
//! API; this module is genuinely new architecture, not composition -- see
//! `docs/M12_ACCEPTANCE.md`'s own reconnaissance section for why that is
//! expected here and nowhere else in M12. Task ownership/cancellation is
//! still fully composed: callers reuse `app::session::Sessions` verbatim
//! (`Kind::Plugin`), so "no orphan process after quit" falls out of
//! `Sessions`'s own existing `Drop`/`shutdown` guarantees, not new code.
//!
//! EXTENSIBILITY != UNBOUNDED TRUST: a plugin is an explicit, user-approved
//! executable + fixed argv template, never an ad-hoc shell string. No
//! `sh -c`/`bash -c`/`eval`/shell interpolation anywhere in this module.
//! No credential (kubeconfig path/content, bearer token, exec-plugin
//! credential output, raw Secret value) ever reaches a child process --
//! the environment is an explicit allowlist, never the parent's full
//! environment.
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// A plugin either has been explicitly approved by editing the config file
/// (the same trust gesture `readonly = false` already requires for
/// mutations) or it does not run at all. There is deliberately no
/// "untrusted but runs with a warning" state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Trust {
    Approved,
    #[default]
    Disabled,
}

fn default_timeout_secs() -> u64 {
    10
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginConfig {
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub trust: Trust,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

/// Matches `kube::logs`'s own established per-line clip bound exactly --
/// one bounded-output convention project-wide, not a second one invented
/// here.
const MAX_LINE_BYTES: usize = 16 * 1024;
/// Independent of line length: a plugin that prints millions of short
/// lines must not grow memory unbounded either.
const MAX_LINES: usize = 500;
/// Never the parent's full environment. No KUBECONFIG, no token, no
/// credential-plugin output, no Secret value -- ever, by default.
const ENV_ALLOWLIST: &[&str] = &["PATH", "HOME", "LANG"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Completed {
        exit_code: i32,
        stdout: Vec<String>,
        stderr: Vec<String>,
        stdout_truncated: bool,
        stderr_truncated: bool,
    },
    TimedOut,
    Cancelled,
    /// Config rejection (not approved), spawn failure (missing executable,
    /// permission denied), or an internal I/O error -- always an explicit
    /// outcome, never a crash, never a silent retry.
    Failed(String),
}

async fn capture(reader: impl tokio::io::AsyncRead + Unpin) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut truncated = false;
    let mut buffered = BufReader::new(reader);
    loop {
        let mut line = Vec::new();
        match buffered.read_until(b'\n', &mut line).await {
            Ok(0) => break,
            Ok(_) => {
                if lines.len() >= MAX_LINES {
                    truncated = true;
                    continue;
                }
                let clipped = if line.len() > MAX_LINE_BYTES {
                    truncated = true;
                    &line[..MAX_LINE_BYTES]
                } else {
                    &line[..]
                };
                lines.push(String::from_utf8_lossy(clipped).trim_end().to_string());
            }
            Err(_) => {
                truncated = true;
                break;
            }
        }
    }
    (lines, truncated)
}

#[cfg(unix)]
fn kill_process_group(pid: Option<u32>) {
    if let Some(pid) = pid {
        // Negative pid targets the whole process group `process_group(0)`
        // placed this child (and anything it itself spawns) into -- a
        // direct `child.kill()` only reaches the immediate child.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}
#[cfg(not(unix))]
fn kill_process_group(_pid: Option<u32>) {}

/// Real bug found live during M12.2's own acceptance work (see
/// `docs/M12_ACCEPTANCE.md`'s M12.1 journal entry for the full
/// root-cause): `Sessions::drop` cancels the token and immediately calls
/// `JoinSet::abort_all()` with no `.await` in between, so an aborted
/// plugin task's future is dropped in place *without ever being polled
/// again* -- the `tokio::select!` cancellation branch below, including its
/// own `kill_process_group` call, never runs on that path. Only
/// in-scope `Drop` impls fire. `tokio::process::Child`'s own
/// `kill_on_drop` reaches only the direct child, not a grandchild the
/// plugin itself forked (proven exactly this way live: a real
/// `sh -c "sleep 60 & wait"` plugin left its `sleep` grandchild running
/// after a real app quit, even though the unit-level cooperative-cancel
/// test for the same scenario passed). This guard's own `Drop` is what
/// actually has to run unconditionally on every exit path -- normal
/// completion, cooperative cancel/timeout, and abrupt task abort alike --
/// which no `tokio::select!` branch can guarantee by itself.
struct GroupKillGuard(Option<u32>);
impl Drop for GroupKillGuard {
    fn drop(&mut self) {
        kill_process_group(self.0);
    }
}

/// M12.2's own protocol input: canonical target identity plus the already-
/// redacted `Health` projection -- never a raw object body, never a
/// Secret-adjacent field, never a credential. `resources::Object::new`
/// already ran `safety::redact` on `object.value` at construction time,
/// but this function deliberately does not touch `object.value` at all,
/// so there is no redaction to trust or re-derive here: only the
/// identity/health fields ever existed to leak in the first place.
pub fn projection(object: &crate::resources::Object) -> serde_json::Value {
    serde_json::json!({
        "protocolVersion": 1,
        "target": {
            "kind": object.kind,
            "namespace": object.namespace,
            "name": object.name,
            "uid": object.uid,
        },
        "health": {
            "status": object.health.status,
            "severity": format!("{:?}", object.health.severity),
            "evidence": object.health.evidence,
        },
    })
}

/// Runs one approved plugin to completion, bounded by `config.timeout_secs`
/// and `cancel`. `input` is whatever bounded, already-redacted JSON
/// projection the caller built (`projection`, above) -- this function does
/// not inspect or redact it further; it only transports bytes.
pub async fn run(
    config: &PluginConfig,
    input: &serde_json::Value,
    cancel: CancellationToken,
) -> Status {
    if config.trust != Trust::Approved {
        return Status::Failed("plugin is not approved (trust = disabled)".into());
    }
    let mut cmd = Command::new(&config.executable);
    cmd.args(&config.args);
    cmd.env_clear();
    for key in ENV_ALLOWLIST {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    // config::directory() is only created lazily by Config::save() -- on a
    // fresh machine/CI runner where nothing has ever been saved, it does
    // not exist yet, and Command::spawn() would fail with the misleading
    // "No such file or directory" (as if the executable were missing).
    // Same create_dir_all convention Config::save() itself already uses.
    let working_dir = crate::config::directory();
    if let Err(e) = std::fs::create_dir_all(&working_dir) {
        return Status::Failed(format!("cannot create config directory: {e}"));
    }
    cmd.current_dir(&working_dir);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Status::Failed(format!("cannot start plugin: {e}")),
    };
    let pid = child.id();
    // Lives for this whole function's scope and fires on every exit path --
    // normal completion, cooperative cancel/timeout below, AND an abrupt
    // external task abort (`Sessions::drop`'s own path) that never lets
    // this function run another line of its own code. See the guard's own
    // doc comment for the real bug this closes.
    let _group_guard = GroupKillGuard(pid);
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let input_bytes = serde_json::to_vec(input).unwrap_or_default();
    let write_stdin = async move {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(&input_bytes).await;
            let _ = stdin.shutdown().await;
        }
    };

    let work = async {
        let (_, (stdout_lines, stdout_truncated), (stderr_lines, stderr_truncated)) =
            tokio::join!(write_stdin, capture(stdout), capture(stderr));
        match child.wait().await {
            Ok(status) => Status::Completed {
                exit_code: status.code().unwrap_or(-1),
                stdout: stdout_lines,
                stderr: stderr_lines,
                stdout_truncated,
                stderr_truncated,
            },
            Err(e) => Status::Failed(format!("plugin process error: {e}")),
        }
    };

    // `_group_guard`'s own `Drop` is the actual, unconditional termination
    // mechanism now (see its doc comment) -- these branches only decide
    // which `Status` to report; they no longer need to kill anything
    // themselves.
    let timeout = Duration::from_secs(config.timeout_secs.max(1));
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Status::Cancelled,
        _ = tokio::time::sleep(timeout) => Status::TimedOut,
        status = work => status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_never_includes_raw_object_body_or_secret_adjacent_fields() {
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion": "v1", "kind": "Secret",
            "metadata": {"namespace": "default", "name": "s", "uid": "u"},
            "data": {"password": "hunter2-should-never-leak"},
        }));
        let value = projection(&object);
        let dump = value.to_string();
        assert!(!dump.contains("hunter2"), "{dump}");
        assert!(!dump.contains("password"), "{dump}");
        assert_eq!(value["target"]["kind"], "Secret");
        assert_eq!(value["target"]["uid"], "u");
        assert_eq!(value["protocolVersion"], 1);
    }

    fn config(executable: &str, args: &[&str]) -> PluginConfig {
        PluginConfig {
            executable: executable.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            trust: Trust::Approved,
            timeout_secs: 5,
        }
    }

    #[tokio::test]
    async fn disabled_trust_never_spawns_anything() {
        let mut c = config("/bin/echo", &["should-not-run"]);
        c.trust = Trust::Disabled;
        let status = run(&c, &serde_json::json!({}), CancellationToken::new()).await;
        assert!(matches!(status, Status::Failed(msg) if msg.contains("not approved")));
    }

    #[tokio::test]
    async fn missing_executable_is_an_explicit_failure_never_a_panic() {
        let c = config("/definitely/not/a/real/executable-xyz", &[]);
        let status = run(&c, &serde_json::json!({}), CancellationToken::new()).await;
        assert!(matches!(status, Status::Failed(_)));
    }

    #[tokio::test]
    async fn successful_run_captures_stdout_exit_code_and_input() {
        // cat echoes stdin back to stdout -- proves input actually transports.
        let c = config("/bin/cat", &[]);
        let status = run(
            &c,
            &serde_json::json!({"hello": "world"}),
            CancellationToken::new(),
        )
        .await;
        match status {
            Status::Completed {
                exit_code, stdout, ..
            } => {
                assert_eq!(exit_code, 0);
                assert!(stdout.join("").contains("hello"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn nonzero_exit_is_reported_never_treated_as_success() {
        let c = config("/bin/sh", &["-c", "exit 7"]);
        let status = run(&c, &serde_json::json!({}), CancellationToken::new()).await;
        assert!(matches!(status, Status::Completed { exit_code: 7, .. }));
    }

    #[tokio::test]
    async fn timeout_kills_the_process_and_its_children() {
        // The outer sleep is the direct child; it spawns a grandchild
        // sleep of its own -- process-group kill must reach both, not
        // just the direct child SIGKILL would reach.
        let c = {
            let mut c = config("/bin/sh", &["-c", "sleep 30 & wait"]);
            c.timeout_secs = 1;
            c
        };
        let start = std::time::Instant::now();
        let status = run(&c, &serde_json::json!({}), CancellationToken::new()).await;
        assert_eq!(status, Status::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timeout must actually bound wall time, not wait for the 30s child"
        );
    }

    #[tokio::test]
    async fn aborting_the_task_still_kills_the_whole_process_group_not_just_the_direct_child() {
        // Reproduces the real bug found live during M12.2's own
        // acceptance work: `Sessions::drop` cancels the token and calls
        // `JoinSet::abort_all()` with no `.await` in between, so `run`'s
        // own `tokio::select!` cancellation branch never gets polled --
        // only in-scope `Drop` impls run when a task is aborted, not the
        // rest of its code. This test aborts the `JoinHandle` directly
        // (exactly what `abort_all()` does per task) rather than
        // cancelling the token, so it actually exercises the same path a
        // cooperative-cancel test cannot.
        // A unique sleep duration (not a bare "sleep 30") so this test's
        // own `pgrep -f` matches the actual grandchild specifically --
        // cargo test runs tests in parallel by default, and the sibling
        // process-group test below intentionally spawns an
        // identical-looking "sleep 30" of its own concurrently. Base 30
        // is this file's own reserved prefix; `app::session`'s own
        // equivalent test uses base 35 -- `std::process::id()` alone is
        // NOT enough (it's identical for every test in one binary), a
        // real collision found live in M12.8. The duration itself never
        // matters (both are killed well before it elapses); only
        // cross-test uniqueness does.
        let marker = format!("30.{}", std::process::id() % 1000);
        let c = config("/bin/sh", &["-c", &format!("sleep {marker} & wait")]);
        let handle =
            tokio::spawn(
                async move { run(&c, &serde_json::json!({}), CancellationToken::new()).await },
            );
        tokio::time::sleep(Duration::from_millis(300)).await;
        let before = std::process::Command::new("pgrep")
            .args(["-f", &marker])
            .output()
            .expect("pgrep");
        assert!(
            !before.stdout.is_empty(),
            "the child must actually be running first"
        );
        handle.abort();
        let _ = handle.await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let after = std::process::Command::new("pgrep")
            .args(["-f", &marker])
            .output()
            .expect("pgrep");
        assert!(
            after.stdout.is_empty(),
            "no orphan may survive an aborted task: {}",
            String::from_utf8_lossy(&after.stdout)
        );
    }

    #[tokio::test]
    async fn cancellation_kills_the_process_before_the_timeout() {
        let c = {
            let mut c = config("/bin/sleep", &["30"]);
            c.timeout_secs = 30;
            c
        };
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            cancel_clone.cancel();
        });
        let start = std::time::Instant::now();
        let status = run(&c, &serde_json::json!({}), cancel).await;
        assert_eq!(status, Status::Cancelled);
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn stdout_is_bounded_never_unbounded_memory_growth() {
        let c = config(
            "/bin/sh",
            &["-c", "for i in $(seq 1 2000); do echo line$i; done"],
        );
        let status = run(&c, &serde_json::json!({}), CancellationToken::new()).await;
        match status {
            Status::Completed {
                stdout,
                stdout_truncated,
                ..
            } => {
                assert!(stdout.len() <= MAX_LINES);
                assert!(
                    stdout_truncated,
                    "2000 lines must exceed the bound and be marked truncated"
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_secret_bearing_environment_variable_reaches_the_child() {
        // SAFETY: test-local, single-threaded within this test's own scope,
        // restored before returning.
        unsafe {
            std::env::set_var("KUBECONFIG", "/should/never/be/seen");
            std::env::set_var("SAURON_TEST_TOKEN", "hunter2-should-not-leak");
        }
        let c = config("/usr/bin/env", &[]);
        let status = run(&c, &serde_json::json!({}), CancellationToken::new()).await;
        unsafe {
            std::env::remove_var("KUBECONFIG");
            std::env::remove_var("SAURON_TEST_TOKEN");
        }
        match status {
            Status::Completed { stdout, .. } => {
                let dump = stdout.join("\n");
                assert!(!dump.contains("KUBECONFIG"), "{dump}");
                assert!(!dump.contains("hunter2"), "{dump}");
                assert!(!dump.contains("SAURON_TEST_TOKEN"), "{dump}");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stderr_is_captured_separately_from_stdout() {
        let c = config("/bin/sh", &["-c", "echo out; echo err 1>&2"]);
        let status = run(&c, &serde_json::json!({}), CancellationToken::new()).await;
        match status {
            Status::Completed { stdout, stderr, .. } => {
                assert!(stdout.iter().any(|l| l == "out"));
                assert!(stderr.iter().any(|l| l == "err"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }
}
