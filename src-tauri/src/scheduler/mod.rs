pub mod commands;
pub mod engine;
pub mod manifest;
pub mod runner;
pub mod state;
pub mod templating;

use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

#[derive(Clone, Default)]
pub struct SchedulerHandle {
    pub inner: Arc<SchedulerInner>,
}

#[derive(Default)]
pub struct SchedulerInner {
    pub cancel: Notify,
    pub rescan: Notify,
    pub state_lock: Mutex<()>,
}

impl SchedulerHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shutdown(&self) {
        self.inner.cancel.notify_waiters();
    }

    pub fn rescan(&self) {
        self.inner.rescan.notify_waiters();
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn make_full_id(app_id: &str, local_id: &str) -> String {
    format!("{app_id}::{local_id}")
}

pub fn split_full_id(full: &str) -> Option<(&str, &str)> {
    full.split_once("::")
}
