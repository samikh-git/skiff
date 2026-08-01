//! Import editor MCP server configs into bake (`baked.json`).
//!
//! Supports Cursor / Claude JSON (`mcpServers`) and Codex TOML (`mcp_servers`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;

use crate::bake::{create_baked, is_valid_bake_name, load_baked_all, BakedTool};
use crate::error::{Error, Result};
use crate::paths::DEFAULT_CACHE_TTL;

/// Editor / config source for [`import_mcp_servers`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportFrom {
    #[default]
    Auto,
    Cursor,
    Claude,
    Codex,
}

impl ImportFrom {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cursor" => Ok(Self::Cursor),
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            other => Err(Error::usage(format!(
                "invalid --from {other:?}; expected auto|cursor|claude|codex"
            ))),
        }
    }
}

/// One server ready to bake.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportCandidate {
    /// Original editor server key (display name).
    pub editor_name: String,
    /// Sanitized bake name `[a-z][a-z0-9-]*`.
    pub bake_name: String,
    pub tool: BakedTool,
    pub warnings: Vec<String>,
    pub source_label: String,
}

/// Options for [`import_mcp_servers`].
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    pub from: ImportFrom,
    pub path: Option<PathBuf>,
    /// Import only this server (matches editor name or bake name, case-insensitive).
    pub name: Option<String>,
    pub force: bool,
    pub dry_run: bool,
}

/// Resolve default config paths and import matching servers into bake.
pub fn import_mcp_servers(opts: &ImportOptions) -> Result<ImportReport> {
    let sources = resolve_sources(opts)?;
    if sources.is_empty() {
        return Err(Error::runtime(
            "no editor MCP config found; pass --path FILE or configure Cursor/Claude/Codex",
        ));
    }

    let mut candidates = Vec::new();
    for (label, path) in &sources {
        let text = fs::read_to_string(path)
            .map_err(|e| Error::runtime(format!("failed to read {}: {e}", path.display())))?;
        let parsed = parse_editor_config(&text, path)?;
        for mut c in parsed {
            c.source_label = format!("{label} ({})", path.display());
            candidates.push(c);
        }
    }

    if let Some(filter) = &opts.name {
        let f = filter.to_ascii_lowercase();
        candidates.retain(|c| {
            c.editor_name.to_ascii_lowercase() == f || c.bake_name.to_ascii_lowercase() == f
        });
        if candidates.is_empty() {
            return Err(Error::runtime(format!(
                "no MCP server matching {filter:?} in the selected config(s)"
            )));
        }
    }

    // Deduplicate bake names across sources (first wins unless --force on write).
    let mut seen = BTreeMap::<String, ImportCandidate>::new();
    for c in candidates {
        seen.entry(c.bake_name.clone()).or_insert(c);
    }
    let candidates: Vec<_> = seen.into_values().collect();

    let existing = load_baked_all()?;
    let mut imported = Vec::new();
    let mut skipped = Vec::new();

    for c in candidates {
        if existing.contains_key(&c.bake_name) && !opts.force {
            skipped.push(format!(
                "{} (bake name '{}'; already exists; use --force)",
                c.editor_name, c.bake_name
            ));
            continue;
        }
        if opts.dry_run {
            imported.push(c);
            continue;
        }
        create_baked(&c.bake_name, c.tool.clone(), opts.force)?;
        imported.push(c);
    }

    Ok(ImportReport {
        imported,
        skipped,
        dry_run: opts.dry_run,
    })
}

#[derive(Debug)]
pub struct ImportReport {
    pub imported: Vec<ImportCandidate>,
    pub skipped: Vec<String>,
    pub dry_run: bool,
}

fn resolve_sources(opts: &ImportOptions) -> Result<Vec<(String, PathBuf)>> {
    if let Some(path) = &opts.path {
        if !path.is_file() {
            return Err(Error::runtime(format!(
                "config file not found: {}",
                path.display()
            )));
        }
        return Ok(vec![("file".into(), path.clone())]);
    }

    let home = home_dir().ok_or_else(|| Error::runtime("HOME is not set"))?;
    let mut out = Vec::new();
    let want = opts.from;

    let cursor = home.join(".cursor").join("mcp.json");
    let claude = home.join(".claude.json");
    let claude_desktop = home
        .join("Library")
        .join("Application Support")
        .join("Claude")
        .join("claude_desktop_config.json");
    let codex = home.join(".codex").join("config.toml");

    match want {
        ImportFrom::Cursor => {
            if cursor.is_file() {
                out.push(("cursor".into(), cursor));
            }
        }
        ImportFrom::Claude => {
            if claude.is_file() {
                out.push(("claude".into(), claude));
            } else if claude_desktop.is_file() {
                out.push(("claude-desktop".into(), claude_desktop));
            }
        }
        ImportFrom::Codex => {
            if codex.is_file() {
                out.push(("codex".into(), codex));
            }
        }
        ImportFrom::Auto => {
            if cursor.is_file() {
                out.push(("cursor".into(), cursor));
            }
            if claude.is_file() {
                out.push(("claude".into(), claude));
            } else if claude_desktop.is_file() {
                out.push(("claude-desktop".into(), claude_desktop));
            }
            if codex.is_file() {
                out.push(("codex".into(), codex));
            }
        }
    }

    Ok(out)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Parse a config file into import candidates (bake names not yet deduped).
pub fn parse_editor_config(text: &str, path: &Path) -> Result<Vec<ImportCandidate>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "toml" || looks_like_toml(text) {
        return parse_codex_toml(text);
    }
    parse_mcp_servers_json(text)
}

fn looks_like_toml(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with('[') && !t.starts_with('{')
}

fn parse_mcp_servers_json(text: &str) -> Result<Vec<ImportCandidate>> {
    let v: Value = serde_json::from_str(text)
        .map_err(|e| Error::runtime(format!("invalid MCP JSON config: {e}")))?;
    let servers = v
        .get("mcpServers")
        .or_else(|| v.get("mcp_servers"))
        .and_then(|x| x.as_object())
        .ok_or_else(|| {
            Error::runtime("JSON config has no mcpServers object (Cursor/Claude shape)")
        })?;

    let mut out = Vec::new();
    for (name, cfg) in servers {
        if let Some(c) = server_json_to_candidate(name, cfg)? {
            out.push(c);
        }
    }
    Ok(out)
}

fn server_json_to_candidate(name: &str, cfg: &Value) -> Result<Option<ImportCandidate>> {
    let obj = match cfg.as_object() {
        Some(o) => o,
        None => return Ok(None),
    };

    let mut warnings = Vec::new();
    let url = obj
        .get("url")
        .or_else(|| obj.get("serverUrl"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let command = obj.get("command").and_then(|v| v.as_str());

    let (source_type, source) = if let Some(url) = url {
        ("mcp", url)
    } else if let Some(cmd) = command {
        let mut parts = vec![cmd.to_string()];
        if let Some(args) = obj.get("args").and_then(|a| a.as_array()) {
            for a in args {
                if let Some(s) = a.as_str() {
                    parts.push(s.to_string());
                } else {
                    warnings.push(format!("skipped non-string arg in {name:?}"));
                }
            }
        }
        ("mcp_stdio", shell_words::join(&parts))
    } else {
        warnings.push(format!(
            "skip {name:?}: no url/serverUrl or command (unsupported transport)"
        ));
        return Ok(None);
    };

    let mut auth_headers = Vec::new();
    if let Some(headers) = obj.get("headers").and_then(|h| h.as_object()) {
        for (hk, hv) in headers {
            let Some(raw) = hv.as_str() else {
                warnings.push(format!("skip non-string header {hk} on {name}"));
                continue;
            };
            let (stored, warn) = rewrite_secret_value(raw);
            if let Some(w) = warn {
                warnings.push(format!("{name} header {hk}: {w}"));
            }
            auth_headers.push((hk.clone(), stored));
        }
    }

    let mut env_vars = BTreeMap::new();
    if let Some(env) = obj.get("env").and_then(|e| e.as_object()) {
        for (ek, ev) in env {
            let Some(raw) = ev.as_str() else {
                warnings.push(format!("skip non-string env {ek} on {name}"));
                continue;
            };
            let trimmed = raw.trim();
            // `${VAR}` / `$VAR` means inherit from the process environment at call time —
            // do not persist a secret into baked.json.
            if extract_env_ref(trimmed).is_some() {
                warnings.push(format!(
                    "{name} env {ek}: inherits from process environment (not stored in bake)"
                ));
                continue;
            }
            env_vars.insert(ek.clone(), trimmed.to_string());
        }
    }

    let transport = obj
        .get("transport")
        .and_then(|t| t.as_str())
        .unwrap_or("auto")
        .to_string();
    let transport = match transport.to_ascii_lowercase().as_str() {
        "sse" => "sse".into(),
        "streamable" | "http" | "streamable-http" => "streamable".into(),
        _ => "auto".into(),
    };

    let bake_name = sanitize_bake_name(name);
    if !is_valid_bake_name(&bake_name) {
        warnings.push(format!("skip {name:?}: could not derive a valid bake name"));
        return Ok(None);
    }

    let description = obj
        .get("description")
        .or_else(|| obj.get("name"))
        .and_then(|d| d.as_str())
        .unwrap_or(name)
        .to_string();

    let tool = BakedTool {
        source_type: source_type.into(),
        source,
        auth_headers,
        env_vars,
        cache_ttl: DEFAULT_CACHE_TTL,
        transport,
        description,
        ..Default::default()
    };

    Ok(Some(ImportCandidate {
        editor_name: name.to_string(),
        bake_name,
        tool,
        warnings,
        source_label: String::new(),
    }))
}

fn parse_codex_toml(text: &str) -> Result<Vec<ImportCandidate>> {
    let table: toml::Table = text
        .parse()
        .map_err(|e| Error::runtime(format!("invalid Codex TOML config: {e}")))?;
    let servers = table
        .get("mcp_servers")
        .and_then(|v| v.as_table())
        .ok_or_else(|| Error::runtime("TOML config has no [mcp_servers] table"))?;

    let mut out = Vec::new();
    for (name, cfg) in servers {
        let Some(tbl) = cfg.as_table() else {
            continue;
        };
        // Convert TOML table → JSON Value for shared mapping.
        let json = toml_to_json(toml::Value::Table(tbl.clone()));
        if let Some(c) = server_json_to_candidate(name, &json)? {
            out.push(c);
        }
    }
    Ok(out)
}

fn toml_to_json(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            let mut map = serde_json::Map::new();
            for (k, v) in t {
                map.insert(k, toml_to_json(v));
            }
            Value::Object(map)
        }
    }
}

/// Sanitize an editor server name into a bake name.
pub fn sanitize_bake_name(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        return String::new();
    }
    if out.as_bytes()[0].is_ascii_digit() {
        out.insert_str(0, "mcp-");
    }
    // Collapse accidental double dashes already avoided; ensure valid.
    if is_valid_bake_name(&out) {
        out
    } else {
        String::new()
    }
}

/// Rewrite `${VAR}` / `$VAR` style header values into `env:` / `Bearer:env:` forms.
fn rewrite_secret_value(raw: &str) -> (String, Option<&'static str>) {
    let trimmed = raw.trim();
    if trimmed.starts_with("env:") || trimmed.starts_with("file:") {
        return (trimmed.to_string(), None);
    }
    if let Some(rest) = trimmed.strip_prefix("Bearer ") {
        if let Some(var) = extract_env_ref(rest.trim()) {
            return (format!("Bearer:env:{var}"), None);
        }
        if rest.starts_with("env:") || rest.starts_with("file:") {
            return (format!("Bearer:{rest}"), None);
        }
        return (
            trimmed.to_string(),
            Some("literal Bearer token stored; prefer ${VAR} in the editor config"),
        );
    }
    if let Some(var) = extract_env_ref(trimmed) {
        return (format!("env:{var}"), None);
    }
    // Plain literal — keep but warn.
    (
        trimmed.to_string(),
        Some("literal header value stored; prefer ${VAR} so secrets stay out of baked.json"),
    )
}

fn extract_env_ref(s: &str) -> Option<String> {
    static BRACE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"^\$\{([A-Za-z_][A-Za-z0-9_]*)\}$").unwrap());
    static PLAIN: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"^\$([A-Za-z_][A-Za-z0-9_]*)$").unwrap());
    if let Some(c) = BRACE.captures(s) {
        return Some(c[1].to_string());
    }
    if let Some(c) = PLAIN.captures(s) {
        return Some(c[1].to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize_bake_name("Docs by LangChain"), "docs-by-langchain");
        assert_eq!(sanitize_bake_name("raindrop"), "raindrop");
        assert_eq!(sanitize_bake_name("123abc"), "mcp-123abc");
        assert!(sanitize_bake_name("!!!").is_empty());
    }

    #[test]
    fn parse_cursor_json_http_and_stdio() {
        let text = r#"{
          "mcpServers": {
            "Docs by LangChain": {
              "url": "https://docs.langchain.com/mcp",
              "headers": { "Authorization": "Bearer ${API_TOKEN}" }
            },
            "raindrop": {
              "command": "raindrop",
              "args": ["workshop", "mcp"],
              "env": { "DEBUG": "1" }
            }
          }
        }"#;
        let path = Path::new("/tmp/mcp.json");
        let c = parse_editor_config(text, path).unwrap();
        assert_eq!(c.len(), 2);
        let docs = c
            .iter()
            .find(|x| x.bake_name == "docs-by-langchain")
            .unwrap();
        assert_eq!(docs.tool.source_type, "mcp");
        assert_eq!(docs.tool.source, "https://docs.langchain.com/mcp");
        assert_eq!(
            docs.tool.auth_headers,
            vec![("Authorization".into(), "Bearer:env:API_TOKEN".into())]
        );
        let rd = c.iter().find(|x| x.bake_name == "raindrop").unwrap();
        assert_eq!(rd.tool.source_type, "mcp_stdio");
        assert_eq!(rd.tool.source, "raindrop workshop mcp");
    }

    #[test]
    fn parse_codex_toml() {
        let text = r#"
[mcp_servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]

[mcp_servers.docs]
url = "https://example.com/mcp"
"#;
        let c = parse_editor_config(text, Path::new("config.toml")).unwrap();
        assert_eq!(c.len(), 2);
        let gh = c.iter().find(|x| x.bake_name == "github").unwrap();
        assert_eq!(gh.tool.source_type, "mcp_stdio");
        assert!(gh.tool.source.contains("npx"));
        assert!(gh
            .tool
            .source
            .contains("@modelcontextprotocol/server-github"));
        let docs = c.iter().find(|x| x.bake_name == "docs").unwrap();
        assert_eq!(docs.tool.source_type, "mcp");
    }

    #[test]
    fn rewrite_bearer_env() {
        let (v, w) = rewrite_secret_value("Bearer ${CF_API_TOKEN}");
        assert_eq!(v, "Bearer:env:CF_API_TOKEN");
        assert!(w.is_none());
        let (v, _) = rewrite_secret_value("${MY_TOKEN}");
        assert_eq!(v, "env:MY_TOKEN");
    }

    #[test]
    fn server_missing_transport_skipped() {
        let cfg = json!({ "type": "unsupported" });
        let r = server_json_to_candidate("x", &cfg).unwrap();
        assert!(r.is_none());
    }
}
