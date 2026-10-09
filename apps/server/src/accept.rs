//! The accept loop `axum::serve` runs, plus TCP keepalive on each accepted
//! socket: axum 0.7 never hands them out, and Windows doesn't pass a
//! listener's keepalive on to them. A peer that vanished without closing (a
//! phone asleep or off the network) then has its idle connection, and the
//! descriptor, freed instead of held until a restart.

use std::future::Future;
use std::time::Duration;

use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

/// Idle this long, a connection is probed; unanswered probes close it.
const KEEPALIVE_IDLE: Duration = Duration::from_secs(30);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

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
    let builder = Builder::new(TokioExecutor::new());
    let conn =
        builder.serve_connection_with_upgrades(TokioIo::new(stream), TowerToHyperService::new(app));
    tokio::pin!(conn);
    tokio::select! {
        _ = conn.as_mut() => return,
        _ = stopping.wait_for(|stop| *stop) => {}
    }
    conn.as_mut().graceful_shutdown();
    let _ = conn.await;
}
