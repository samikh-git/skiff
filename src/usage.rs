//! Local usage tracking for `--sort usage|recent`.

use std::collections::BTreeMap;
use std::fs;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cache::atomic_write_json;
use crate::error::Result;
use crate::model::CommandDef;
use crate::paths::usage_file;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageEntry {
    pub count: u64,
    pub last_used: String,
}

pub type UsageBucket = BTreeMap<String, UsageEntry>;
pub type UsageData = BTreeMap<String, UsageBucket>;

pub fn load_usage() -> UsageData {
    let path = usage_file();
    if !path.exists() {
        return UsageData::new();
    }
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => UsageData::new(),
    }
}

pub fn save_usage(data: &UsageData) -> Result<()> {
    atomic_write_json(&usage_file(), data)
}

pub fn record_usage(source_hash: &str, tool_name: &str) -> Result<()> {
    let mut usage = load_usage();
    let bucket = usage.entry(source_hash.to_string()).or_default();
    let entry = bucket.entry(tool_name.to_string()).or_default();
    entry.count += 1;
    entry.last_used = Utc::now().to_rfc3339();
    save_usage(&usage)
}

pub fn source_hash_for(source: &str) -> String {
    let digest = Sha256::digest(source.as_bytes());
    hex::encode(&digest[..8])
}

fn usage_key(c: &CommandDef) -> &str {
    c.tool_name
        .as_deref()
        .or(c.graphql_field_name.as_deref())
        .unwrap_or(c.name.as_str())
}

pub fn sort_commands(
    mut commands: Vec<CommandDef>,
    sort_mode: &str,
    source_hash: &str,
) -> Vec<CommandDef> {
    match sort_mode {
        "default" => commands,
        "alpha" => {
            commands.sort_by(|a, b| a.name.cmp(&b.name));
            commands
        }
        "usage" | "recent" => {
            let usage = load_usage();
            let bucket = usage.get(source_hash);
            if bucket.is_none() {
                return commands;
            }
            let bucket = bucket.unwrap();
            if sort_mode == "usage" {
                commands.sort_by(|a, b| {
                    let ca = bucket.get(usage_key(a)).map(|e| e.count).unwrap_or(0);
                    let cb = bucket.get(usage_key(b)).map(|e| e.count).unwrap_or(0);
                    cb.cmp(&ca)
                });
            } else {
                commands.sort_by(|a, b| {
                    let la = bucket
                        .get(usage_key(a))
                        .map(|e| e.last_used.as_str())
                        .unwrap_or("");
                    let lb = bucket
                        .get(usage_key(b))
                        .map(|e| e.last_used.as_str())
                        .unwrap_or("");
                    lb.cmp(la)
                });
            }
            commands
        }
        _ => commands,
    }
}

pub fn resolve_sort_mode(explicit: Option<&str>, source_hash: &str) -> String {
    if let Some(s) = explicit {
        return s.to_string();
    }
    let usage = load_usage();
    if usage.get(source_hash).is_some_and(|b| !b.is_empty()) {
        "usage".into()
    } else {
        "default".into()
    }
}

/// Emit usage file contents as JSON Value (tests / debugging).
pub fn usage_as_value() -> Value {
    serde_json::to_value(load_usage()).unwrap_or(Value::Object(Default::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{set_cache_dir_override, set_config_dir_override, TEST_PATHS_LOCK};
    use tempfile::tempdir;

    fn with_tmp_cache<F: FnOnce()>(f: F) {
        let _guard = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempdir().unwrap();
        set_cache_dir_override(Some(dir.path().to_path_buf()));
        std::fs::create_dir_all(dir.path()).unwrap();
        f();
        set_cache_dir_override(None);
        set_config_dir_override(None);
    }

    #[test]
    fn load_save_roundtrip_and_corrupt() {
        with_tmp_cache(|| {
            assert!(load_usage().is_empty());
            let mut data = UsageData::new();
            let mut bucket = UsageBucket::new();
            bucket.insert(
                "my-tool".into(),
                UsageEntry {
                    count: 5,
                    last_used: "2026-01-01T00:00:00+00:00".into(),
                },
            );
            data.insert("abc123".into(), bucket);
            save_usage(&data).unwrap();
            assert_eq!(load_usage()["abc123"]["my-tool"].count, 5);

            fs::write(usage_file(), "{bad json").unwrap();
            assert!(load_usage().is_empty());
        });
    }

    #[test]
    fn record_and_hash() {
        with_tmp_cache(|| {
            record_usage("src1", "tool-a").unwrap();
            record_usage("src1", "tool-a").unwrap();
            record_usage("src1", "tool-b").unwrap();
            record_usage("src2", "tool-a").unwrap();
            let u = load_usage();
            assert_eq!(u["src1"]["tool-a"].count, 2);
            assert_eq!(u["src1"]["tool-b"].count, 1);
            assert_eq!(u["src2"]["tool-a"].count, 1);

            let h1 = source_hash_for("http://example.com");
            let h2 = source_hash_for("http://example.com");
            assert_eq!(h1, h2);
            assert_eq!(h1.len(), 16);
            assert_ne!(
                source_hash_for("http://example.com"),
                source_hash_for("http://other.com")
            );
        });
    }

    fn make_commands(names: &[&str]) -> Vec<CommandDef> {
        names
            .iter()
            .map(|n| {
                let mut c = CommandDef::new(*n);
                c.tool_name = Some((*n).into());
                c.description = format!("desc-{n}");
                c
            })
            .collect()
    }

    #[test]
    fn sort_modes() {
        with_tmp_cache(|| {
            let cmds = make_commands(&["c", "a", "b"]);
            let names: Vec<_> = sort_commands(cmds.clone(), "default", "src1")
                .into_iter()
                .map(|c| c.name)
                .collect();
            assert_eq!(names, ["c", "a", "b"]);

            let names: Vec<_> = sort_commands(cmds.clone(), "alpha", "src1")
                .into_iter()
                .map(|c| c.name)
                .collect();
            assert_eq!(names, ["a", "b", "c"]);

            record_usage("src1", "b").unwrap();
            record_usage("src1", "b").unwrap();
            record_usage("src1", "a").unwrap();
            let names: Vec<_> = sort_commands(make_commands(&["c", "a", "b"]), "usage", "src1")
                .into_iter()
                .map(|c| c.name)
                .collect();
            assert_eq!(names[0], "b");
            assert_eq!(names[1], "a");

            assert_eq!(resolve_sort_mode(Some("alpha"), "src1"), "alpha");
            assert_eq!(resolve_sort_mode(None, "src1"), "usage");
            assert_eq!(resolve_sort_mode(None, "empty"), "default");
        });
    }
}
