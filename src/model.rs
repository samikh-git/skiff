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

/// Serialize a command for `--list --json`.
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
