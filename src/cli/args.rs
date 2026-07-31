//! Global CLI flags (pre-parser).

use clap::Parser;

use crate::paths::DEFAULT_CACHE_TTL;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "mcp2cli",
    about = "Turn any MCP server or OpenAPI spec into a CLI",
    version,
    disable_help_subcommand = true,
    allow_hyphen_values = true
)]
pub struct GlobalArgs {
    /// OpenAPI spec URL or file path
    #[arg(long)]
    pub spec: Option<String>,

    /// MCP server URL (HTTP)
    #[arg(long)]
    pub mcp: Option<String>,

    /// MCP server command (stdio)
    #[arg(long)]
    pub mcp_stdio: Option<String>,

    /// GraphQL endpoint URL (deferred)
    #[arg(long)]
    pub graphql: Option<String>,

    /// HTTP header as Name:Value (repeatable)
    #[arg(long = "auth-header", value_name = "Name:Value")]
    pub auth_header: Vec<String>,

    /// Override base URL from OpenAPI spec
    #[arg(long)]
    pub base_url: Option<String>,

    /// Custom cache key
    #[arg(long)]
    pub cache_key: Option<String>,

    /// Cache TTL in seconds
    #[arg(long, default_value_t = DEFAULT_CACHE_TTL)]
    pub cache_ttl: u64,

    /// Force re-fetch
    #[arg(long)]
    pub refresh: bool,

    /// List available subcommands
    #[arg(long = "list", visible_alias = "list-commands")]
    pub list_commands: bool,

    /// Search tools by name or description
    #[arg(long = "search", value_name = "PATTERN")]
    pub search_pattern: Option<String>,

    /// Full tool descriptions in --list
    #[arg(long)]
    pub verbose: bool,

    /// Sort --list: usage|recent|alpha|default
    #[arg(long, value_parser = ["usage", "recent", "alpha", "default"])]
    pub sort: Option<String>,

    /// Show only top N tools
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,

    /// Space-separated tool names only
    #[arg(long)]
    pub compact: bool,

    /// Pretty-print JSON
    #[arg(long)]
    pub pretty: bool,

    /// Print raw response body
    #[arg(long)]
    pub raw: bool,

    /// Force valid JSON output
    #[arg(long = "json")]
    pub json_output: bool,

    /// TOON encoding (falls back with warning in M1)
    #[arg(long)]
    pub toon: bool,

    /// Limit output to first N array records
    #[arg(long, value_name = "N")]
    pub head: Option<usize>,

    /// GraphQL selection set override
    #[arg(long)]
    pub fields: Option<String>,

    /// MCP HTTP transport: auto|sse|streamable
    #[arg(long, default_value = "auto", value_parser = ["auto", "sse", "streamable"])]
    pub transport: String,

    /// Env KEY=VALUE for MCP stdio (repeatable)
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,
}

impl GlobalArgs {
    pub fn output_options(&self) -> crate::output::OutputOptions {
        crate::output::OutputOptions {
            pretty: self.pretty,
            raw: self.raw,
            toon: self.toon,
            head: self.head,
            json_output: self.json_output,
        }
    }

    pub fn parse_auth_headers(&self) -> crate::error::Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        for item in &self.auth_header {
            let Some((k, v)) = item.split_once(':') else {
                return Err(crate::error::Error::usage(format!(
                    "invalid auth header format: {item:?}"
                )));
            };
            let v = crate::coerce::resolve_secret(v.trim())?;
            out.push((k.trim().to_string(), v));
        }
        Ok(out)
    }

    pub fn parse_env_vars(&self) -> crate::error::Result<std::collections::BTreeMap<String, String>> {
        let mut out = std::collections::BTreeMap::new();
        for item in &self.env {
            let Some((k, v)) = item.split_once('=') else {
                return Err(crate::error::Error::usage(format!(
                    "invalid env format: {item:?}"
                )));
            };
            out.insert(k.trim().to_string(), v.to_string());
        }
        Ok(out)
    }
}
