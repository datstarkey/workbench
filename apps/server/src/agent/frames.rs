//! What a chat session sends its attached clients, and how changes gather
//! into `update` frames: a streamed reply changes its item on every delta,
//! and each frame resends the item whole, so changes are coalesced to one
//! frame per [`FLUSH_EVERY`], with `meta` only when it changed.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::broadcast;
use workbench_core::claude_transcript::ChatView;

use super::lock;

/// How long changes gather before an `update` frame carries them.
pub(super) const FLUSH_EVERY: Duration = Duration::from_millis(50);
/// How often an idle flusher checks that its session still exists.
const IDLE_CHECK: Duration = Duration::from_secs(30);

/// A frame for attached clients. Shared, not cloned, by every receiver.
pub enum Frame {
    Text(String),
    /// `changes` is the JSON array of `[index, item]`; `meta` the current meta,
    /// which a client that asked for it only on change gets only then.
    Update {
        changes: String,
        meta: Arc<str>,
        meta_changed: bool,
    },
    /// The session is over for its clients (`exit`, or `replaced` by a relaunch).
    End(String),
}

impl Frame {
    /// The frame as JSON; `every_meta` for clients that predate `meta` being
    /// optional on `update` frames (they read it from every one).
    pub fn render(&self, every_meta: bool) -> Cow<'_, str> {
        match self {
            Self::Text(text) | Self::End(text) => Cow::Borrowed(text),
            Self::Update {
                changes,
                meta,
                meta_changed,
            } if every_meta || *meta_changed => Cow::Owned(format!(
                r#"{{"t":"update","changes":{changes},"meta":{meta}}}"#
            )),
            Self::Update { changes, .. } => {
                Cow::Owned(format!(r#"{{"t":"update","changes":{changes}}}"#))
            }
        }
    }

    pub fn ends(&self) -> bool {
        matches!(self, Self::End(_))
    }
}

/// Changes no `update` frame has carried yet.
#[derive(Default)]
struct Outbox {
    items: BTreeSet<usize>,
    /// Something changed since the last frame (items, or perhaps meta).
    dirty: bool,
    /// The meta JSON clients last got; `None` sends it with the next frame.
    meta: Option<Arc<str>>,
}

/// A session's broadcast channel and its pending changes. Every method that
/// takes a view runs under the session's driver lock (the outbox lock is
/// taken after it), so frames go out in the order changes were applied.
pub(super) struct Frames {
    tx: broadcast::Sender<Arc<Frame>>,
    outbox: Arc<(Mutex<Outbox>, Condvar)>,
}

impl Frames {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(256).0,
            outbox: Arc::default(),
        }
    }

    pub fn has_receivers(&self) -> bool {
        self.tx.receiver_count() > 0
    }

    /// A receiver for frames after the snapshot the caller builds now.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Frame>> {
        // Meta may change and change back before the next flush: send it
        // then, so this client can't be left on its snapshot's.
        lock(&self.outbox.0).meta = None;
        self.tx.subscribe()
    }

    /// Note changed items (and maybe meta) for the next `update` frame.
    pub fn mark(&self, changed: &[usize]) {
        let (outbox, due) = &*self.outbox;
        let mut o = lock(outbox);
        o.items.extend(changed);
        if !o.dirty {
            o.dirty = true;
            due.notify_one();
        }
    }

    /// Send what changed since the last frame.
    pub fn flush(&self, t: &dyn ChatView) {
        let mut o = lock(&self.outbox.0);
        if !std::mem::take(&mut o.dirty) {
            return;
        }
        let indices = std::mem::take(&mut o.items);
        if !self.has_receivers() {
            return; // nobody to tell; an attach starts from a snapshot
        }
        let meta: Arc<str> = serde_json::to_string(t.meta()).unwrap_or_default().into();
        let meta_changed = o.meta.as_deref() != Some(&*meta);
        if indices.is_empty() && !meta_changed {
            return;
        }
        o.meta = Some(meta.clone());
        drop(o);
        let items = t.items();
        let changes: Vec<Value> = indices
            .into_iter()
            .filter(|&i| i < items.len())
            .map(|i| json!([i, &items[i]]))
            .collect();
        let _ = self.tx.send(Arc::new(Frame::Update {
            changes: Value::Array(changes).to_string(),
            meta,
            meta_changed,
        }));
    }

    /// Any other frame, after the changes made before it.
    pub fn emit(&self, t: &dyn ChatView, frame: String) {
        self.flush(t);
        self.send(frame);
    }

    /// A frame that needn't follow pending changes (a reply to a setting).
    pub fn send(&self, frame: String) {
        let _ = self.tx.send(Arc::new(Frame::Text(frame)));
    }

    /// Every client gets the whole state again: nothing pending is owed.
    pub fn snapshot(&self, snapshot: String) {
        *lock(&self.outbox.0) = Outbox::default();
        self.send(snapshot);
    }

    /// The session failed: its error is the last thing clients should read
    /// before the end, so changes still pending are dropped, not sent after it.
    pub fn fail(&self, error: String) {
        let mut o = lock(&self.outbox.0);
        o.items.clear();
        o.dirty = false;
        drop(o);
        self.send(error);
    }

    pub fn end(&self, frame: String) {
        let _ = self.tx.send(Arc::new(Frame::End(frame)));
    }

    /// Call `flush` [`FLUSH_EVERY`] after each first change, until it says
    /// the session is gone.
    pub fn start_flusher(&self, flush: impl Fn() -> bool + Send + 'static) {
        let outbox = self.outbox.clone();
        std::thread::spawn(move || loop {
            {
                let (o, due) = &*outbox;
                let o = due
                    .wait_timeout_while(lock(o), IDLE_CHECK, |o| !o.dirty)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
                if !o.dirty {
                    drop(o);
                    if !flush() {
                        break;
                    }
                    continue;
                }
            }
            std::thread::sleep(FLUSH_EVERY);
            if !flush() {
                break;
            }
        });
    }
}

impl Drop for Frames {
    /// Wake the flusher so it sees the session is gone.
    fn drop(&mut self) {
        self.outbox.1.notify_all();
    }
}
