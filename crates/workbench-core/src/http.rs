//! The one place outbound HTTP clients are configured. Every client gets a request
//! timeout: GitHub and Trello calls run on the GitHub poller's worker, so a stalled
//! response without one would freeze status polling for every project.

use std::time::Duration;

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) fn client_builder(timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(concat!("workbench/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(timeout.min(Duration::from_secs(10)))
        .timeout(timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stalled_response_times_out() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        // Accepts, then never answers.
        std::thread::spawn(move || {
            let conn = listener.accept();
            std::thread::sleep(Duration::from_secs(5));
            drop(conn);
        });

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = client_builder(Duration::from_millis(300)).build().unwrap();
        let err = rt
            .block_on(async { client.get(url).send().await })
            .unwrap_err();
        assert!(err.is_timeout(), "{err}");
    }
}
