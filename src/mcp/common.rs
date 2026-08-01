//! Shared helpers for MCP clients (stdio / HTTP / SSE).

use rmcp::model::CallToolResult;
use serde_json::{json, Map, Value};

use crate::error::{Error, Result};
use crate::mcp::extract_mcp_commands;
use crate::model::CommandDef;
use crate::spool::write_spool;

pub type McpClient = rmcp::service::RunningService<rmcp::RoleClient, ()>;

pub fn tools_from_rmcp(tools: impl IntoIterator<Item = rmcp::model::Tool>) -> Vec<Value> {
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

fn spool_base64_blob(data_b64: &str, mime: &str, kind: &str) -> Result<Value> {
    use base64::Engine;
    // Prefer decode when possible; otherwise store the base64 text.
    let raw = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .unwrap_or_else(|_| data_b64.as_bytes().to_vec());
    let path = write_spool(&raw, "bin")?;
    eprintln!(
        "mcp2cli: spooled {kind} content ({mime}, {} bytes) to {}",
        raw.len(),
        path.display()
    );
    Ok(json!({
        "type": kind,
        "mimeType": mime,
        "bytes": raw.len(),
        "path": path.display().to_string(),
        "spooled": true,
        "hint": format!("file '{}'", path.display()),
    }))
}

/// Format a tool result for agents: content-only by default; spool non-text blobs.
pub fn format_tool_result(result: &CallToolResult, full_envelope: bool) -> Result<Value> {
    if full_envelope {
        return serde_json::to_value(result).map_err(|e| Error::runtime(e.to_string()));
    }

    let mut texts = Vec::new();
    let mut extras = Vec::new();

    for block in &result.content {
        if let Some(t) = block.as_text() {
            texts.push(t.text.clone());
            continue;
        }
        if let Some(img) = block.as_image() {
            extras.push(spool_base64_blob(&img.data, &img.mime_type, "image")?);
            continue;
        }
        if let Some(audio) = block.as_audio() {
            extras.push(spool_base64_blob(&audio.data, &audio.mime_type, "audio")?);
            continue;
        }
        // resource / resource_link / unknown — stub without dumping
        extras.push(json!({
            "type": "other",
            "note": "non-text content omitted; use --envelope for wire form",
        }));
    }

    if texts.is_empty() && extras.is_empty() {
        return Ok(Value::Null);
    }

    if texts.is_empty() {
        return Ok(if extras.len() == 1 {
            extras.pop().unwrap()
        } else {
            Value::Array(extras)
        });
    }

    let text = texts.join("\n");
    let text_val = if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
        parsed
    } else {
        Value::String(text)
    };

    if extras.is_empty() {
        return Ok(text_val);
    }

    Ok(json!({
        "text": text_val,
        "attachments": extras,
    }))
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
