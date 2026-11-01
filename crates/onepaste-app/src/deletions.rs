//! Pending deletions live only in memory. No tombstone or outbox write happens during the undo interval.
use crate::AppError;
use serde::Serialize;
use std::{future::Future, pin::Pin, time::{Duration, Instant}};

pub const DELETE_GRACE: Duration = Duration::from_secs(10);

/// Monotonic time and waiting are supplied together, so tests can advance time without changing the grace interval.
pub trait DeletionClock: Send + Sync {
    fn now(&self) -> Duration;
    fn wait_until(&self, deadline: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

pub(crate) struct MonotonicClock(Instant);
impl Default for MonotonicClock { fn default() -> Self { Self(Instant::now()) } }
impl DeletionClock for MonotonicClock {
    fn now(&self) -> Duration { self.0.elapsed() }
    fn wait_until(&self, deadline: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move { while self.now() < deadline { tokio::time::sleep(deadline.saturating_sub(self.now())).await; } })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDeletion { pub token: String, pub item_ids: Vec<String>, pub remaining_ms: u64, pub committing: bool }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionStatus { pub pending: Vec<PendingDeletion>, pub failed_batches: u64 }

struct Entry { token: String, ids: Vec<String>, deadline: Duration, committing: bool }
#[derive(Default)]
pub(crate) struct Queue { entries: Vec<Entry>, failed_batches: u64, stopped: bool }

impl Queue {
    pub fn stage(&mut self, token: String, ids: Vec<String>, deadline: Duration) -> Result<(), AppError> {
        if self.stopped { return Err(AppError::NotJoined); }
        if self.entries.iter().any(|entry| entry.ids.iter().any(|id| ids.contains(id))) { return Err(AppError::Other("delete_pending".into())); }
        self.entries.push(Entry { token, ids, deadline, committing: false }); Ok(())
    }
    pub fn undo(&mut self, token: &str, now: Duration) -> bool {
        let Some(index) = self.entries.iter().position(|entry| entry.token == token && !entry.committing && now < entry.deadline) else { return false; };
        self.entries.remove(index); true
    }
    pub fn mark_committing(&mut self, token: &str, now: Duration) -> Option<Vec<String>> {
        let entry = self.entries.iter_mut().find(|entry| entry.token == token)?;
        if self.stopped || entry.committing || now < entry.deadline { return None; }
        entry.committing = true; Some(entry.ids.clone())
    }
    pub fn is_committing(&self, token: &str) -> bool { !self.stopped && self.entries.iter().any(|entry| entry.token == token && entry.committing) }
    pub fn finish(&mut self, token: &str, failed: bool) {
        self.entries.retain(|entry| entry.token != token);
        if failed { self.failed_batches = self.failed_batches.saturating_add(1); }
    }
    pub fn cancel_all(&mut self) { self.entries.clear(); }
    pub fn stop(&mut self) { self.stopped = true; self.cancel_all(); }
    pub fn reopen(&mut self) { self.stopped = false; }
    pub fn status(&self, now: Duration) -> DeletionStatus {
        DeletionStatus { pending: self.entries.iter().map(|entry| {
            let remaining = entry.deadline.saturating_sub(now);
            let milliseconds = remaining.as_millis() + u128::from(remaining.subsec_nanos() % 1_000_000 != 0);
            PendingDeletion { token: entry.token.clone(), item_ids: entry.ids.clone(), remaining_ms: milliseconds.min(u64::MAX as u128) as u64, committing: entry.committing }
        }).collect(), failed_batches: self.failed_batches }
    }
}

pub(crate) fn unique_ids(ids: &[String]) -> Result<Vec<String>, AppError> {
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<_> = ids.iter().filter(|id| seen.insert((*id).clone())).cloned().collect();
    if ids.is_empty() { return Err(AppError::NotFound); } Ok(ids)
}
