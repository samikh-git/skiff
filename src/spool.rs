//! Spill large / binary MCP payloads to `$MCP2CLI_CACHE_DIR/spool/` for agent grep.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::fsutil::{atomic_write_0600, ensure_dir_0700};
use crate::paths::cache_dir;

/// Default max rendered stdout bytes before spill (agent profile).
pub const DEFAULT_AGENT_MAX_BYTES: usize = 65_536;

/// Preview size included in spool pointer JSON.
pub const PREVIEW_BYTES: usize = 512;

/// Default spool file TTL (24h).
pub const DEFAULT_SPOOL_TTL_SECS: u64 = 86_400;

pub fn spool_dir() -> PathBuf {
    cache_dir().join("spool")
}

fn extension_for(kind: &str) -> &'static str {
    match kind {
        "toon" => "toon",
        "txt" => "txt",
        "bin" => "bin",
        _ => "json",
    }
}

/// Write `bytes` to a new spool file; returns absolute path.
pub fn write_spool(bytes: &[u8], kind: &str) -> Result<PathBuf> {
    let dir = spool_dir();
    ensure_dir_0700(&dir)?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hex::encode(hasher.finalize());
    let short = &digest[..12.min(digest.len())];
    let path = dir.join(format!("{secs}_{short}.{}", extension_for(kind)));
    atomic_write_0600(&path, bytes)?;
    Ok(path)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Build the small stdout pointer object for a spooled payload.
pub fn pointer_json(path: &Path, bytes: &[u8], preview_src: &str) -> Value {
    let preview: String = preview_src.chars().take(PREVIEW_BYTES).collect();
    let path_str = path.display().to_string();
    json!({
        "spooled": true,
        "path": path_str,
        "bytes": bytes.len(),
        "sha256": sha256_hex(bytes),
        "preview": preview,
        "hint": format!("rg -n PATTERN '{path_str}'"),
    })
}

/// Remove spool files older than `ttl_secs` (0 = delete all).
pub fn clean_spool(ttl_secs: u64) -> Result<usize> {
    let dir = spool_dir();
    if !dir.is_dir() {
        return Ok(0);
    }
    let now = SystemTime::now();
    let mut removed = 0usize;
    for entry in std::fs::read_dir(&dir).map_err(Error::from)? {
        let entry = entry.map_err(Error::from)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let meta = entry.metadata().map_err(Error::from)?;
        let age_ok = if ttl_secs == 0 {
            true
        } else {
            match meta.modified() {
                Ok(mtime) => now
                    .duration_since(mtime)
                    .map(|d| d.as_secs() >= ttl_secs)
                    .unwrap_or(true),
                Err(_) => true,
            }
        };
        if age_ok {
            let _ = std::fs::remove_file(&path);
            removed += 1;
        }
    }
    Ok(removed)
}

/// Best-effort cleanup of expired spool files (ignore errors).
pub fn maybe_clean_expired() {
    let _ = clean_spool(DEFAULT_SPOOL_TTL_SECS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{set_cache_dir_override, TEST_PATHS_LOCK};
    use tempfile::tempdir;

    #[test]
    fn write_and_clean() {
        let _g = TEST_PATHS_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        set_cache_dir_override(Some(dir.path().to_path_buf()));
        let path = write_spool(b"hello needle world", "txt").unwrap();
        assert!(path.exists());
        let ptr = pointer_json(&path, b"hello needle world", "hello needle world");
        assert_eq!(ptr["spooled"], true);
        assert!(ptr["path"].as_str().unwrap().contains("spool"));
        let n = clean_spool(0).unwrap();
        assert!(n >= 1);
        set_cache_dir_override(None);
    }
}
