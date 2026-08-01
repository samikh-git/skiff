//! Bake mode — named connection configs in `$MCP2CLI_CONFIG_DIR/baked.json`.
//!
//! `@name` expands via [`BakedTool::to_argv`]. Prefer `env:`/`file:` secrets;
//! [`BakedTool::masked_for_display`] is used by `bake show`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::model::BakeConfig;
use crate::paths::{baked_file, DEFAULT_CACHE_TTL};

static BAKE_NAME_RE: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9-]*$").expect("bake name regex"));

/// Validate bake tool names: `[a-z][a-z0-9-]*`.
pub fn is_valid_bake_name(name: &str) -> bool {
    BAKE_NAME_RE.is_match(name)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BakedTool {
    pub source_type: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_headers: Vec<(String, String)>,
    #[serde(default)]
    pub env_vars: BTreeMap<String, String>,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl: u64,
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default)]
    pub oauth: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_secret: Option<String>,
    #[serde(
        default = "default_oauth_client_name",
        skip_serializing_if = "is_default_oauth_client_name"
    )]
    pub oauth_client_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_redirect_uri: Option<String>,
    #[serde(
        default = "default_oauth_flow",
        skip_serializing_if = "is_default_oauth_flow"
    )]
    pub oauth_flow: String,
    /// Prefer routing through this named session daemon when present
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Forward-compat for fields we don't model yet.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_cache_ttl() -> u64 {
    DEFAULT_CACHE_TTL
}

fn default_transport() -> String {
    "auto".into()
}

fn default_oauth_client_name() -> String {
    "mcp2cli".into()
}

fn is_default_oauth_client_name(s: &str) -> bool {
    s == "mcp2cli"
}

fn default_oauth_flow() -> String {
    "auto".into()
}

fn is_default_oauth_flow(s: &str) -> bool {
    s == "auto"
}

impl Default for BakedTool {
    fn default() -> Self {
        Self {
            source_type: String::new(),
            source: String::new(),
            base_url: None,
            auth_headers: Vec::new(),
            env_vars: BTreeMap::new(),
            cache_ttl: DEFAULT_CACHE_TTL,
            transport: default_transport(),
            oauth: false,
            oauth_client_id: None,
            oauth_client_secret: None,
            oauth_client_name: default_oauth_client_name(),
            oauth_scope: None,
            oauth_redirect_uri: None,
            oauth_flow: default_oauth_flow(),
            session: None,
            include: Vec::new(),
            exclude: Vec::new(),
            methods: Vec::new(),
            description: String::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl BakedTool {
    pub fn bake_config(&self) -> BakeConfig {
        BakeConfig {
            include: self.include.clone(),
            exclude: self.exclude.clone(),
            methods: self.methods.clone(),
        }
    }

    /// Reconstruct CLI argv from a baked config (Python `_baked_to_argv`).
    pub fn to_argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        match self.source_type.as_str() {
            "spec" => {
                argv.push("--spec".into());
                argv.push(self.source.clone());
            }
            "mcp" => {
                argv.push("--mcp".into());
                argv.push(self.source.clone());
            }
            "mcp_stdio" => {
                argv.push("--mcp-stdio".into());
                argv.push(self.source.clone());
            }
            "graphql" => {
                argv.push("--graphql".into());
                argv.push(self.source.clone());
            }
            other => {
                // Unknown source types still emit a best-effort flag.
                argv.push(format!("--{other}"));
                argv.push(self.source.clone());
            }
        }
        if let Some(base) = &self.base_url {
            argv.push("--base-url".into());
            argv.push(base.clone());
        }
        for (name, value) in &self.auth_headers {
            argv.push("--auth-header".into());
            argv.push(format!("{name}:{value}"));
        }
        for (k, v) in &self.env_vars {
            argv.push("--env".into());
            argv.push(format!("{k}={v}"));
        }
        argv.push("--cache-ttl".into());
        argv.push(self.cache_ttl.to_string());
        if self.transport != "auto" {
            argv.push("--transport".into());
            argv.push(self.transport.clone());
        }
        if self.oauth {
            argv.push("--oauth".into());
        }
        if let Some(id) = &self.oauth_client_id {
            argv.push("--oauth-client-id".into());
            argv.push(id.clone());
        }
        if let Some(sec) = &self.oauth_client_secret {
            argv.push("--oauth-client-secret".into());
            argv.push(sec.clone());
        }
        if self.oauth_client_name != "mcp2cli" {
            argv.push("--oauth-client-name".into());
            argv.push(self.oauth_client_name.clone());
        }
        if let Some(scope) = &self.oauth_scope {
            argv.push("--oauth-scope".into());
            argv.push(scope.clone());
        }
        if let Some(uri) = &self.oauth_redirect_uri {
            argv.push("--oauth-redirect-uri".into());
            argv.push(uri.clone());
        }
        if self.oauth_flow != "auto" {
            argv.push("--oauth-flow".into());
            argv.push(self.oauth_flow.clone());
        }
        if let Some(sess) = &self.session {
            argv.push("--session".into());
            argv.push(sess.clone());
        }
        argv
    }

    /// Copy suitable for `bake show` (literal secrets masked).
    pub fn masked_for_display(&self) -> Value {
        let mut display = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(headers) = display
            .get_mut("auth_headers")
            .and_then(|v| v.as_array_mut())
        {
            for entry in headers {
                if let Some(arr) = entry.as_array_mut() {
                    if arr.len() >= 2 {
                        if let Some(val) = arr[1].as_str() {
                            arr[1] = Value::String(mask_secret(val));
                        }
                    }
                }
            }
        }
        if let Some(Value::String(sec)) = display.get_mut("oauth_client_secret") {
            *sec = mask_secret(sec);
        }
        display
    }
}

fn mask_secret(val: &str) -> String {
    if val.starts_with("env:") || val.starts_with("file:") {
        val.to_string()
    } else if val.len() > 4 {
        format!("{}****", &val[..4])
    } else {
        "****".into()
    }
}

pub type BakedStore = BTreeMap<String, BakedTool>;

pub fn load_baked_all() -> Result<BakedStore> {
    let path = baked_file();
    if !path.exists() {
        return Ok(BakedStore::new());
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Ok(BakedStore::new()),
    };
    match serde_json::from_str(&text) {
        Ok(store) => Ok(store),
        Err(_) => Ok(BakedStore::new()),
    }
}

pub fn save_baked_all(store: &BakedStore) -> Result<()> {
    // Pretty-print to match Python `indent=2` baked.json.
    let path = baked_file();
    let text = serde_json::to_string_pretty(store)? + "\n";
    crate::fsutil::atomic_write_0600(&path, text.as_bytes())?;
    Ok(())
}

/// Load a single baked config by name.
pub fn get_baked(name: &str) -> Result<Option<BakedTool>> {
    Ok(load_baked_all()?.get(name).cloned())
}

pub fn require_baked(name: &str) -> Result<BakedTool> {
    get_baked(name)?.ok_or_else(|| Error::runtime(format!("no baked tool named '{name}'")))
}

pub fn split_csv_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn split_methods(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(|x| x.to_uppercase())
        .collect()
}

/// Parse `Name:Value` auth headers without resolving secrets (stored as-is).
pub fn parse_auth_header_raw(items: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for item in items {
        let Some((k, v)) = item.split_once(':') else {
            return Err(Error::usage(format!(
                "invalid auth header format: {item:?}"
            )));
        };
        out.push((k.trim().to_string(), v.trim().to_string()));
    }
    Ok(out)
}

pub fn parse_env_raw(items: &[String]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for item in items {
        let Some((k, v)) = item.split_once('=') else {
            return Err(Error::usage(format!("invalid env format: {item:?}")));
        };
        out.insert(k.trim().to_string(), v.to_string());
    }
    Ok(out)
}

pub fn create_baked(name: &str, tool: BakedTool, force: bool) -> Result<()> {
    if !is_valid_bake_name(name) {
        return Err(Error::usage(format!(
            "invalid name '{name}' — must match [a-z][a-z0-9-]*"
        )));
    }
    let mut store = load_baked_all()?;
    if store.contains_key(name) && !force {
        return Err(Error::runtime(format!(
            "'{name}' already exists. Use --force to overwrite."
        )));
    }
    store.insert(name.to_string(), tool);
    save_baked_all(&store)
}

pub fn remove_baked(name: &str) -> Result<()> {
    let mut store = load_baked_all()?;
    if store.remove(name).is_none() {
        return Err(Error::runtime(format!("no baked tool named '{name}'")));
    }
    save_baked_all(&store)?;
    // Clean up default install wrapper if present.
    if let Some(home) = home_dir() {
        let wrapper = home.join(".local").join("bin").join(name);
        if wrapper.exists() {
            fs::remove_file(&wrapper)?;
            println!("Removed installed wrapper: {}", wrapper.display());
        }
    }
    Ok(())
}

pub fn update_baked(name: &str, mutator: impl FnOnce(&mut BakedTool)) -> Result<()> {
    let mut store = load_baked_all()?;
    let cfg = store
        .get_mut(name)
        .ok_or_else(|| Error::runtime(format!("no baked tool named '{name}'")))?;
    mutator(cfg);
    save_baked_all(&store)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn resolve_mcp2cli_bin() -> String {
    if let Ok(exe) = std::env::current_exe() {
        return exe.to_string_lossy().into_owned();
    }
    which_mcp2cli().unwrap_or_else(|| "mcp2cli".into())
}

fn which_mcp2cli() -> Option<String> {
    let output = StdCommand::new("which").arg("mcp2cli").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn shell_quote(s: &str) -> String {
    // POSIX single-quote escaping.
    if s.is_empty() {
        return "''".into();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// Install a shell wrapper that runs `mcp2cli @name "$@"`.
pub fn install_wrapper(name: &str, dir: Option<&Path>) -> Result<PathBuf> {
    let _ = require_baked(name)?;
    let bin_dir = match dir {
        Some(d) => d.to_path_buf(),
        None => home_dir()
            .map(|h| h.join(".local").join("bin"))
            .ok_or_else(|| Error::runtime("cannot determine home directory"))?,
    };
    fs::create_dir_all(&bin_dir)?;
    let wrapper = bin_dir.join(name);
    let bin = resolve_mcp2cli_bin();
    let content = format!("#!/bin/sh\nexec {} @{} \"$@\"\n", shell_quote(&bin), name);
    fs::write(&wrapper, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;
    }
    Ok(wrapper)
}

pub fn default_install_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".local").join("bin"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{set_config_dir_override, TEST_PATHS_LOCK};
    use tempfile::tempdir;

    #[test]
    fn name_validation() {
        for name in ["petstore", "my-api", "a1", "x-y-z"] {
            assert!(is_valid_bake_name(name), "{name} should be valid");
        }
        for name in ["1abc", "Abc", "a_b", "-foo", ""] {
            assert!(!is_valid_bake_name(name), "{name} should be invalid");
        }
    }

    #[test]
    fn baked_to_argv_spec() {
        let cfg = BakedTool {
            source_type: "spec".into(),
            source: "https://example.com/spec.json".into(),
            base_url: Some("https://api.example.com".into()),
            auth_headers: vec![("Authorization".into(), "env:TOKEN".into())],
            cache_ttl: 7200,
            ..Default::default()
        };
        let argv = cfg.to_argv();
        assert!(argv.contains(&"--spec".into()));
        assert!(argv.contains(&"https://example.com/spec.json".into()));
        assert!(argv.contains(&"--base-url".into()));
        assert!(argv.contains(&"--auth-header".into()));
        assert!(argv.contains(&"Authorization:env:TOKEN".into()));
        assert!(argv.contains(&"--cache-ttl".into()));
        assert!(argv.contains(&"7200".into()));
    }

    #[test]
    fn baked_to_argv_mcp_stdio() {
        let mut env = BTreeMap::new();
        env.insert("GH_TOKEN".into(), "abc".into());
        let cfg = BakedTool {
            source_type: "mcp_stdio".into(),
            source: "npx @mcp/github".into(),
            env_vars: env,
            cache_ttl: 3600,
            ..Default::default()
        };
        let argv = cfg.to_argv();
        assert!(argv.contains(&"--mcp-stdio".into()));
        assert!(argv.contains(&"npx @mcp/github".into()));
        assert!(argv.contains(&"--env".into()));
        assert!(argv.contains(&"GH_TOKEN=abc".into()));
    }

    #[test]
    fn baked_to_argv_oauth() {
        let cfg = BakedTool {
            source_type: "mcp".into(),
            source: "https://mcp.example.com".into(),
            transport: "sse".into(),
            oauth: true,
            oauth_client_id: Some("env:CID".into()),
            oauth_client_secret: Some("env:CSEC".into()),
            oauth_scope: Some("read write".into()),
            ..Default::default()
        };
        let argv = cfg.to_argv();
        assert!(argv.contains(&"--oauth".into()));
        assert!(argv.contains(&"--oauth-client-id".into()));
        assert!(argv.contains(&"--oauth-client-secret".into()));
        assert!(argv.contains(&"--oauth-scope".into()));
        assert!(argv.contains(&"--transport".into()));
        assert!(argv.contains(&"sse".into()));
        assert!(!argv.iter().any(|a| a == "--oauth-redirect-uri"));
    }

    #[test]
    fn baked_to_argv_oauth_redirect() {
        let uri = "http://localhost:18080/oauth/callback";
        let cfg = BakedTool {
            source_type: "mcp".into(),
            source: "https://mcp.example.com".into(),
            oauth: true,
            oauth_redirect_uri: Some(uri.into()),
            ..Default::default()
        };
        let argv = cfg.to_argv();
        let idx = argv
            .iter()
            .position(|a| a == "--oauth-redirect-uri")
            .unwrap();
        assert_eq!(argv[idx + 1], uri);
    }

    #[test]
    fn baked_to_argv_session() {
        let cfg = BakedTool {
            source_type: "mcp_stdio".into(),
            source: "python3 server.py".into(),
            session: Some("warm".into()),
            ..Default::default()
        };
        let argv = cfg.to_argv();
        assert!(argv.windows(2).any(|w| w == ["--session", "warm"]));
    }

    #[test]
    fn round_trip_store() {
        let _g = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempdir().unwrap();
        set_config_dir_override(Some(dir.path().to_path_buf()));
        let tool = BakedTool {
            source_type: "spec".into(),
            source: "https://example.com/spec.json".into(),
            ..Default::default()
        };
        create_baked("test", tool.clone(), false).unwrap();
        let loaded = require_baked("test").unwrap();
        assert_eq!(loaded.source_type, "spec");
        assert_eq!(loaded.source, tool.source);
        set_config_dir_override(None);
    }

    #[test]
    fn load_missing() {
        let _g = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempdir().unwrap();
        set_config_dir_override(Some(dir.path().join("nope")));
        assert!(load_baked_all().unwrap().is_empty());
        set_config_dir_override(None);
    }
}
