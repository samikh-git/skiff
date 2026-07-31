//! Shared helpers for MCP clients (stdio / HTTP / SSE).

use rmcp::model::CallToolResult;
use serde_json::{json, Map, Value};

use crate::error::{Error, Result};
use crate::mcp::extract_mcp_commands;
use crate::model::CommandDef;

pub type McpClient = rmcp::service::RunningService<rmcp::RoleClient, ()>;

pub fn tools_from_rmcp(
    tools: impl IntoIterator<Item = rmcp::model::Tool>,
) -> Vec<Value> {
    let mut out = Vec::new();
    for t in tools {
        let schema = serde_json::to_value(&t.input_schema)
            .unwrap_or_else(|_| json!({"type": "object", "properties": {}}));
        out.push(json!({
            "name": t.name,
            "description": t.description.clone().unwrap_or_default(),
            "inputSchema": schema,
        }));
    }
    out
}

pub fn format_tool_result(result: &CallToolResult, full_envelope: bool) -> Result<Value> {
    if full_envelope {
        return serde_json::to_value(result).map_err(|e| Error::runtime(e.to_string()));
    }

    let mut texts = Vec::new();
    for block in &result.content {
        if let Some(t) = block.as_text() {
            texts.push(t.text.clone());
        }
    }
    let text = texts.join("\n");
    if text.is_empty() {
        Ok(serde_json::to_value(result).unwrap_or(Value::Null))
    } else if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
        Ok(parsed)
    } else {
        Ok(Value::String(text))
    }
}

pub fn tools_to_commands(tools: &[Value]) -> Vec<CommandDef> {
    extract_mcp_commands(tools)
}

pub fn auth_headers_to_http(
    auth_headers: &[(String, String)],
) -> Result<std::collections::HashMap<http::HeaderName, http::HeaderValue>> {
    let mut map = std::collections::HashMap::new();
    for (name, value) in auth_headers {
        let hn = http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| Error::usage(format!("invalid auth header name {name:?}: {e}")))?;
        let hv = http::HeaderValue::from_str(value)
            .map_err(|e| Error::usage(format!("invalid auth header value for {name}: {e}")))?;
        map.insert(hn, hv);
    }
    Ok(map)
}

pub async fn list_tools_on(client: &McpClient) -> Result<Vec<Value>> {
    let tools = client
        .list_all_tools()
        .await
        .map_err(|e| Error::runtime(format!("list_tools failed: {e}")))?;
    Ok(tools_from_rmcp(tools))
}

pub async fn call_tool_on(
    client: &McpClient,
    tool_name: &str,
    arguments: Map<String, Value>,
    full_envelope: bool,
) -> Result<Value> {
    use rmcp::model::CallToolRequestParams;

    let params = CallToolRequestParams::new(tool_name.to_string()).with_arguments(arguments);
    let result = client
        .call_tool(params)
        .await
        .map_err(|e| Error::runtime(format!("call_tool failed: {e}")))?;
    format_tool_result(&result, full_envelope)
}
