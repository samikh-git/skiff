//! Bake config store stubs (CRUD filled in bake milestone).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::atomic_write_json;
use crate::error::{Error, Result};
use crate::paths::baked_file;

pub const BAKED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BakedTool {
    #[serde(default = "default_version")]
    pub version: u32,
    pub source_type: String,
    pub source: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub auth_headers: Vec<(String, String)>,
    #[serde(default)]
    pub env_vars: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub cache_ttl: Option<u64>,
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default)]
    pub oauth: bool,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, Value>,
}

fn default_version() -> u32 {
    BAKED_SCHEMA_VERSION
}

fn default_transport() -> String {
    "auto".into()
}

pub type BakedStore = std::collections::BTreeMap<String, BakedTool>;

pub fn load_baked_all() -> Result<BakedStore> {
    let path = baked_file();
    if !path.exists() {
        return Ok(BakedStore::new());
    }
    let text = std::fs::read_to_string(&path)?;
    match serde_json::from_str(&text) {
        Ok(store) => Ok(store),
        Err(_) => Ok(BakedStore::new()),
    }
}

pub fn save_baked_all(store: &BakedStore) -> Result<()> {
    atomic_write_json(&baked_file(), store)
}

pub fn get_baked(name: &str) -> Result<BakedTool> {
    load_baked_all()?
        .get(name)
        .cloned()
        .ok_or_else(|| Error::runtime(format!("no baked tool named '{name}'")))
}
