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
        "skiff: spooled {kind} content ({mime}, {} bytes) to {}",
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

fn content_only_payload(result: &CallToolResult) -> Result<Value> {
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
        // Prefer structuredContent when the tool returned no content blocks.
        if let Some(sc) = &result.structured_content {
            return Ok(sc.clone());
        }
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

fn tool_error_message(payload: &Value) -> String {
    match payload {
        Value::Null => "MCP tool returned an error".into(),
        Value::String(s) => format!("MCP tool error: {s}"),
        other => format!(
            "MCP tool error: {}",
            serde_json::to_string(other).unwrap_or_else(|_| other.to_string())
        ),
    }
}

/// Format a tool result for agents: content-only by default; spool non-text blobs.
///
/// When `is_error` is set, returns [`Error::Runtime`] so the CLI exits non-zero.
/// Structured-only results (`structured_content` with empty `content`) are returned
/// as that JSON value in content-only mode.
pub fn format_tool_result(result: &CallToolResult, full_envelope: bool) -> Result<Value> {
    let value = if full_envelope {
        serde_json::to_value(result).map_err(|e| Error::runtime(e.to_string()))?
    } else {
        content_only_payload(result)?
    };

    if result.is_error == Some(true) {
        return Err(Error::runtime(tool_error_message(&value)));
    }
    Ok(value)
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

pub async fn list_resources_on(client: &McpClient) -> Result<Value> {
    let resources = client
        .list_all_resources()
        .await
        .map_err(|e| Error::runtime(format!("list_resources: {e}")))?;
    let arr: Vec<Value> = resources
        .into_iter()
        .map(|r| {
            json!({
                "name": r.name,
                "uri": r.uri,
                "description": r.description.unwrap_or_default(),
                "mimeType": r.mime_type.unwrap_or_default(),
            })
        })
        .collect();
    Ok(Value::Array(arr))
}

pub async fn list_resource_templates_on(client: &McpClient) -> Result<Value> {
    let templates = client
        .list_all_resource_templates()
        .await
        .map_err(|e| Error::runtime(format!("list_resource_templates: {e}")))?;
    Ok(serde_json::to_value(templates).unwrap_or(Value::Null))
}

pub async fn read_resource_on(client: &McpClient, uri: &str) -> Result<Value> {
    let result = client
        .read_resource(rmcp::model::ReadResourceRequestParams::new(uri))
        .await
        .map_err(|e| Error::runtime(format!("read_resource: {e}")))?;
    Ok(serde_json::to_value(result).unwrap_or(Value::Null))
}

pub async fn list_prompts_on(client: &McpClient) -> Result<Value> {
    let prompts = client
        .list_all_prompts()
        .await
        .map_err(|e| Error::runtime(format!("list_prompts: {e}")))?;
    Ok(serde_json::to_value(prompts).unwrap_or(Value::Null))
}

pub async fn get_prompt_on(
    client: &McpClient,
    name: &str,
    arguments: Map<String, Value>,
) -> Result<Value> {
    let mut params = rmcp::model::GetPromptRequestParams::new(name);
    if !arguments.is_empty() {
        params = params.with_arguments(arguments);
    }
    let result = client
        .get_prompt(params)
        .await
        .map_err(|e| Error::runtime(format!("get_prompt: {e}")))?;
    Ok(serde_json::to_value(result).unwrap_or(Value::Null))
}

/// True when any MCP resource/prompt discovery flag is set.
pub fn wants_mcp_extras(pre: &crate::cli::args::GlobalArgs) -> bool {
    pre.list_resources
        || pre.list_resource_templates
        || pre.read_resource.is_some()
        || pre.list_prompts
        || pre.get_prompt.is_some()
}

/// Run exactly one resource/prompt operation against a connected client.
pub async fn run_mcp_extras(
    client: &McpClient,
    pre: &crate::cli::args::GlobalArgs,
) -> Result<Value> {
    if pre.list_resources {
        return list_resources_on(client).await;
    }
    if pre.list_resource_templates {
        return list_resource_templates_on(client).await;
    }
    if let Some(uri) = &pre.read_resource {
        return read_resource_on(client, uri).await;
    }
    if pre.list_prompts {
        return list_prompts_on(client).await;
    }
    if let Some(pname) = &pre.get_prompt {
        let mut args = Map::new();
        for item in &pre.prompt_arg {
            let Some((k, v)) = item.split_once('=') else {
                return Err(Error::usage(format!(
                    "invalid --prompt-arg {item:?}; expected KEY=VALUE"
                )));
            };
            args.insert(k.to_string(), Value::String(v.to_string()));
        }
        return get_prompt_on(client, pname, args).await;
    }
    Err(Error::usage("no MCP resource/prompt operation requested"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolResult;
    use serde_json::json;

    fn parse_result(v: Value) -> CallToolResult {
        serde_json::from_value(v).expect("CallToolResult")
    }

    #[test]
    fn structured_only_content() {
        let r = parse_result(json!({
            "content": [],
            "structuredContent": {"ok": true, "n": 7}
        }));
        assert_eq!(
            format_tool_result(&r, false).unwrap(),
            json!({"ok": true, "n": 7})
        );
    }

    #[test]
    fn is_error_returns_runtime_err() {
        let r = parse_result(json!({
            "content": [{"type": "text", "text": "boom"}],
            "isError": true
        }));
        let err = format_tool_result(&r, false).unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn text_content_preferred_over_structured() {
        let r = parse_result(json!({
            "content": [{"type": "text", "text": "hello"}],
            "structuredContent": {"ignored": true}
        }));
        assert_eq!(format_tool_result(&r, false).unwrap(), json!("hello"));
    }

    #[test]
    fn envelope_includes_is_error_flag_on_success_path() {
        let r = parse_result(json!({
            "content": [{"type": "text", "text": "ok"}],
            "isError": false
        }));
        let v = format_tool_result(&r, true).unwrap();
        assert!(v.get("content").is_some());
    }
}
