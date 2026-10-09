//! The hook bridge: a loopback listener that Claude hook events (the
//! `workbench` plugin's HTTP POST) and Codex's `notify` script (raw JSON
//! lines) report to. The server stamps its address on every terminal and chat
//! it starts (`WORKBENCH_HOOK_SOCKET` = `host:port#secret`), so no client
//! names it; whoever displays the events subscribes ([`HookBridge::subscribe`]).
//!
//! Loopback is reachable by any local (or sandboxed) process: each event must
//! carry this launch's secret, which only the processes the server starts get.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;

mod http;

const CONNECTION_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// One event a process reported.
#[derive(Debug, Clone)]
pub enum HookEvent {
    Claude {
        pane_id: String,
        hook: Value,
    },
    Codex {
        pane_id: String,
        codex: Value,
    },
    /// A line that couldn't be read, for the log.
    Invalid {
        summary: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Envelope {
    Claude {
        pane_id: String,
        hook: Value,
    },
    Codex {
        pane_id: String,
        codex: Value,
        /// Codex's notify script writes raw lines, so its secret rides in the line.
        #[serde(default)]
        secret: Option<String>,
    },
}

struct Listening {
    address: String,
    secret: String,
}

#[derive(Clone)]
pub struct HookBridge {
    listening: Arc<OnceLock<Option<Listening>>>,
    events: broadcast::Sender<HookEvent>,
}

impl Default for HookBridge {
    fn default() -> Self {
        Self {
            listening: Arc::default(),
            events: broadcast::channel(256).0,
        }
    }
}

impl HookBridge {
    /// Bind the listener once; later calls do nothing.
    pub fn start(&self) {
        self.listening.get_or_init(|| match self.listen() {
            Ok(listening) => Some(listening),
            Err(e) => {
                tracing::error!("hook bridge: {e:#}");
                None
            }
        });
    }

    fn listen(&self) -> anyhow::Result<Listening> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = format!("127.0.0.1:{}", listener.local_addr()?.port());
        // Each launch's sandbox file lets a sandboxed `claude` reach it.
        workbench_core::sandbox_runtime::set_hook_socket(Some(address.clone()));
        let secret = workbench_core::token::generate()?;
        let (events, expected) = (self.events.clone(), Arc::new(secret.clone()));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                // A client that connects and goes quiet would hold its thread forever.
                let _ = stream.set_read_timeout(Some(CONNECTION_READ_TIMEOUT));
                let (events, expected) = (events.clone(), expected.clone());
                std::thread::spawn(move || serve(&stream, &events, &expected));
            }
        });
        Ok(Listening { address, secret })
    }

    /// `host:port#secret`, as `WORKBENCH_HOOK_SOCKET` hands it to a process.
    pub fn socket(&self) -> Option<String> {
        let l = self.listening.get()?.as_ref()?;
        Some(format!("{}#{}", l.address, l.secret))
    }

    /// The bare `host:port`, for the sandbox's network allowlist.
    pub fn address(&self) -> Option<String> {
        Some(self.listening.get()?.as_ref()?.address.clone())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HookEvent> {
        self.events.subscribe()
    }
}

fn serve(stream: &std::net::TcpStream, events: &broadcast::Sender<HookEvent>, secret: &str) {
    let mut reader = BufReader::new(stream);
    if !http::is_post(&mut reader) {
        return read_lines(reader, events, Some(secret));
    }
    // Answered once the event is published: the plugin awaits the reply, so
    // events stay in order.
    let reply = match http::read_json_body(&mut reader) {
        Ok(Some(post)) if authorized(secret, post.secret.as_deref()) => {
            read_lines(BufReader::new(post.body.as_slice()), events, None);
            http::ACCEPTED
        }
        Ok(Some(_)) => http::FORBIDDEN,
        _ => http::REFUSED,
    };
    let _ = (&*stream).write_all(reply);
}

/// `secret`: each line must carry it (a raw Codex line); `None` when the
/// request already proved it (the plugin's POST header).
fn read_lines<R: Read>(
    reader: BufReader<R>,
    events: &broadcast::Sender<HookEvent>,
    secret: Option<&str>,
) {
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let envelope = match serde_json::from_str::<Envelope>(&line) {
            Ok(envelope) => envelope,
            Err(e) => {
                let shown = workbench_core::text::truncate_bytes(&line, 200);
                let _ = events.send(HookEvent::Invalid {
                    summary: format!("Invalid payload: {e} — {shown}"),
                });
                continue;
            }
        };
        let given = match &envelope {
            Envelope::Codex { secret, .. } => secret.as_deref(),
            Envelope::Claude { .. } => None,
        };
        if secret.is_some_and(|expected| !authorized(expected, given)) {
            tracing::warn!("hook bridge: refused an event without this launch's secret");
            continue;
        }
        let _ = events.send(match envelope {
            Envelope::Claude { pane_id, hook } => HookEvent::Claude { pane_id, hook },
            Envelope::Codex { pane_id, codex, .. } => HookEvent::Codex { pane_id, codex },
        });
    }
}

fn authorized(expected: &str, given: Option<&str>) -> bool {
    given.is_some_and(|given| {
        workbench_core::token::constant_time_eq(expected.as_bytes(), given.as_bytes())
    })
}

/// A Codex `notify` that finished a turn: `(thread id, cwd)`.
pub fn codex_turn_ended(codex: &Value) -> Option<(&str, &str)> {
    if codex.get("type").and_then(Value::as_str) != Some("agent-turn-complete") {
        return None;
    }
    Some((
        codex.get("thread-id")?.as_str()?,
        codex.get("cwd")?.as_str()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn published(lines: &str, secret: Option<&str>) -> Vec<HookEvent> {
        let (tx, mut rx) = broadcast::channel(16);
        read_lines(BufReader::new(lines.as_bytes()), &tx, secret);
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    #[test]
    fn a_raw_codex_line_needs_the_secret() {
        let line = r#"{"pane_id":"p","secret":"s3cret","codex":{"type":"agent-turn-complete"}}"#;
        assert!(matches!(
            &published(line, Some("s3cret"))[..],
            [HookEvent::Codex { .. }]
        ));
        assert!(published(line, Some("other")).is_empty());
        let bare = r#"{"pane_id":"p","codex":{}}"#;
        assert!(published(bare, Some("s3cret")).is_empty());
    }

    #[test]
    fn a_posted_claude_hook_is_published() {
        let line = r#"{"pane_id":"p1","hook":{"session_id":"s1"}}"#;
        match &published(line, None)[..] {
            [HookEvent::Claude { pane_id, hook }] => {
                assert_eq!(pane_id, "p1");
                assert_eq!(hook["session_id"], "s1");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_bad_line_is_logged_not_dropped_silently() {
        assert!(matches!(
            &published(r#"{"pane_id":"p3"}"#, None)[..],
            [HookEvent::Invalid { .. }]
        ));
    }

    #[test]
    fn only_this_launchs_secret_is_authorized() {
        assert!(authorized("s3cret", Some("s3cret")));
        assert!(!authorized("s3cret", Some("guess")));
        assert!(!authorized("s3cret", None));
    }

    #[test]
    fn reads_a_finished_codex_turn() {
        let done = json!({"type": "agent-turn-complete", "thread-id": "t", "cwd": "/p"});
        assert_eq!(codex_turn_ended(&done), Some(("t", "/p")));
        assert_eq!(codex_turn_ended(&json!({"type": "other"})), None);
    }

    #[test]
    fn the_listener_takes_a_post_with_the_secret() {
        let bridge = HookBridge::default();
        let mut rx = bridge.subscribe();
        bridge.start();
        let socket = bridge.socket().unwrap();
        let (address, secret) = socket.split_once('#').unwrap();
        let body = r#"{"pane_id":"p","hook":{"hook_event_name":"Stop"}}"#;
        let post = |secret: &str| {
            let mut conn = std::net::TcpStream::connect(address).unwrap();
            write!(
                conn,
                "POST / HTTP/1.1\r\n{}: {secret}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                http::SECRET_HEADER,
                body.len()
            )
            .unwrap();
            let mut reply = String::new();
            conn.read_to_string(&mut reply).unwrap();
            reply
        };
        assert!(post("wrong").starts_with("HTTP/1.1 403"));
        assert!(post(secret).starts_with("HTTP/1.1 204"));
        assert!(matches!(rx.try_recv(), Ok(HookEvent::Claude { .. })));
        assert!(rx.try_recv().is_err(), "the refused post published nothing");
    }
}
