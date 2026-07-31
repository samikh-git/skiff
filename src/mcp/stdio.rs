//! MCP stdio client via rmcp.

use std::collections::BTreeMap;
use std::time::Duration;

use rmcp::{
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::{json, Map, Value};
use tokio::process::Command;

use crate::cache::{load_cached, save_cache};
use crate::error::{Error, Result};
use crate::mcp::extract_mcp_commands;
use crate::model::CommandDef;

type McpClient = rmcp::service::RunningService<rmcp::RoleClient, ()>;

pub async fn fetch_mcp_tools_stdio(
    command_str: &str,
    env_vars: &BTreeMap<String, String>,
    cache_key: &str,
    ttl: u64,
    refresh: bool,
) -> Result<Vec<Value>> {
    let tools_key = format!("{cache_key}_tools");
    if !refresh {
        if let Some(cached) = load_cached(&tools_key, ttl)? {
            if let Some(arr) = cached.as_array() {
                return Ok(arr.clone());
            }
        }
    }

    let tools = list_tools_stdio(command_str, env_vars).await?;
    save_cache(&tools_key, &Value::Array(tools.clone()))?;
    Ok(tools)
}

pub async fn list_tools_stdio(
    command_str: &str,
    env_vars: &BTreeMap<String, String>,
) -> Result<Vec<Value>> {
    let client = connect_stdio(command_str, env_vars).await?;
    let tools = client
        .list_all_tools()
        .await
        .map_err(|e| Error::runtime(format!("list_tools failed: {e}")))?;
    let _ = client.cancel().await;

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
    Ok(out)
}

pub async fn call_tool_stdio(
    command_str: &str,
    env_vars: &BTreeMap<String, String>,
    tool_name: &str,
    arguments: Map<String, Value>,
    full_envelope: bool,
) -> Result<Value> {
    let client = connect_stdio(command_str, env_vars).await?;
    let params = CallToolRequestParams::new(tool_name.to_string()).with_arguments(arguments);
    let result = client
        .call_tool(params)
        .await
        .map_err(|e| Error::runtime(format!("call_tool failed: {e}")))?;
    let _ = client.cancel().await;

    if full_envelope {
        return serde_json::to_value(&result).map_err(|e| Error::runtime(e.to_string()));
    }

    let mut texts = Vec::new();
    for block in &result.content {
        if let Some(t) = block.as_text() {
            texts.push(t.text.clone());
        }
    }
    let text = texts.join("\n");
    if text.is_empty() {
        Ok(serde_json::to_value(&result).unwrap_or(Value::Null))
    } else if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
        Ok(parsed)
    } else {
        Ok(Value::String(text))
    }
}

async fn connect_stdio(
    command_str: &str,
    env_vars: &BTreeMap<String, String>,
) -> Result<McpClient> {
    let parts = shell_words::split(command_str)
        .map_err(|e| Error::runtime(format!("invalid --mcp-stdio command: {e}")))?;
    if parts.is_empty() {
        return Err(Error::usage("--mcp-stdio command is empty"));
    }

    let transport = TokioChildProcess::new(Command::new(&parts[0]).configure(|c| {
        c.args(&parts[1..]).kill_on_drop(true);
        for (k, v) in env_vars {
            c.env(k, v);
        }
    }))
    .map_err(|e| Error::runtime(format!("failed to start MCP stdio process: {e}")))?;

    tokio::time::timeout(Duration::from_secs(30), ().serve(transport))
        .await
        .map_err(|_| Error::runtime("MCP initialize timed out after 30s"))?
        .map_err(|e| Error::runtime(format!("MCP initialize failed: {e}")))
}

pub fn tools_to_commands(tools: &[Value]) -> Vec<CommandDef> {
    extract_mcp_commands(tools)
}
