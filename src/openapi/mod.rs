//! OpenAPI $ref resolution, command extraction, and execution.

mod execute;
mod extract;
mod refs;

pub use execute::{execute_openapi, resolve_base_url};
pub use extract::extract_openapi_commands;
pub use refs::resolve_refs;

use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::cache::{cache_key_for, load_cached, save_cache};
use crate::error::{Error, Result};
use crate::paths::DEFAULT_CACHE_TTL;

/// Load an OpenAPI spec from a local file (URL fetch comes with HTTP client wiring).
pub fn load_openapi_spec_file(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path)?;
    parse_spec(&raw)
}

pub fn parse_spec(raw: &str) -> Result<Value> {
    let spec: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => serde_yaml::from_str(raw)?,
    };
    if !spec.is_object() || spec.get("paths").is_none() {
        return Err(Error::runtime("spec must contain 'paths'"));
    }
    Ok(resolve_refs(spec))
}

/// Load from URL with caching (uses blocking reqwest for simplicity in sync API).
pub fn load_openapi_spec_url(
    url: &str,
    auth_headers: &[(String, String)],
    cache_key: Option<&str>,
    ttl: u64,
    refresh: bool,
) -> Result<Value> {
    let key = cache_key.map(str::to_string).unwrap_or_else(|| {
        cache_key_for(&serde_json::json!({
            "source": url,
            "auth_headers": auth_headers,
        }))
    });
    if !refresh {
        if let Some(cached) = load_cached(&key, ttl)? {
            return Ok(cached);
        }
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::runtime(e.to_string()))?;
    let mut req = client.get(url);
    for (k, v) in auth_headers {
        req = req.header(k, v);
    }
    let resp = req.send().map_err(|e| Error::runtime(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(Error::runtime(format!(
            "Error {}: {}",
            resp.status().as_u16(),
            resp.text().unwrap_or_default()
        )));
    }
    let raw = resp.text().map_err(|e| Error::runtime(e.to_string()))?;
    let spec = parse_spec(&raw)?;
    save_cache(&key, &spec)?;
    Ok(spec)
}

pub fn load_openapi_spec(
    source: &str,
    auth_headers: &[(String, String)],
    cache_key: Option<&str>,
    ttl: Option<u64>,
    refresh: bool,
) -> Result<Value> {
    let ttl = ttl.unwrap_or(DEFAULT_CACHE_TTL);
    if source.starts_with("http://") || source.starts_with("https://") {
        load_openapi_spec_url(source, auth_headers, cache_key, ttl, refresh)
    } else {
        load_openapi_spec_file(Path::new(source))
    }
}
