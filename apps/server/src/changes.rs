//! A cheap "something changed" signal: writers bump it, readers wake and
//! re-read whatever they show. Carries no data, so a slow reader never holds
//! more than the fact that it is behind.

use std::sync::Arc;

use tokio::sync::watch;

#[derive(Clone)]
pub struct Changes(Arc<watch::Sender<u64>>);

impl Default for Changes {
    fn default() -> Self {
        Self(Arc::new(watch::channel(0).0))
    }
}

impl Changes {
    /// Wakes every subscriber; never blocks, and works with none.
    pub fn notify(&self) {
        self.0.send_modify(|n| *n = n.wrapping_add(1));
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.subscribe()
    }
}
