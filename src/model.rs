//! Shared command/parameter model (OpenAPI + MCP + GraphQL).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamLocation {
    Path,
    Query,
    Header,
    Body,
    File,
    ToolInput,
    GraphqlArg,
}

impl ParamLocation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Query => "query",
            Self::Header => "header",
            Self::Body => "body",
            Self::File => "file",
            Self::ToolInput => "tool_input",
            Self::GraphqlArg => "graphql_arg",
        }
    }
}

/// CLI parameter type: `None` means boolean `store_true` flag (Python `python_type is None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Boolean,
    Integer,
    Float,
    String,
}

impl ParamType {
    pub fn type_name(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Integer => "int",
            Self::Float => "float",
            Self::String => "str",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParamDef {
    /// kebab-case CLI flag name
    pub name: String,
    /// original name for API/tool call
    pub original_name: String,
    pub python_type: ParamType,
    pub required: bool,
    pub description: String,
    pub choices: Option<Vec<String>>,
    pub location: ParamLocation,
    pub schema: Value,
}

#[derive(Debug, Clone, Default)]
pub struct CommandDef {
    pub name: String,
    pub description: String,
    pub params: Vec<ParamDef>,
    pub has_body: bool,
    // OpenAPI
    pub method: Option<String>,
    pub path: Option<String>,
    pub content_type: Option<String>,
    // MCP
    pub tool_name: Option<String>,
    // GraphQL
    pub graphql_operation_type: Option<String>,
    pub graphql_field_name: Option<String>,
    pub graphql_return_type: Option<Value>,
}

impl CommandDef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BakeConfig {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub methods: Vec<String>,
}

/// Map JSON Schema type to CLI param type + help suffix (Python `schema_type_to_python`).
pub fn schema_type_to_python(schema: &Value) -> (ParamType, &'static str) {
    match schema.get("type").and_then(|t| t.as_str()) {
        Some("integer") => (ParamType::Integer, ""),
        Some("number") => (ParamType::Float, ""),
        Some("boolean") => (ParamType::Boolean, ""),
        Some("array") => (ParamType::String, " (JSON array)"),
        Some("object") => (ParamType::String, " (JSON object)"),
        _ => (ParamType::String, ""),
    }
}

/// Serialize a parameter for `--list --json`.
pub fn param_to_json(p: &ParamDef) -> Value {
    let mut d = serde_json::json!({
        "name": p.name,
        "type": p.python_type.type_name(),
        "required": p.required,
        "description": p.description,
        "location": p.location.as_str(),
    });
    if let Some(choices) = &p.choices {
        d["choices"] = Value::Array(choices.iter().cloned().map(Value::String).collect());
    }
    d
}

/// List / help JSON detail level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListDetail {
    /// Tool names only.
    Names,
    /// Name + short description (default for `--agent` lists).
    #[default]
    Brief,
    /// Full parameter schemas ([`command_to_json`]).
    Full,
}

impl ListDetail {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "names" => Some(Self::Names),
            "brief" => Some(Self::Brief),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// Truncate `s` to at most `max_chars` Unicode scalars, preferring a word boundary.
pub fn truncate_at_word(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    match truncated.rsplit_once(' ') {
        Some((head, _)) if !head.is_empty() => format!("{head}..."),
        _ => format!("{truncated}..."),
    }
}

/// Name + truncated description for progressive discovery.
pub fn command_to_brief_json(cmd: &CommandDef) -> Value {
    let desc = truncate_at_word(&cmd.description, 120);
    let mut d = serde_json::json!({
        "name": cmd.name,
        "description": desc,
    });
    if let Some(m) = &cmd.method {
        d["method"] = Value::String(m.to_uppercase());
    }
    if let Some(op) = &cmd.graphql_operation_type {
        d["operationType"] = Value::String(op.clone());
    }
    d
}

/// Serialize a command for `--list --json --detail full` / `TOOL --help --json`.
pub fn command_to_json(cmd: &CommandDef) -> Value {
    let mut d = serde_json::json!({
        "name": cmd.name,
        "description": cmd.description,
        "parameters": cmd.params.iter().map(param_to_json).collect::<Vec<_>>(),
    });
    if let Some(m) = &cmd.method {
        d["method"] = Value::String(m.to_uppercase());
    }
    if let Some(p) = &cmd.path {
        d["path"] = Value::String(p.clone());
    }
    if let Some(t) = &cmd.tool_name {
        d["toolName"] = Value::String(t.clone());
    }
    if let Some(op) = &cmd.graphql_operation_type {
        d["operationType"] = Value::String(op.clone());
    }
    d
}

/// Serialize commands at the requested detail level.
pub fn commands_to_json(commands: &[CommandDef], detail: ListDetail) -> Value {
    match detail {
        ListDetail::Names => names_list_json(commands),
        ListDetail::Brief => Value::Array(commands.iter().map(command_to_brief_json).collect()),
        ListDetail::Full => Value::Array(commands.iter().map(command_to_json).collect()),
    }
}

/// Flat name array, or prefix-compressed object when that saves space.
///
/// Compressed shape (agent-reconstructable):
/// `{"groups":{"workers-scripts":["list","get"]},"names":["echo"]}`
/// → tool id = `"{prefix}-{suffix}"` for groups, or bare `names` entries.
pub fn names_list_json(commands: &[CommandDef]) -> Value {
    let mut seen = std::collections::HashSet::new();
    let names: Vec<String> = commands
        .iter()
        .filter(|c| seen.insert(c.name.clone()))
        .map(|c| c.name.clone())
        .collect();
    compress_names(&names)
        .unwrap_or_else(|| Value::Array(names.into_iter().map(Value::String).collect()))
}

fn compress_names(names: &[String]) -> Option<Value> {
    // Dedupe while preserving order (fat catalogs can emit duplicate kebab ids).
    let mut seen = std::collections::HashSet::new();
    let names: Vec<String> = names
        .iter()
        .filter(|n| seen.insert((*n).clone()))
        .cloned()
        .collect();
    if names.len() < 8 {
        return None;
    }
    let mut groups: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut ungrouped: Vec<String> = Vec::new();

    for name in &names {
        match name.rfind('-') {
            Some(i) if i > 0 && i + 1 < name.len() => {
                let prefix = name[..i].to_string();
                let suffix = name[i + 1..].to_string();
                groups.entry(prefix).or_default().push(suffix);
            }
            _ => ungrouped.push(name.clone()),
        }
    }

    let mut kept = serde_json::Map::new();
    for (prefix, suffixes) in groups {
        // Unique suffixes within a group (duplicate full names → duplicate suffixes).
        let mut suf_seen = std::collections::HashSet::new();
        let suffixes: Vec<String> = suffixes
            .into_iter()
            .filter(|s| suf_seen.insert(s.clone()))
            .collect();
        if suffixes.len() >= 2 {
            kept.insert(
                prefix,
                Value::Array(suffixes.into_iter().map(Value::String).collect()),
            );
        } else if let Some(suf) = suffixes.into_iter().next() {
            ungrouped.push(format!("{prefix}-{suf}"));
        }
    }

    if kept.is_empty() {
        return None;
    }

    let compressed = {
        let mut m = serde_json::Map::new();
        m.insert("groups".into(), Value::Object(kept));
        if !ungrouped.is_empty() {
            m.insert(
                "names".into(),
                Value::Array(ungrouped.iter().cloned().map(Value::String).collect()),
            );
        }
        Value::Object(m)
    };
    let flat = Value::Array(names.iter().cloned().map(Value::String).collect());
    let c_len = serde_json::to_vec(&compressed)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    let f_len = serde_json::to_vec(&flat).map(|v| v.len()).unwrap_or(0);
    if c_len < f_len {
        Some(compressed)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_at_word_utf8_safe() {
        // Multi-byte chars must not panic on a mid-codepoint cut.
        let s = "éééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééééé";
        let out = truncate_at_word(s, 10);
        assert!(out.ends_with("..."));
        assert!(out.chars().count() <= 13); // 10 + "..."
    }

    #[test]
    fn truncate_prefers_word_boundary() {
        let s = "hello world this is a long description here";
        let out = truncate_at_word(s, 20);
        assert_eq!(out, "hello world this is...");
    }

    #[test]
    fn brief_json_truncates_long_desc() {
        let cmd = CommandDef {
            name: "t".into(),
            description: "a".repeat(200),
            ..Default::default()
        };
        let v = command_to_brief_json(&cmd);
        let d = v["description"].as_str().unwrap();
        assert!(d.len() < 200);
        assert!(d.ends_with("..."));
    }

    #[test]
    fn compresses_shared_prefixes() {
        let names: Vec<String> = (0..10)
            .map(|i| format!("workers-scripts-op{i}"))
            .chain(std::iter::once("echo".into()))
            .collect();
        let v = compress_names(&names).expect("should compress");
        assert!(v.get("groups").is_some());
        let flat_len = serde_json::to_vec(&Value::Array(
            names.iter().cloned().map(Value::String).collect(),
        ))
        .unwrap()
        .len();
        let c_len = serde_json::to_vec(&v).unwrap().len();
        assert!(c_len < flat_len);
    }

    #[test]
    fn compress_dedupes_duplicate_names() {
        let mut names: Vec<String> = (0..10)
            .map(|i| format!("accounts-workers-by-worker-{i}"))
            .collect();
        // Same kebab id twice (fat catalogs sometimes emit this).
        names.push("accounts-workers-by-worker-0".into());
        names.push("accounts-workers-by-worker-0".into());
        let v = names_list_json(
            &names
                .iter()
                .map(|n| CommandDef {
                    name: n.clone(),
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
        );
        if let Some(groups) = v.get("groups") {
            for (_, suffixes) in groups.as_object().unwrap() {
                let arr = suffixes.as_array().unwrap();
                let mut seen = std::collections::HashSet::new();
                for s in arr {
                    assert!(
                        seen.insert(s.as_str().unwrap()),
                        "duplicate suffix in compress groups: {s}"
                    );
                }
            }
        } else {
            let arr = v.as_array().unwrap();
            let mut seen = std::collections::HashSet::new();
            for n in arr {
                assert!(seen.insert(n.as_str().unwrap()));
            }
        }
    }
}
