//! The `workbench` Claude Code plugin (`plugins/workbench`) reaches the bridge
//! with `$.http.fetch`, so a connection may be one HTTP POST instead of the
//! raw JSON lines Codex's notify script writes.

use std::io::{self, BufRead, Read};

const MAX_BODY: usize = 16 * 1024 * 1024;
const MAX_HEADER_LINE: u64 = 8 * 1024;

pub const ACCEPTED: &[u8] = b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
pub const FORBIDDEN: &[u8] =
    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
pub const REFUSED: &[u8] =
    b"HTTP/1.1 415 Unsupported Media Type\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// Whether the connection opens with an HTTP POST, without consuming it.
pub fn is_post<R: BufRead>(reader: &mut R) -> bool {
    reader.fill_buf().is_ok_and(|buf| buf.starts_with(b"POST "))
}

pub const SECRET_HEADER: &str = "x-workbench-hook-secret";

/// A POST's JSON body and the bridge secret it carried.
pub struct JsonPost {
    pub body: Vec<u8>,
    pub secret: Option<String>,
}

/// Reads the request line, headers and body. `None` unless the body is
/// `application/json`: a browser can't send that cross-origin without a
/// preflight, which the bridge never answers.
pub fn read_json_body<R: BufRead>(reader: &mut R) -> io::Result<Option<JsonPost>> {
    let mut length = None;
    let mut is_json = false;
    let mut secret = None;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.by_ref().take(MAX_HEADER_LINE).read_line(&mut line)? == 0 || !line.ends_with('\n')
        {
            return Ok(None);
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse::<usize>().ok();
        } else if name.eq_ignore_ascii_case(SECRET_HEADER) {
            secret = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("content-type") {
            is_json = value
                .split(';')
                .next()
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"));
        }
    }
    let Some(length) = length.filter(|&n| is_json && n <= MAX_BODY) else {
        return Ok(None);
    };
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(JsonPost { body, secret }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    fn request(content_type: &str, body: &str) -> String {
        format!(
            "POST /hook HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn detects_a_post_without_consuming_it() {
        let raw = request("application/json", "{}");
        let mut reader = BufReader::new(raw.as_bytes());
        assert!(is_post(&mut reader));
        assert!(is_post(&mut reader));
        assert!(!is_post(&mut BufReader::new(&b"{\"pane_id\":\"p\"}\n"[..])));
    }

    #[test]
    fn reads_a_json_body() {
        let body = r#"{"pane_id":"p","hook":{"hook_event_name":"Stop"}}"#;
        let raw = request("application/json; charset=utf-8", body);
        let read = read_json_body(&mut BufReader::new(raw.as_bytes())).unwrap();
        assert_eq!(read.map(|p| p.body).as_deref(), Some(body.as_bytes()));
    }

    #[test]
    fn reads_the_bridge_secret() {
        let raw = "POST /hook HTTP/1.1\r\nX-Workbench-Hook-Secret: s3cret\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
        let read = read_json_body(&mut BufReader::new(raw.as_bytes()))
            .unwrap()
            .unwrap();
        assert_eq!(read.secret.as_deref(), Some("s3cret"));
        let bare = request("application/json", "{}");
        let read = read_json_body(&mut BufReader::new(bare.as_bytes()))
            .unwrap()
            .unwrap();
        assert_eq!(read.secret, None);
    }

    #[test]
    fn refuses_bodies_a_browser_could_send_cross_origin() {
        let raw = request("text/plain", r#"{"pane_id":"p","hook":{}}"#);
        assert!(read_json_body(&mut BufReader::new(raw.as_bytes()))
            .unwrap()
            .is_none());
    }

    #[test]
    fn refuses_an_oversized_header_line() {
        let raw = format!(
            "POST /hook HTTP/1.1\r\nX-Pad: {}\r\n\r\n",
            "a".repeat(10_000)
        );
        assert!(read_json_body(&mut BufReader::new(raw.as_bytes()))
            .unwrap()
            .is_none());
    }

    #[test]
    fn refuses_a_body_without_a_length() {
        let raw = "POST /hook HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}";
        assert!(read_json_body(&mut BufReader::new(raw.as_bytes()))
            .unwrap()
            .is_none());
    }
}
