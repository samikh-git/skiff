//! Cache and config directory resolution (Python-compatible layout).
//!
//! - Cache: `$MCP2CLI_CACHE_DIR` or `~/.cache/mcp2cli` (tool lists, GraphQL/OpenAPI,
//!   OAuth, sessions, usage)
//! - Config: `$MCP2CLI_CONFIG_DIR` or `~/.config/mcp2cli` (`baked.json`)
//!
//! Tests may override via [`set_cache_dir_override`] / [`set_config_dir_override`]
//! under [`TEST_PATHS_LOCK`].

use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};

static CACHE_DIR_OVERRIDE: LazyLock<RwLock<Option<PathBuf>>> = LazyLock::new(|| RwLock::new(None));
static CONFIG_DIR_OVERRIDE: LazyLock<RwLock<Option<PathBuf>>> = LazyLock::new(|| RwLock::new(None));

/// Override cache dir (for tests). `None` clears the override.
pub fn set_cache_dir_override(path: Option<PathBuf>) {
    *CACHE_DIR_OVERRIDE.write().expect("cache override lock") = path;
}

/// Override config dir (for tests). `None` clears the override.
pub fn set_config_dir_override(path: Option<PathBuf>) {
    *CONFIG_DIR_OVERRIDE.write().expect("config override lock") = path;
}

pub fn cache_dir() -> PathBuf {
    if let Some(p) = CACHE_DIR_OVERRIDE
        .read()
        .expect("cache override lock")
        .clone()
    {
        return p;
    }
    if let Ok(p) = std::env::var("MCP2CLI_CACHE_DIR") {
        return PathBuf::from(p);
    }
    home_dir()
        .map(|h| h.join(".cache").join("mcp2cli"))
        .unwrap_or_else(|| PathBuf::from(".cache/mcp2cli"))
}

pub fn config_dir() -> PathBuf {
    if let Some(p) = CONFIG_DIR_OVERRIDE
        .read()
        .expect("config override lock")
        .clone()
    {
        return p;
    }
    if let Ok(p) = std::env::var("MCP2CLI_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    home_dir()
        .map(|h| h.join(".config").join("mcp2cli"))
        .unwrap_or_else(|| PathBuf::from(".config/mcp2cli"))
}

pub fn usage_file() -> PathBuf {
    cache_dir().join("usage.json")
}

pub fn baked_file() -> PathBuf {
    config_dir().join("baked.json")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub const DEFAULT_CACHE_TTL: u64 = 3600;

/// Shared lock so tests that override cache/config dirs don't race.
#[cfg(test)]
pub static TEST_PATHS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
