//! Compact tool catalog index for warm `--search` / `--detail names`.
//!
//! Full MCP tool lists embed every `inputSchema` (multi‑MB). We store a small
//! parallel index:
//!
//! - **names** (sorted) — exact/prefix via binary search
//! - **tool_overrides** — sparse MCP names when they are not kebab→snake
//! - **postings** — kebab-segment → tool ids (in memory only; rebuilt on load)
//! - **descs** — optional truncated text (omitted from v4 disk by default)
//!
//! Session daemons keep this struct in RAM and search in-process. Disk is a
//! thin non-session accelerator (names + overrides only).
//!
//! The inverted `postings` map is the KV shape we need at ~3k tools. An embedded
//! KV engine (redb/sled) is not worth the deps unless catalogs grow far larger
//! or we need a shared index without a session daemon.
//!
//! A BST does **not** help arbitrary substring search. Sorted names + postings
//! are the right shape for CLI discovery.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::{load_cached, save_cache};
use crate::coerce::to_kebab;
use crate::error::Result;
use crate::model::CommandDef;

/// Max description chars retained when building with `with_descs: true`.
pub const DESC_TRUNCATE: usize = 80;

/// On-disk format: names + sparse overrides; postings rebuilt in memory.
pub const INDEX_VERSION: u32 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolIndexEntry {
    pub name: String,
    pub tool_name: String,
    pub description: String,
}

/// Compact index (v4 disk omits `descs`/`postings`; v2/v3 still load).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactIndex {
    pub v: u32,
    /// Sorted kebab CLI names (binary-searchable for exact/prefix).
    pub names: Vec<String>,
    /// Legacy v2: parallel MCP tool names (omitted in v3+).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_names: Vec<String>,
    /// Sparse MCP names when `tool_name != names[i].replace('-', '_')`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_overrides: BTreeMap<u32, String>,
    /// Truncated descriptions; empty on v4 disk / names-only builds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descs: Vec<String>,
    /// Kebab segment → tool indices. Not persisted in v4; rebuild on load.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub postings: BTreeMap<String, Vec<u32>>,
}

fn default_tool_name_from_kebab(kebab: &str) -> String {
    kebab.replace('-', "_")
}

fn tokens_of(name: &str) -> impl Iterator<Item = &str> {
    name.split('-').filter(|t| !t.is_empty())
}

/// Rebuild inverted postings from `names` (used after v4 disk load).
pub fn rebuild_postings(index: &mut CompactIndex) {
    let mut postings: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (i, name) in index.names.iter().enumerate() {
        for tok in tokens_of(name) {
            postings.entry(tok.to_string()).or_default().push(i as u32);
        }
    }
    index.postings = postings;
}

impl CompactIndex {
    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn has_descs(&self) -> bool {
        !self.descs.is_empty()
    }

    pub fn tool_name_at(&self, i: usize) -> String {
        if let Some(t) = self.tool_overrides.get(&(i as u32)) {
            return t.clone();
        }
        if let Some(t) = self
            .tool_names
            .get(i)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
        {
            return t.to_string();
        }
        default_tool_name_from_kebab(&self.names[i])
    }

    pub fn desc_at(&self, i: usize) -> &str {
        self.descs.get(i).map(String::as_str).unwrap_or("")
    }

    pub fn entry_at(&self, i: usize) -> ToolIndexEntry {
        ToolIndexEntry {
            name: self.names[i].clone(),
            tool_name: self.tool_name_at(i),
            description: self.desc_at(i).to_string(),
        }
    }

    pub fn to_entries(&self) -> Vec<ToolIndexEntry> {
        (0..self.len()).map(|i| self.entry_at(i)).collect()
    }

    /// MCP-shaped light tools (`name` = wire tool name) for IPC / light commands.
    pub fn to_light_tools_json(&self, entries: &[ToolIndexEntry]) -> Value {
        Value::Array(
            entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "name": e.tool_name,
                        "description": e.description,
                    })
                })
                .collect(),
        )
    }
}

pub fn index_cache_key(tools_cache_key: &str) -> String {
    format!("{tools_cache_key}_index")
}

fn truncate_desc(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    if let Some(i) = out.rfind(' ') {
        out.truncate(i);
    }
    out.push_str("...");
    out
}

pub fn build_compact_index(tools: &[Value], with_descs: bool) -> CompactIndex {
    let mut rows: Vec<(String, String, String)> = Vec::with_capacity(tools.len());
    for tool in tools {
        let tool_name = tool
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let name = to_kebab(&tool_name);
        let description = if with_descs {
            truncate_desc(
                tool.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                DESC_TRUNCATE,
            )
        } else {
            String::new()
        };
        rows.push((name, tool_name, description));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    let mut names = Vec::with_capacity(rows.len());
    let mut tool_overrides: BTreeMap<u32, String> = BTreeMap::new();
    let mut descs = Vec::new();
    let mut postings: BTreeMap<String, Vec<u32>> = BTreeMap::new();

    for (i, (name, tool_name, description)) in rows.into_iter().enumerate() {
        let guessed = default_tool_name_from_kebab(&name);
        if tool_name != guessed {
            tool_overrides.insert(i as u32, tool_name);
        }
        for tok in tokens_of(&name) {
            postings.entry(tok.to_string()).or_default().push(i as u32);
        }
        names.push(name);
        if with_descs {
            descs.push(description);
        }
    }

    CompactIndex {
        v: INDEX_VERSION,
        names,
        tool_names: Vec::new(),
        tool_overrides,
        descs,
        postings,
    }
}

pub fn build_index(tools: &[Value]) -> Vec<ToolIndexEntry> {
    build_compact_index(tools, true).to_entries()
}

/// Persist names + overrides only (no descs / postings) for a slim disk sidecar.
pub fn save_index(tools_cache_key: &str, tools: &[Value]) -> Result<()> {
    let mut index = build_compact_index(tools, false);
    index.postings.clear();
    index.descs.clear();
    save_cache(
        &index_cache_key(tools_cache_key),
        &serde_json::to_value(&index)?,
    )
}

pub fn load_compact_index(tools_cache_key: &str, ttl: u64) -> Result<Option<CompactIndex>> {
    let Some(v) = load_cached(&index_cache_key(tools_cache_key), ttl)? else {
        return Ok(None);
    };
    let ver = v.get("v").and_then(|x| x.as_u64());
    if matches!(ver, Some(2) | Some(3) | Some(4)) && v.get("names").is_some() {
        let mut idx: CompactIndex = serde_json::from_value(v)?;
        if idx.postings.is_empty() {
            rebuild_postings(&mut idx);
        }
        return Ok(Some(idx));
    }
    // Legacy v1: array of {name,tool_name,description}
    if let Ok(entries) = serde_json::from_value::<Vec<ToolIndexEntry>>(v) {
        return Ok(Some(legacy_entries_to_compact(entries)));
    }
    Ok(None)
}

fn legacy_entries_to_compact(mut entries: Vec<ToolIndexEntry>) -> CompactIndex {
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let tools: Vec<Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.tool_name,
                "description": e.description,
            })
        })
        .collect();
    build_compact_index(&tools, true)
}

pub fn load_index(tools_cache_key: &str, ttl: u64) -> Result<Option<Vec<ToolIndexEntry>>> {
    Ok(load_compact_index(tools_cache_key, ttl)?.map(|c| c.to_entries()))
}

pub fn save_tools_and_index(tools_cache_key: &str, tools: &[Value]) -> Result<()> {
    save_cache(tools_cache_key, &Value::Array(tools.to_vec()))?;
    save_index(tools_cache_key, tools)?;
    Ok(())
}

pub fn index_to_commands(entries: &[ToolIndexEntry]) -> Vec<CommandDef> {
    entries
        .iter()
        .map(|e| CommandDef {
            name: e.name.clone(),
            description: e.description.clone(),
            tool_name: Some(e.tool_name.clone()),
            ..Default::default()
        })
        .collect()
}

pub fn tools_to_light_commands(tools: &[Value]) -> Vec<CommandDef> {
    index_to_commands(&build_index(tools))
}

/// Search using postings when `pattern` is a single kebab token; else scan names.
pub fn search_compact(index: &CompactIndex, pattern: &str) -> Vec<ToolIndexEntry> {
    let p = pattern.to_lowercase();
    let mut ids = if !p.is_empty() && !p.contains('-') && !p.contains(' ') {
        if let Some(list) = index.postings.get(&p) {
            list.clone()
        } else {
            let mut acc: Vec<u32> = index
                .postings
                .iter()
                .filter(|(k, _)| k.starts_with(&p) || k.contains(&p))
                .flat_map(|(_, v)| v.iter().copied())
                .collect();
            acc.sort_unstable();
            acc.dedup();
            if acc.is_empty() {
                return scan_names(index, &p);
            }
            acc
        }
    } else {
        return scan_names(index, &p);
    };

    ids.sort_unstable();
    ids.dedup();

    ids.into_iter()
        .filter_map(|i| {
            let i = i as usize;
            (i < index.len()).then(|| index.entry_at(i))
        })
        .collect()
}

fn scan_names(index: &CompactIndex, p: &str) -> Vec<ToolIndexEntry> {
    index
        .names
        .iter()
        .enumerate()
        .filter(|(i, n)| {
            n.contains(p)
                || index.tool_name_at(*i).to_lowercase().contains(p)
                || index.desc_at(*i).to_lowercase().contains(p)
        })
        .map(|(i, _)| index.entry_at(i))
        .collect()
}

pub fn search_index(entries: &[ToolIndexEntry], pattern: &str) -> Vec<ToolIndexEntry> {
    let p = pattern.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&p)
                || e.tool_name.to_lowercase().contains(&p)
                || e.description.to_lowercase().contains(&p)
        })
        .cloned()
        .collect()
}

/// Exact name lookup via binary search on sorted names.
pub fn find_exact(index: &CompactIndex, name: &str) -> Option<ToolIndexEntry> {
    index
        .names
        .binary_search_by(|n| n.as_str().cmp(name))
        .ok()
        .map(|i| index.entry_at(i))
}

/// Warm-path commands from disk index.
///
/// When `require_descs` is true (`--detail brief`) and the index has no
/// descriptions (v4 default), returns `None` so the caller can fall through.
pub fn try_commands_from_index(
    tools_cache_key: &str,
    ttl: u64,
    search: Option<&str>,
    require_descs: bool,
) -> Result<Option<Vec<CommandDef>>> {
    let Some(index) = load_compact_index(tools_cache_key, ttl)? else {
        return Ok(None);
    };
    if require_descs && !index.has_descs() {
        return Ok(None);
    }
    let entries = if let Some(pat) = search {
        search_compact(&index, pat)
    } else {
        index.to_entries()
    };
    Ok(Some(index_to_commands(&entries)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_and_search_postings() {
        let tools = vec![
            json!({"name": "workers_list", "description": "List workers", "inputSchema": {"type":"object","properties":{"a":{"type":"string"}}}}),
            json!({"name": "dns_records", "description": "DNS stuff", "inputSchema": {}}),
            json!({"name": "workers_scripts_get", "description": "Get script", "inputSchema": {}}),
        ];
        let idx = build_compact_index(&tools, false);
        assert_eq!(idx.len(), 3);
        assert!(idx.names.windows(2).all(|w| w[0] <= w[1]));
        assert!(idx.postings.contains_key("workers"));
        let hit = search_compact(&idx, "workers");
        assert_eq!(hit.len(), 2);
        assert!(find_exact(&idx, "dns-records").is_some());
    }

    #[test]
    fn v4_disk_omits_postings_rebuild_on_load() {
        let tools = vec![
            json!({"name": "workers_list", "description": "List workers"}),
            json!({"name": "dns_records", "description": "DNS"}),
        ];
        let mut disk = build_compact_index(&tools, false);
        disk.postings.clear();
        disk.descs.clear();
        let v = serde_json::to_value(&disk).unwrap();
        assert!(
            v.get("postings").is_none()
                || v.get("postings").unwrap().as_object().unwrap().is_empty()
        );
        let mut loaded: CompactIndex = serde_json::from_value(v).unwrap();
        assert!(loaded.postings.is_empty());
        rebuild_postings(&mut loaded);
        assert!(loaded.postings.contains_key("workers"));
        assert_eq!(search_compact(&loaded, "workers").len(), 1);
    }

    #[test]
    fn compact_disk_smaller_than_naive_array() {
        let tools: Vec<Value> = (0..50)
            .map(|i| {
                json!({
                    "name": format!("workers_scripts_op_{i}"),
                    "description": "x".repeat(200),
                    "inputSchema": {"type":"object","properties":{"a":{"type":"string"},"b":{"type":"integer"}}},
                })
            })
            .collect();
        let mut disk = build_compact_index(&tools, false);
        disk.postings.clear();
        let compact = serde_json::to_vec(&disk).unwrap();
        let naive = serde_json::to_vec(&build_index(&tools)).unwrap();
        assert!(
            compact.len() < naive.len() / 2,
            "disk compact {} vs naive {}",
            compact.len(),
            naive.len()
        );
    }

    #[test]
    fn snake_default_skips_tool_names_array() {
        let tools = vec![
            json!({"name": "workers_list", "description": "a"}),
            json!({"name": "get_accounts_scim_v2_Groups", "description": "b"}),
        ];
        let idx = build_compact_index(&tools, false);
        assert!(idx.tool_names.is_empty());
        let workers = idx.names.iter().position(|n| n == "workers-list").unwrap();
        assert_eq!(idx.tool_name_at(workers), "workers_list");
        let groups = idx
            .names
            .iter()
            .position(|n| n == "get-accounts-scim-v2-groups")
            .unwrap();
        assert_eq!(idx.tool_name_at(groups), "get_accounts_scim_v2_Groups");
        assert_eq!(idx.tool_overrides.len(), 1);
    }

    #[test]
    fn require_descs_skips_names_only_index() {
        let tools = vec![json!({"name": "workers_list", "description": "hi"})];
        let idx = build_compact_index(&tools, false);
        assert!(!idx.has_descs());
    }
}
