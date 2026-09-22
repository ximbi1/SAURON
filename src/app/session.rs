//! Owns long-running tasks independently of the view consuming their data.
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    time::{Duration, Instant},
};
use tokio::task::{Id, JoinSet};
use tokio_util::sync::CancellationToken;

const MAX_ACTIVE: usize = 8;
const MAX_ENDED: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);
impl SessionId {
    pub fn number(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Logs,
    Exec,
    PortForward,
    /// M12.1: a local subprocess -- see `plugin::run`. Reuses this exact
    /// bounded/cancellable/`Drop`-safe ownership, not a second one.
    Plugin,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub epoch: u64,
    pub request: u64,
    pub context: String,
    pub cluster: String,
    pub resource: String,
    pub namespace: String,
    pub name: String,
    pub uid: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Cancelled,
    Failed(String),
    Panicked,
    Aborted,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Starting,
    Running,
    Stopping,
    Ended(Outcome),
}
impl State {
    pub fn label(&self) -> String {
        match self {
            Self::Starting => "Connecting".into(),
            Self::Running => "Streaming".into(),
            Self::Stopping => "Stopping".into(),
            Self::Ended(Outcome::Completed) => "Ended".into(),
            Self::Ended(Outcome::Cancelled) => "Cancelled".into(),
            Self::Ended(Outcome::Failed(message)) => format!("Failed: {message}"),
            Self::Ended(Outcome::Panicked) => "Failed: session task panicked".into(),
            Self::Ended(Outcome::Aborted) => "Ended: task aborted during cleanup".into(),
        }
    }
}
#[derive(Clone)]
pub struct Record {
    pub id: SessionId,
    pub kind: Kind,
    pub scope: Scope,
    pub started: Instant,
    pub state: State,
    cancel: CancellationToken,
}
#[derive(Default)]
pub struct Sessions {
    next: u64,
    tasks: JoinSet<(SessionId, Outcome)>,
    task_ids: HashMap<Id, SessionId>,
    active: HashMap<SessionId, Record>,
    ended: VecDeque<Record>,
}
impl Sessions {
    pub fn stop(&mut self, id: SessionId) -> bool {
        if let Some(record) = self.active.get_mut(&id) {
            record.state = State::Stopping;
            record.cancel.cancel();
            true
        } else {
            false
        }
    }
    pub fn spawn<F, Fut>(
        &mut self,
        kind: Kind,
        scope: Scope,
        cancel: CancellationToken,
        work: F,
    ) -> anyhow::Result<SessionId>
    where
        F: FnOnce(SessionId) -> Fut,
        Fut: Future<Output = Outcome> + Send + 'static,
    {
        anyhow::ensure!(
            self.active.len() < MAX_ACTIVE,
            "Session limit reached; wait for stopping sessions to finish"
        );
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Session identity exhausted"))?;
        let id = SessionId(self.next);
        let future = work(id);
        let token = cancel.clone();
        let task = self.tasks.spawn(async move {
            let outcome = tokio::select! {
                biased;
                _ = token.cancelled() => Outcome::Cancelled,
                outcome = future => outcome,
            };
            (id, outcome)
        });
        self.task_ids.insert(task.id(), id);
        self.active.insert(
            id,
            Record {
                id,
                kind,
                scope,
                started: Instant::now(),
                state: State::Starting,
                cancel,
            },
        );
        Ok(id)
    }
    pub fn running(&mut self, id: SessionId) {
        if let Some(record) = self.active.get_mut(&id) {
            if record.cancel.is_cancelled() {
                record.state = State::Stopping;
            } else if record.state == State::Starting {
                record.state = State::Running;
            }
        }
    }
    pub fn state(&self, id: SessionId) -> Option<State> {
        self.active
            .get(&id)
            .or_else(|| self.ended.iter().find(|r| r.id == id))
            .map(|r| {
                if !matches!(r.state, State::Ended(_)) && r.cancel.is_cancelled() {
                    State::Stopping
                } else {
                    r.state.clone()
                }
            })
    }
    pub fn active_count(&self) -> usize {
        self.active.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
    /// Completion never uses the bounded UI channel. Reaping is cancellation-safe.
    pub async fn join_next(&mut self) -> Option<Record> {
        let result = self.tasks.join_next_with_id().await?;
        let (task_id, id, outcome) = match result {
            Ok((task, (id, outcome))) => (task, id, outcome),
            Err(error) => {
                let task = error.id();
                let id = *self.task_ids.get(&task)?;
                (
                    task,
                    id,
                    if error.is_panic() {
                        Outcome::Panicked
                    } else {
                        Outcome::Aborted
                    },
                )
            }
        };
        self.task_ids.remove(&task_id);
        let mut record = self.active.remove(&id)?;
        record.state = State::Ended(outcome);
        self.ended.push_back(record.clone());
        if self.ended.len() > MAX_ENDED {
            self.ended.pop_front();
        }
        Some(record)
    }
    pub async fn shutdown(&mut self) {
        for record in self.active.values_mut() {
            record.state = State::Stopping;
            record.cancel.cancel();
        }
        if tokio::time::timeout(Duration::from_secs(2), async {
            while self.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            self.tasks.abort_all();
            while self.join_next().await.is_some() {}
        }
    }
}
impl Drop for Sessions {
    fn drop(&mut self) {
        for record in self.active.values() {
            record.cancel.cancel();
        }
        self.tasks.abort_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            epoch: 1,
            request: 2,
            context: "test".into(),
            cluster: "test".into(),
            resource: "v1/pods".into(),
            namespace: "test".into(),
            name: "pod".into(),
            uid: "uid".into(),
        }
    }
    #[tokio::test]
    async fn lifecycle_is_monotonic_and_history_bounded() {
        let mut sessions = Sessions::default();
        for n in 0..70 {
            let id = sessions
                .spawn(Kind::Logs, scope(), CancellationToken::new(), |_| async {
                    Outcome::Completed
                })
                .expect("spawn");
            assert_eq!(id.0, n + 1);
            assert_eq!(sessions.state(id), Some(State::Starting));
            sessions.running(id);
            assert_eq!(sessions.state(id), Some(State::Running));
            assert_eq!(
                sessions.join_next().await.expect("end").state,
                State::Ended(Outcome::Completed)
            );
            sessions.running(id);
            assert_eq!(sessions.state(id), Some(State::Ended(Outcome::Completed)));
        }
        assert_eq!(sessions.active_count(), 0);
        assert_eq!(sessions.ended.len(), MAX_ENDED);
    }
    #[tokio::test]
    async fn cancellation_preempts_unpolled_work_and_full_channel() {
        let mut sessions = Sessions::default();
        let token = CancellationToken::new();
        token.cancel();
        sessions
            .spawn(Kind::Logs, scope(), token, |_| async {
                panic!("cancelled work polled")
            })
            .expect("spawn");
        assert_eq!(
            sessions.join_next().await.expect("end").state,
            State::Ended(Outcome::Cancelled)
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        tx.send(()).await.expect("fill");
        let token = CancellationToken::new();
        let id = sessions
            .spawn(Kind::Logs, scope(), token.clone(), |_| async move {
                let _ = tx.send(()).await;
                Outcome::Completed
            })
            .expect("spawn");
        tokio::task::yield_now().await;
        token.cancel();
        assert_eq!(sessions.state(id), Some(State::Stopping));
        let record = tokio::time::timeout(Duration::from_secs(1), sessions.join_next())
            .await
            .expect("no deadlock")
            .expect("end");
        assert_eq!(record.state, State::Ended(Outcome::Cancelled));
    }
    #[tokio::test]
    async fn panic_limit_and_shutdown_are_accounted() {
        let mut sessions = Sessions::default();
        let id = sessions
            .spawn(Kind::Logs, scope(), CancellationToken::new(), |_| async {
                panic!("test session panic")
            })
            .expect("spawn");
        let record = sessions.join_next().await.expect("panic result");
        assert_eq!(record.id, id);
        assert_eq!(record.state, State::Ended(Outcome::Panicked));
        for _ in 0..MAX_ACTIVE {
            sessions
                .spawn(Kind::Logs, scope(), CancellationToken::new(), |_| {
                    std::future::pending()
                })
                .expect("spawn");
        }
        assert!(
            sessions
                .spawn(Kind::Logs, scope(), CancellationToken::new(), |_| {
                    std::future::pending()
                })
                .is_err()
        );
        sessions.shutdown().await;
        assert!(sessions.is_empty());
        assert_eq!(sessions.active_count(), 0);
        assert!(sessions.task_ids.is_empty());
    }
    #[tokio::test]
    async fn dropping_owner_cancels_children() {
        let mut sessions = Sessions::default();
        let token = CancellationToken::new();
        sessions
            .spawn(Kind::Logs, scope(), token.clone(), |_| {
                std::future::pending()
            })
            .expect("spawn");
        drop(sessions);
        assert!(token.is_cancelled());
    }
    /// M12.1's own "no orphan process after quit" guarantee end-to-end:
    /// a real `plugin::run` spawned through `Sessions::spawn` (exactly the
    /// way `Runtime` will do it, not a synthetic `pending()` future) must
    /// leave no child process alive once the owning `Sessions` drops --
    /// proven by inspecting the real OS process table, not just the
    /// cancellation token.
    #[tokio::test]
    async fn dropping_sessions_leaves_no_real_child_process_running() {
        let mut sessions = Sessions::default();
        // A unique duration (not a bare "sleep 30") so this test's own
        // `pgrep -f` can never collide with `plugin.rs`'s own identical-
        // looking fixtures running concurrently -- cargo test runs tests
        // in parallel by default. Base 35 (not 30): `std::process::id()`
        // is the SAME for every test in this one binary, so a marker
        // built only from the PID still collides with `plugin.rs`'s own
        // `aborting_the_task_...` test, which uses the identical "30."
        // formula -- a real collision found live in M12.8's own combined
        // acceptance run. Distinct static bases make collision impossible
        // regardless of PID.
        let marker = format!("35.{}", std::process::id() % 1000);
        let config = crate::plugin::PluginConfig {
            executable: "/bin/sleep".into(),
            args: vec![marker.clone()],
            trust: crate::plugin::Trust::Approved,
            timeout_secs: 30,
        };
        // The outer Sessions-level token is deliberately never cancelled in
        // this test: what's under test is that dropping `Sessions` itself
        // (which aborts the underlying tokio task outright) still leaves
        // `plugin::run`'s own process-group `GroupKillGuard` no chance to
        // leak, not the cooperative-cancel path `plugin.rs`'s own unit
        // tests already cover in isolation.
        sessions
            .spawn(
                Kind::Plugin,
                scope(),
                CancellationToken::new(),
                move |_| async move {
                    let _ = crate::plugin::run(
                        &config,
                        &serde_json::json!({}),
                        CancellationToken::new(),
                    )
                    .await;
                    Outcome::Completed
                },
            )
            .expect("spawn");
        // Let the child actually start before we drop the owner.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let before = std::process::Command::new("pgrep")
            .args(["-f", &marker])
            .output()
            .expect("pgrep");
        assert!(
            !before.stdout.is_empty(),
            "the plugin's sleep must actually be running first"
        );
        drop(sessions);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let after = std::process::Command::new("pgrep")
            .args(["-f", &marker])
            .output()
            .expect("pgrep");
        assert!(
            after.stdout.is_empty(),
            "no orphan child process may survive Sessions being dropped: {}",
            String::from_utf8_lossy(&after.stdout)
        );
    }
}
