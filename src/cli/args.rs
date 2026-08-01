//! Global CLI flags parsed after [`crate::cli::split_at_subcommand`].
//!
//! Source flags are mutually exclusive at dispatch time. Session and OAuth
//! helpers live here so clap help stays the single source of user-facing docs.

use clap::Parser;

use crate::model::ListDetail;
use crate::paths::DEFAULT_CACHE_TTL;
use crate::spool::DEFAULT_AGENT_MAX_BYTES;

/// Default `--top` when `--agent` + `--search` and user did not set `--top`.
pub const DEFAULT_AGENT_SEARCH_TOP: usize = 20;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "skiff",
    about = "Turn any MCP server, OpenAPI spec, or GraphQL endpoint into a CLI",
    version,
    disable_help_subcommand = true,
    allow_hyphen_values = true
)]
pub struct GlobalArgs {
    /// OpenAPI spec URL or local file
    #[arg(long)]
    pub spec: Option<String>,

    /// MCP server URL (HTTP streamable / SSE)
    #[arg(long)]
    pub mcp: Option<String>,

    /// MCP server as a shell command (stdio transport)
    #[arg(long)]
    pub mcp_stdio: Option<String>,

    /// GraphQL endpoint URL
    #[arg(long)]
    pub graphql: Option<String>,

    /// HTTP header as Name:Value (repeatable; value may use env:/file:)
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

    /// List / help JSON detail: names|brief|full
    #[arg(long, value_name = "LEVEL", value_parser = ["names", "brief", "full"])]
    pub detail: Option<String>,

    /// Emit one tool's full schema as JSON (progressive describe)
    #[arg(long = "describe", value_name = "TOOL")]
    pub describe: Option<String>,

    /// Full tool descriptions in --list
    #[arg(long)]
    pub verbose: bool,

    /// Sort --list: usage|recent|alpha|default
    #[arg(long, value_parser = ["usage", "recent", "alpha", "default"])]
    pub sort: Option<String>,

    /// Show only top N tools
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,

    /// Space-separated tool names only (alias for --detail names)
    #[arg(long)]
    pub compact: bool,

    /// Pretty-print JSON
    #[arg(long)]
    pub pretty: bool,

    /// Print raw response body
    #[arg(long)]
    pub raw: bool,

    /// Force valid JSON output (content-only for MCP; see --envelope)
    #[arg(long = "json")]
    pub json_output: bool,

    /// Return full MCP CallToolResult envelope (instead of content-only)
    #[arg(long = "envelope", visible_alias = "full")]
    pub envelope: bool,

    /// TOON encoding (native; falls back to JSON on encode failure)
    #[arg(long)]
    pub toon: bool,

    /// Limit output to first N array records
    #[arg(long, value_name = "N")]
    pub head: Option<usize>,

    /// Spill stdout to spool when rendered size exceeds N bytes (0 = never)
    #[arg(long = "max-bytes", value_name = "N")]
    pub max_bytes: Option<usize>,

    /// Never spill; always print full stdout
    #[arg(long)]
    pub inline: bool,

    /// Agent defaults: JSON, brief discovery, spool oversize (or SKIFF_AGENT=1)
    #[arg(long)]
    pub agent: bool,

    /// Delete expired spool files and exit
    #[arg(long = "spool-clean")]
    pub spool_clean: bool,

    /// GraphQL selection set override
    #[arg(long)]
    pub fields: Option<String>,

    /// MCP HTTP transport: auto|sse|streamable
    #[arg(long, default_value = "auto", value_parser = ["auto", "sse", "streamable"])]
    pub transport: String,

    /// Env KEY=VALUE for MCP stdio (repeatable)
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    /// Enable OAuth (also implied by --oauth-client-id / --oauth-client-secret)
    #[arg(long)]
    pub oauth: bool,

    /// OAuth client ID (supports env:/file: secrets)
    #[arg(long)]
    pub oauth_client_id: Option<String>,

    /// OAuth client secret (supports env:/file: secrets)
    #[arg(long)]
    pub oauth_client_secret: Option<String>,

    /// OAuth client name for DCR
    #[arg(long, default_value = "skiff")]
    pub oauth_client_name: String,

    /// OAuth scope string
    #[arg(long)]
    pub oauth_scope: Option<String>,

    /// Loopback redirect URI (http://127.0.0.1:<port>/callback)
    #[arg(long)]
    pub oauth_redirect_uri: Option<String>,

    /// OAuth flow: auto|authorization_code|client_credentials
    #[arg(long, default_value = "auto", value_parser = ["auto", "authorization_code", "client_credentials"])]
    pub oauth_flow: String,

    /// Clear cached OAuth credentials for the discovery URL and exit
    #[arg(long = "oauth-clear")]
    pub oauth_clear: bool,

    /// Start a named MCP session daemon (requires --mcp or --mcp-stdio; Unix)
    #[arg(long = "session-start", value_name = "NAME")]
    pub session_start: Option<String>,

    /// Use an existing session daemon instead of a one-shot MCP connect (Unix)
    #[arg(long = "session", value_name = "NAME")]
    pub session: Option<String>,

    /// Stop a named session daemon (SIGTERM, then SIGKILL; Unix)
    #[arg(long = "session-stop", value_name = "NAME")]
    pub session_stop: Option<String>,

    /// List session daemons (use --json for machine-readable output)
    #[arg(long = "session-list")]
    pub session_list: bool,

    /// Session idle exit after N seconds of no IPC (default 1800; 0 = never)
    #[arg(long = "session-idle-secs", value_name = "SECS")]
    pub session_idle_secs: Option<u64>,

    /// For stdio sessions: child gets only PATH/HOME/LANG/TMP* plus --env
    #[arg(long = "session-clean-env")]
    pub session_clean_env: bool,

    /// List MCP resources
    #[arg(long = "list-resources")]
    pub list_resources: bool,

    /// List MCP resource templates
    #[arg(long = "list-resource-templates")]
    pub list_resource_templates: bool,

    /// Read an MCP resource by URI
    #[arg(long = "read-resource", value_name = "URI")]
    pub read_resource: Option<String>,

    /// List MCP prompts
    #[arg(long = "list-prompts")]
    pub list_prompts: bool,

    /// Get an MCP prompt by name
    #[arg(long = "get-prompt", value_name = "NAME")]
    pub get_prompt: Option<String>,

    /// Prompt argument as key=value (repeatable, with --get-prompt)
    #[arg(long = "prompt-arg", value_name = "KEY=VALUE")]
    pub prompt_arg: Vec<String>,
}

impl GlobalArgs {
    /// Apply `--agent` / `SKIFF_AGENT=1` defaults (idempotent).
    pub fn apply_agent_defaults(&mut self) {
        let env_agent = std::env::var("SKIFF_AGENT")
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        if !self.agent && !env_agent {
            return;
        }
        self.agent = true;
        if !self.raw && !self.toon {
            self.json_output = true;
        }
        if self.detail.is_none() && !self.compact {
            // Search: names only (then --top). Browse: brief.
            if self.search_pattern.is_some() {
                self.detail = Some("names".into());
            } else {
                self.detail = Some("brief".into());
            }
        }
        if self.search_pattern.is_some() && self.top.is_none() {
            self.top = Some(DEFAULT_AGENT_SEARCH_TOP);
        }
        if self.max_bytes.is_none() {
            self.max_bytes = Some(DEFAULT_AGENT_MAX_BYTES);
        }
    }

    pub fn list_detail(&self) -> ListDetail {
        if self.compact {
            return ListDetail::Names;
        }
        if let Some(d) = self.detail.as_deref().and_then(ListDetail::parse) {
            return d;
        }
        // Agent default: brief. Plain `--json`/`--toon`: full (pre-agent parity).
        if self.agent {
            ListDetail::Brief
        } else if self.json_output || self.toon {
            ListDetail::Full
        } else {
            ListDetail::Brief
        }
    }

    /// Suppress human list banners.
    pub fn quiet_list(&self) -> bool {
        self.agent || self.json_output || self.compact || self.toon
    }

    pub fn output_options(&self) -> crate::output::OutputOptions {
        // `apply_agent_defaults` already sets `json_output` when agent && !raw && !toon.
        // Do not force JSON over `--raw`.
        crate::output::OutputOptions {
            pretty: self.pretty,
            raw: self.raw,
            toon: self.toon,
            head: self.head,
            json_output: self.json_output,
            max_bytes: self.max_bytes,
            inline: self.inline,
        }
    }

    /// MCP call: content-only unless `--envelope`.
    pub fn full_envelope(&self) -> bool {
        self.envelope
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

    pub fn parse_env_vars(
        &self,
    ) -> crate::error::Result<std::collections::BTreeMap<String, String>> {
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
