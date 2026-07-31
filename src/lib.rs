//! mcp2cli — Turn any MCP server or OpenAPI spec into a CLI.
//!
//! Rust port of <https://github.com/knowsuchagency/mcp2cli>.

pub mod bake;
pub mod cache;
pub mod cli;
pub mod coerce;
pub mod error;
pub mod filter;
pub mod graphql;
pub mod mcp;
pub mod model;
pub mod oauth;
pub mod openapi;
pub mod output;
pub mod paths;
pub mod usage;

pub use error::{Error, Result};
pub use model::{CommandDef, ParamDef, ParamLocation, ParamType};

use std::ffi::OsString;

/// Library entry used by the binary.
pub fn run(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    match cli::dispatch(args.into_iter().collect()) {
        Err(Error::Usage(msg)) if msg == "__printed__" || msg == "__help__" => Ok(()),
        other => other,
    }
}
