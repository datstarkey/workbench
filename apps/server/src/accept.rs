//! The accept loop `axum::serve` runs, plus TCP keepalive on each accepted
//! socket: axum 0.7 never hands them out, and Windows doesn't pass a
//! listener's keepalive on to them. A peer that vanished without closing (a
//! phone asleep or off the network) then has its idle connection, and the
//! descriptor, freed instead of held until a restart.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// Idle this long, a connection is probed; unanswered probes close it.
const KEEPALIVE_IDLE: Duration = Duration::from_secs(30);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
/// How long a client may take to send a request's headers.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a write may wait for room in the socket before the connection is
/// dropped: its peer stopped reading.
const WRITE_STALL: Duration = Duration::from_secs(60);

/// Serve `app` until `shutdown` resolves, then wait for open requests (an
/// upgraded WebSocket no longer counts as one).
pub async fn serve(listener: TcpListener, app: axum::Router, shutdown: impl Future<Output = ()>) {
    let (stop, stopping) = watch::channel(false);
    tokio::pin!(shutdown);
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            () = &mut shutdown => break,
        };
        match accepted {
            Ok((stream, _)) => {
                tokio::spawn(connection(stream, app.clone(), stopping.clone()));
            }
            Err(e) if is_connection_error(&e) => {}
            // Out of descriptors, say: retrying at once would spin.
            Err(e) => {
                tracing::error!("accept failed: {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    drop((listener, stopping));
    let _ = stop.send(true);
    stop.closed().await;
}

fn is_connection_error(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(
        e.kind(),
        ConnectionRefused | ConnectionAborted | ConnectionReset
    )
}

async fn connection(stream: TcpStream, app: axum::Router, mut stopping: watch::Receiver<bool>) {
    let probes = socket2::TcpKeepalive::new()
        .with_time(KEEPALIVE_IDLE)
        .with_interval(KEEPALIVE_INTERVAL);
    if let Err(e) = socket2::SockRef::from(&stream).set_tcp_keepalive(&probes) {
        tracing::warn!("couldn't turn on TCP keepalive: {e}");
    }
    let _ = stream.set_nodelay(true);
    let mut builder = Builder::new(TokioExecutor::new());
    // Without a timer hyper never times out a request's headers: a client that
    // opens a socket and stalls (a half-open pooled connection, a scanner on
    // the LAN bind) would hold its task and descriptor forever.
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT);
    let io = TokioIo::new(WriteDeadline::new(stream));
    let conn = builder.serve_connection_with_upgrades(io, TowerToHyperService::new(app));
    tokio::pin!(conn);
    tokio::select! {
        _ = conn.as_mut() => return,
        _ = stopping.wait_for(|stop| *stop) => {}
    }
    conn.as_mut().graceful_shutdown();
    let _ = conn.await;
}

/// A socket whose writes fail after [`WRITE_STALL`] without progress. A peer
/// that stopped reading but still ACKs (a frozen app's kernel holds a zero
/// window open, so keepalive never fires) would otherwise keep its connection,
/// task and descriptor forever: an SSE stream or a long poll's answer just
/// waits for room, and nothing in it runs to notice.
struct WriteDeadline<T> {
    inner: T,
    stalled: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl<T> WriteDeadline<T> {
    fn new(inner: T) -> Self {
        Self {
            inner,
            stalled: None,
        }
    }

    /// `written`, or an error once writes have waited past the deadline.
    fn check<R>(
        &mut self,
        cx: &mut Context<'_>,
        written: Poll<io::Result<R>>,
    ) -> Poll<io::Result<R>> {
        if written.is_ready() {
            self.stalled = None;
            return written;
        }
        let stalled = self
            .stalled
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(WRITE_STALL)));
        match stalled.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the peer stopped reading",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for WriteDeadline<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for WriteDeadline<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let written = Pin::new(&mut this.inner).poll_write(cx, buf);
        this.check(cx, written)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let written = Pin::new(&mut this.inner).poll_write_vectored(cx, bufs);
        this.check(cx, written)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    /// Not timed: a socket's flush is immediate, and must not count as the
    /// progress a stalled write is waiting for.
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test(start_paused = true)]
    async fn a_write_the_peer_never_reads_fails_after_the_stall() {
        let (ours, _theirs) = tokio::io::duplex(8);
        let mut socket = WriteDeadline::new(ours);
        socket.write_all(&[0; 8]).await.unwrap();
        let started = tokio::time::Instant::now();
        let err = socket.write_all(&[0; 8]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert_eq!(started.elapsed(), WRITE_STALL);
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_reader_that_keeps_reading_is_never_cut_off() {
        use tokio::io::AsyncReadExt;
        let (ours, mut theirs) = tokio::io::duplex(8);
        let reader = tokio::spawn(async move {
            let mut buf = [0; 8];
            for _ in 0..4 {
                tokio::time::sleep(WRITE_STALL / 2).await;
                theirs.read_exact(&mut buf).await.unwrap();
            }
        });
        let mut socket = WriteDeadline::new(ours);
        for _ in 0..4 {
            socket.write_all(&[0; 8]).await.unwrap();
        }
        reader.await.unwrap();
    }
}
