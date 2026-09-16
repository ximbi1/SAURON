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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Logs,
    Exec,
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
}
