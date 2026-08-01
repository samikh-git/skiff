//! Loopback OAuth callback HTTP server.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use url::Url;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct CallbackResult {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub iss: Option<String>,
}

/// Bind a one-shot callback server and wait up to `timeout` for the redirect.
pub fn wait_for_callback(redirect_uri: &str, timeout: Duration) -> Result<CallbackResult> {
    let parsed = Url::parse(redirect_uri)
        .map_err(|e| Error::runtime(format!("invalid redirect URI: {e}")))?;
    let host = parsed.host_str().unwrap_or("127.0.0.1");
    let port = parsed
        .port()
        .ok_or_else(|| Error::runtime("redirect URI missing port"))?;
    let path = parsed.path().to_string();

    let addr: SocketAddr = if host == "::1" {
        SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port))
    } else {
        format!("{host}:{port}")
            .parse()
            .map_err(|e| Error::runtime(format!("bad bind addr: {e}")))?
    };

    let listener = TcpListener::bind(addr)
        .map_err(|e| Error::runtime(format!("cannot bind callback server on {addr}: {e}")))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| Error::runtime(e.to_string()))?;

    let shared = Arc::new(Mutex::new(None::<CallbackResult>));
    let shared_clone = Arc::clone(&shared);
    let path_clone = path.clone();

    let accept_thread = thread::spawn(move || {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((stream, _)) => {
                    match handle_connection(stream, &path_clone) {
                        Ok(Some(result)) => {
                            if let Ok(mut g) = shared_clone.lock() {
                                *g = Some(result);
                            }
                            return;
                        }
                        Ok(None) => {} // wrong path / probe — keep waiting
                        Err(_) => {}
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => thread::sleep(Duration::from_millis(50)),
            }
        }
    });

    let _ = accept_thread.join();
    let result = shared
        .lock()
        .map_err(|_| Error::runtime("OAuth callback lock poisoned"))?
        .take()
        .ok_or_else(|| Error::runtime("OAuth callback timed out waiting for browser redirect"))?;

    if let Some(err) = &result.error {
        return Err(Error::runtime(format!("OAuth error: {err}")));
    }
    if result.code.is_none() {
        return Err(Error::runtime("OAuth callback missing authorization code"));
    }
    Ok(result)
}

/// Returns `Ok(Some(...))` only for the expected callback path; `Ok(None)` for probes.
fn handle_connection(
    mut stream: TcpStream,
    expected_path: &str,
) -> std::io::Result<Option<CallbackResult>> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let path_and_query = first_line.split_whitespace().nth(1).unwrap_or("/");

    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path_and_query, ""),
    };

    let path_ok =
        path == expected_path || path.trim_end_matches('/') == expected_path.trim_end_matches('/');
    if !path_ok {
        let body = "<html><body><h1>Not found</h1></body></html>";
        let resp = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        return Ok(None);
    }

    let mut result = CallbackResult::default();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let v = urlencoding_decode(v);
            match k {
                "code" => result.code = Some(v),
                "state" => result.state = Some(v),
                "error" => result.error = Some(v),
                "iss" => result.iss = Some(v),
                _ => {}
            }
        }
    }

    let (status, body) = if result.error.is_some() {
        (
            "400 Bad Request",
            "<html><body><h1>Authorization failed</h1><p>You can close this window.</p></body></html>",
        )
    } else if result.code.is_some() {
        (
            "200 OK",
            "<html><body><h1>Authorization successful</h1><p>You can close this window and return to the CLI.</p></body></html>",
        )
    } else {
        (
            "400 Bad Request",
            "<html><body><h1>Missing authorization code</h1></body></html>",
        )
    };

    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    // Complete the wait only when we got code or error (not an empty probe on the right path).
    if result.code.is_some() || result.error.is_some() {
        Ok(Some(result))
    } else {
        Ok(None)
    }
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpStream;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn captures_code_and_state() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let uri = format!("http://127.0.0.1:{port}/callback");

        let handle = thread::spawn(move || wait_for_callback(&uri, Duration::from_secs(5)));

        thread::sleep(Duration::from_millis(100));
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req =
            "GET /callback?code=abc123&state=xyz&iss=https%3A%2F%2Fas.example HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        stream.write_all(req.as_bytes()).unwrap();
        let _ = stream.read(&mut [0u8; 256]);

        let result = handle.join().unwrap().unwrap();
        assert_eq!(result.code.as_deref(), Some("abc123"));
        assert_eq!(result.state.as_deref(), Some("xyz"));
        assert_eq!(result.iss.as_deref(), Some("https://as.example"));
    }
}
