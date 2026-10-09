//! What runs beside the WebSocket attach: output activity (a pane's busy), and
//! an in-process view of a terminal for a renderer in this process (the
//! desktop's native macOS views), which sees the same PTY a socket would.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use portable_pty::PtySize;
use tokio::sync::{broadcast, watch};

use super::{lock, TerminalManager, TerminalSession};

/// Output this soon after input is its echo, or a redraw it caused.
const ECHO_MS: u64 = 300;
/// Busy until output has been quiet this long.
const QUIET_MS: u64 = 1500;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Default)]
pub(super) struct Activity {
    output: AtomicU64,
    input: AtomicU64,
}

impl Activity {
    pub(super) fn input(&self) {
        self.input.store(now_ms(), Ordering::Relaxed);
    }

    pub(super) fn output(&self) {
        let now = now_ms();
        if now.saturating_sub(self.input.load(Ordering::Relaxed)) > ECHO_MS {
            self.output.store(now, Ordering::Relaxed);
        }
    }

    pub(super) fn busy(&self) -> bool {
        let last = self.output.load(Ordering::Relaxed);
        last != 0 && now_ms().saturating_sub(last) < QUIET_MS
    }
}

/// A terminal seen from this process: its scrollback, then its live output
/// until `done` flips.
pub struct Tap {
    pub replay: Vec<u8>,
    pub output: broadcast::Receiver<Vec<u8>>,
    pub done: watch::Receiver<bool>,
    session: Arc<TerminalSession>,
}

impl Tap {
    /// The shell's exit code, once it has one.
    pub fn exit_code(&self) -> Option<i64> {
        *lock(&self.session.exit_code)
    }
}

impl TerminalManager {
    /// Follow a terminal's output, as a socket attach does but without taking
    /// the single-attacher lease: the view and a remote client both see it.
    pub fn tap(&self, id: &str) -> Option<Tap> {
        let session = self.get(id)?;
        let (replay, output) = {
            let buffer = lock(&session.buffer);
            (buffer.iter().copied().collect(), session.tx.subscribe())
        };
        let done = session.done_tx.subscribe();
        Some(Tap {
            replay,
            output,
            done,
            session,
        })
    }

    /// Type into a terminal, waiting for room in its input queue. Blocking:
    /// never call it from an async task. `false` once it's gone.
    pub fn write(&self, id: &str, bytes: Vec<u8>) -> bool {
        let Some(session) = self.get(id) else {
            return false;
        };
        session.activity.input();
        session.input.blocking_send(bytes).is_ok()
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        let session = self
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("no terminal with id {id}"))?;
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let resized = lock(&session.master).resize(size);
        resized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_is_not_activity() {
        let a = Activity::default();
        assert!(!a.busy());
        a.input();
        a.output();
        assert!(!a.busy(), "output right after a keystroke is its echo");
        a.input.store(now_ms() - 2 * ECHO_MS, Ordering::Relaxed);
        a.output();
        assert!(a.busy());
        a.output.store(now_ms() - 2 * QUIET_MS, Ordering::Relaxed);
        assert!(!a.busy(), "quiet again");
    }
}
