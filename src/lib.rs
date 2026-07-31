//! mcp2cli — Turn any MCP server or OpenAPI spec into a CLI.
//!
//! Rust port of <https://github.com/knowsuchagency/mcp2cli>.

pub mod bake;
pub mod cache;
pub mod cli;
pub mod coerce;
pub mod error;
pub mod filter;
pub mod mcp;
pub mod model;
pub mod openapi;
pub mod output;
pub mod paths;
pub mod usage;

pub use error::{Error, Result};
pub use model::{CommandDef, ParamDef, ParamLocation, ParamType};

use std::ffi::OsString;

/// Library entry used by the binary. M1: prints help / version until full dispatch lands.
pub fn run(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    let args: Vec<OsString> = args.into_iter().collect();
    let args_str: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();

    if args_str.iter().any(|a| a == "--version" || a == "-V") {
        println!("mcp2cli {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args_str.is_empty() || args_str.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return Ok(());
    }

    // Early dispatch stubs so the binary is usable while features land.
    if args_str.first().map(String::as_str) == Some("bake") {
        return Err(Error::runtime(
            "bake subcommands are not implemented yet in this Rust port",
        ));
    }

    Err(Error::usage(
        "full CLI dispatch is under construction; core library APIs are available. \
         Try: mcp2cli --version",
    ))
}

fn print_help() {
    eprintln!(
        "\
mcp2cli {version} — Turn any MCP server or OpenAPI spec into a CLI

Usage:
  mcp2cli --spec <URL|FILE> [--list] [command]
  mcp2cli --mcp <URL> [--list] [command]
  mcp2cli --mcp-stdio <CMD> [--list] [command]
  mcp2cli bake <create|list|show|remove|update|install> ...
  mcp2cli @<name> ...

Rust port in progress (M1). Library helpers and parity tests are landing first.
",
        version = env!("CARGO_PKG_VERSION")
    );
}
