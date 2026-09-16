//! Presentation metadata only. Tasks remain owned by the shared Sessions supervisor.
use super::session::{Scope, SessionId, Sessions, State};
use crate::kube::forward::{Ports, Progress};
use std::{collections::BTreeMap, time::Instant};
use tokio::sync::watch;

pub struct Entry {
    pub scope: Scope,
    pub ports: Ports,
    pub started: Instant,
    pub progress: watch::Receiver<Progress>,
}
#[derive(Default)]
pub struct Forwards {
    pub entries: BTreeMap<SessionId, Entry>,
}
impl Forwards {
    pub fn active_count(&self, sessions: &Sessions) -> usize {
        self.entries
            .keys()
            .filter(|id| {
                sessions
                    .state(**id)
                    .is_some_and(|s| !matches!(s, State::Ended(_)))
            })
            .count()
    }
    pub fn prune(&mut self, sessions: &Sessions) {
        while self.entries.len() >= 32 {
            let old = self
                .entries
                .keys()
                .find(|id| {
                    sessions
                        .state(**id)
                        .is_none_or(|s| matches!(s, State::Ended(_)))
                })
                .copied();
            if let Some(id) = old {
                self.entries.remove(&id);
            } else {
                break;
            }
        }
    }
    pub fn document(&self, sessions: &Sessions) -> String {
        let mut text = format!(
            "Background forwards: {} active (max 4); 8 clients per forward\nStop: :pf_stop ID · no reconnect or automatic retarget\n\nID  TYPE  TARGET  CONTEXT  LOCAL -> REMOTE  STATE  AGE\n",
            self.active_count(sessions)
        );
        if self.entries.is_empty() {
            text.push_str("No forwards started this session.\n");
        }
        let mut rows = self.entries.iter().collect::<Vec<_>>();
        rows.sort_by_key(|(id, _)| {
            (
                matches!(sessions.state(**id), Some(State::Ended(_)) | None),
                **id,
            )
        });
        for (id, entry) in rows {
            let progress = entry.progress.borrow();
            let state = sessions.state(*id);
            let label = match state {
                Some(State::Starting | State::Running)
                    if progress.local.is_some() && !progress.ended =>
                {
                    "Listening".into()
                }
                Some(ref state) => state.label(),
                None => "Ended (outcome expired)".into(),
            };
            let address = progress
                .local
                .map(|a| a.to_string())
                .unwrap_or_else(|| format!("127.0.0.1:{} (requested)", entry.ports.local));
            text.push_str(&format!("{}  TCP  {}/{}  {}  {} -> {}  {}  {}s\n  cluster={} uid={} clients={} accepted={} rejected={} failures={}\n", id.number(), entry.scope.namespace, entry.scope.name, entry.scope.context, address, entry.ports.remote, label, entry.started.elapsed().as_secs(), entry.scope.cluster, entry.scope.uid, if progress.ended || matches!(state, Some(State::Ended(_))) { 0 } else { progress.clients }, progress.accepted, progress.rejected, progress.failures));
            if let Some(error) = &progress.last_error {
                text.push_str(&format!("  Last connection error: {error}\n"));
            }
        }
        crate::safety::text(&text)
    }
}
