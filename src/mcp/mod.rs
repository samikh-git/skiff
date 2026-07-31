//! MCP tool → CommandDef extraction (client/runtime wiring is feature-gated).

use serde_json::Value;

use crate::coerce::to_kebab;
use crate::model::{schema_type_to_python, CommandDef, ParamDef, ParamLocation};

/// Convert MCP `list_tools` entries into CLI commands.
///
/// Missing/`null` `inputSchema` is treated as `{}` (upstream issue #69).
pub fn extract_mcp_commands(tools: &[Value]) -> Vec<CommandDef> {
    let mut commands = Vec::new();
    for tool in tools {
        let tool_name = tool
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let name = to_kebab(tool_name);
        let desc = tool
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let schema = tool
            .get("inputSchema")
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));

        let required_fields: std::collections::HashSet<String> = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let mut params = Vec::new();
        if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
            for (prop_name, prop_schema) in props {
                let (py_type, suffix) = schema_type_to_python(prop_schema);
                let description = format!(
                    "{}{suffix}",
                    prop_schema
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or(prop_name)
                );
                let choices = prop_schema.get("enum").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                });
                params.push(ParamDef {
                    name: to_kebab(prop_name),
                    original_name: prop_name.clone(),
                    python_type: py_type,
                    required: required_fields.contains(prop_name),
                    description,
                    choices,
                    location: ParamLocation::ToolInput,
                    schema: prop_schema.clone(),
                });
            }
        }

        let has_body = !params.is_empty();
        commands.push(CommandDef {
            name,
            description: desc,
            params,
            has_body,
            tool_name: Some(tool_name.to_string()),
            ..Default::default()
        });
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_and_handles_null_schema() {
        let tools = vec![
            json!({
                "name": "list_tasks",
                "description": "List tasks",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "status": {"type": "string", "enum": ["TO_DO", "DONE"]}
                    },
                    "required": ["status"]
                }
            }),
            json!({
                "name": "noop",
                "description": "No inputs",
                "inputSchema": null
            }),
        ];
        let cmds = extract_mcp_commands(&tools);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name, "list-tasks");
        assert_eq!(cmds[0].params.len(), 1);
        assert!(cmds[0].params[0].required);
        assert_eq!(cmds[1].name, "noop");
        assert!(cmds[1].params.is_empty());
    }
}
