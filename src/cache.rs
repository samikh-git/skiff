//! Atomic JSON cache under `$SKIFF_CACHE_DIR`.
//!
//! Writes use [`crate::fsutil::atomic_write_0600`]. Cache keys hash connection
//! config (excluding filter/TTL fields).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::fsutil::{atomic_write_0600, ensure_dir_0700};
use crate::paths::{cache_dir, DEFAULT_CACHE_TTL};

pub use crate::paths::DEFAULT_CACHE_TTL as DEFAULT_TTL;

/// Fields that do not affect which tools/spec are returned (Python `cache_key_for`).
const EXCLUDED_KEY_FIELDS: &[&str] = &["cache_ttl", "description", "include", "exclude", "methods"];

pub fn cache_key_for(config: &Value) -> String {
    let mut filtered = serde_json::Map::new();
    if let Some(obj) = config.as_object() {
        for (k, v) in obj {
            if EXCLUDED_KEY_FIELDS.contains(&k.as_str()) {
                continue;
            }
            let mut v = v.clone();
            if k == "auth_headers" {
                if let Some(arr) = v.as_array_mut() {
                    arr.sort_by_key(|a| a.to_string());
                }
            }
            filtered.insert(k.clone(), v);
        }
    }
    let canonical = Value::Object(filtered);
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    hex::encode(&digest[..8]) // 16 hex chars
}

fn cache_path(key: &str) -> PathBuf {
    cache_dir().join(format!("{key}.json"))
}

/// Atomically write JSON to `path` (temp + rename) with mode `0o600`.
pub fn atomic_write_json(path: &Path, data: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir_0700(parent)?;
    }
    let text = serde_json::to_string(data)?;
    atomic_write_0600(path, text.as_bytes())
}

pub fn save_cache(key: &str, data: &Value) -> Result<()> {
    atomic_write_json(&cache_path(key), data)
}

pub fn load_cached(key: &str, ttl: u64) -> Result<Option<Value>> {
    let path = cache_path(key);
    if !path.exists() {
        return Ok(None);
    }
    let meta = fs::metadata(&path)?;
    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default()
        .as_secs();
    if age >= ttl {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let data: Value = serde_json::from_str(&text)?;
    Ok(Some(data))
}

pub fn load_cached_default(key: &str) -> Result<Option<Value>> {
    load_cached(key, DEFAULT_CACHE_TTL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{set_cache_dir_override, TEST_PATHS_LOCK};
    use serde_json::json;
    use std::time::{Duration, SystemTime};
    use tempfile::tempdir;

    #[test]
    fn cache_key_deterministic_and_excludes_ttl() {
        let k1 = cache_key_for(&json!({"url": "https://example.com/spec.json"}));
        let k2 = cache_key_for(&json!({"url": "https://example.com/spec.json"}));
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), 16);

        let a = cache_key_for(&json!({"url": "https://example.com/a.json"}));
        let b = cache_key_for(&json!({"url": "https://example.com/b.json"}));
        assert_ne!(a, b);

        let t1 = cache_key_for(&json!({
            "url": "https://example.com/spec.json",
            "cache_ttl": 60
        }));
        let t2 = cache_key_for(&json!({
            "url": "https://example.com/spec.json",
            "cache_ttl": 3600
        }));
        assert_eq!(t1, t2);
    }

    #[test]
    fn cache_roundtrip_and_expiry() {
        let _guard = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempdir().unwrap();
        set_cache_dir_override(Some(dir.path().to_path_buf()));

        save_cache("testkey", &json!({"foo": "bar"})).unwrap();
        assert_eq!(
            load_cached("testkey", 3600).unwrap(),
            Some(json!({"foo": "bar"}))
        );
        assert!(load_cached("nonexistent", 3600).unwrap().is_none());

        save_cache("testkey", &json!({"a": 1})).unwrap();
        let cache_file = dir.path().join("testkey.json");
        let old = SystemTime::now() - Duration::from_secs(7200);
        filetime_set(&cache_file, old);
        assert!(load_cached("testkey", 3600).unwrap().is_none());

        set_cache_dir_override(None);
    }

    fn filetime_set(path: &Path, when: SystemTime) {
        let file = fs::File::open(path).unwrap();
        file.set_modified(when).unwrap();
    }
}
