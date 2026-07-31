//! Session IPC client (one request per connection).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::session::paths::session_sock_path;
use crate::session::protocol::{SessionRequest, SessionResponse};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Send one NDJSON request to a named session daemon and return the result.
pub fn session_request(name: &str, method: &str, params: Value) -> Result<Value> {
    let sock = session_sock_path(name);
    if !sock.exists() {
        return Err(Error::runtime(format!(
            "session {name:?} is not running (socket missing). Start with --session-start {name}"
        )));
    }

    let mut stream = UnixStream::connect(&sock).map_err(|e| {
        Error::runtime(format!(
            "cannot connect to session {name:?}: {e}. Start with --session-start {name}"
        ))
    })?;
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|e| Error::runtime(e.to_string()))?;
    stream
        .set_write_timeout(Some(CONNECT_TIMEOUT))
        .map_err(|e| Error::runtime(e.to_string()))?;

    let req = SessionRequest {
        id: 1,
        method: method.to_string(),
        params,
    };
    let mut line = serde_json::to_string(&req)?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| Error::runtime(format!("session write failed: {e}")))?;
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut buf = String::new();
    stream
        .read_to_string(&mut buf)
        .map_err(|e| Error::runtime(format!("session read failed: {e}")))?;
    let line = buf.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Err(Error::runtime(format!(
            "session {name:?} closed without a response"
        )));
    }
    let resp: SessionResponse = serde_json::from_str(line)
        .map_err(|e| Error::runtime(format!("invalid session response: {e}")))?;
    if let Some(err) = resp.error {
        return Err(Error::runtime(err));
    }
    Ok(resp.result.unwrap_or(Value::Null))
}
