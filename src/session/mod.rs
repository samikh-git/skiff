//! Named MCP session daemons over Unix-domain sockets (Unix only).
//!
//! One session name → one long-lived [`crate::mcp::McpClient`]. Ephemeral CLI
//! processes speak NDJSON over AF_UNIX (`list_tools`, `call_tool`, resources,
//! prompts). See crate README for layout, idle timeout, and security model.

#[cfg(unix)]
mod client;
#[cfg(unix)]
mod daemon;
#[cfg(unix)]
mod paths;
#[cfg(unix)]
mod peer;
#[cfg(unix)]
mod protocol;
#[cfg(unix)]
mod spawn;

#[cfg(unix)]
pub use client::session_request;
#[cfg(unix)]
pub use daemon::run_session_daemon;
#[cfg(unix)]
pub use paths::{
    clear_stale_session, is_process_alive, session_is_alive, session_log_path, session_meta_path,
    session_sock_path, sessions_dir, SessionMeta,
};
#[cfg(unix)]
pub use protocol::{SessionMethod, SessionRequest, SessionResponse};
#[cfg(unix)]
pub use spawn::{
    resolve_idle_secs, session_list, session_start, session_stop, DaemonConfig, DaemonOAuthConfig,
    DEFAULT_IDLE_SECS,
};

/// Non-Unix platforms: sessions are unsupported.
#[cfg(not(unix))]
pub fn sessions_unsupported() -> crate::error::Error {
    crate::error::Error::runtime(
        "MCP sessions require Unix domain sockets (not supported on this platform)",
    )
}
