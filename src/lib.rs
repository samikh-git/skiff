//! mcp2cli — turn any MCP server, OpenAPI spec, or GraphQL endpoint into a CLI.
//!
//! Library entry is [`run`]. The binary is a thin wrapper; session daemons reuse
//! the same binary via the hidden `__session_daemon <config-path>` argv form.
//!
//! Rust port of <https://github.com/knowsuchagency/mcp2cli>.

pub mod bake;
pub mod cache;
pub mod cli;
pub mod coerce;
pub mod error;
pub mod filter;
pub mod fsutil;
pub mod graphql;
pub mod mcp;
pub mod model;
pub mod oauth;
pub mod openapi;
pub mod output;
pub mod paths;
pub mod session;
pub mod usage;

pub use error::{Error, Result};
pub use model::{CommandDef, ParamDef, ParamLocation, ParamType};

use std::ffi::OsString;
use std::path::PathBuf;

/// Run the CLI with `argv` (no program name). Returns [`Error`] for exit mapping.
pub fn run(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    let args: Vec<OsString> = args.into_iter().collect();
    // Hidden daemon entry: mcp2cli __session_daemon <config-path>
    if args.first().and_then(|a| a.to_str()) == Some("__session_daemon") {
        #[cfg(unix)]
        {
            let path = args
                .get(1)
                .map(PathBuf::from)
                .ok_or_else(|| Error::usage("__session_daemon requires a config path"))?;
            return session::run_session_daemon(path);
        }
        #[cfg(not(unix))]
        {
            return Err(session::sessions_unsupported());
        }
    }
    match cli::dispatch(args) {
        Err(Error::Usage(msg)) if msg == "__printed__" || msg == "__help__" => Ok(()),
        other => other,
    }
}
