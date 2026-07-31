//! MCP stdio client via rmcp.

use std::collections::BTreeMap;
use std::time::Duration;

use rmcp::{
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::{Map, Value};
use tokio::process::Command;

use crate::cache::{load_cached, save_cache};
use crate::error::{Error, Result};
use crate::mcp::common::{call_tool_on, list_tools_on, McpClient};

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
    let tools = list_tools_on(&client).await?;
    let _ = client.cancel().await;
    Ok(tools)
}

pub async fn call_tool_stdio(
    command_str: &str,
    env_vars: &BTreeMap<String, String>,
    tool_name: &str,
    arguments: Map<String, Value>,
    full_envelope: bool,
) -> Result<Value> {
    let client = connect_stdio(command_str, env_vars).await?;
    let result = call_tool_on(&client, tool_name, arguments, full_envelope).await?;
    let _ = client.cancel().await;
    Ok(result)
}

pub async fn connect_stdio(
    command_str: &str,
    env_vars: &BTreeMap<String, String>,
) -> Result<McpClient> {
    connect_stdio_with(command_str, env_vars, false).await
}

/// Connect to an MCP stdio server. When `clean_env` is true, clear the child
/// environment and keep only PATH/HOME/LANG plus `env_vars`.
pub async fn connect_stdio_with(
    command_str: &str,
    env_vars: &BTreeMap<String, String>,
    clean_env: bool,
) -> Result<McpClient> {
    let parts = shell_words::split(command_str)
        .map_err(|e| Error::runtime(format!("invalid --mcp-stdio command: {e}")))?;
    if parts.is_empty() {
        return Err(Error::usage("--mcp-stdio command is empty"));
    }

    let transport = TokioChildProcess::new(Command::new(&parts[0]).configure(|c| {
        c.args(&parts[1..]).kill_on_drop(true);
        if clean_env {
            c.env_clear();
            for key in ["PATH", "HOME", "LANG", "LC_ALL", "TMPDIR", "TMP", "TEMP"] {
                if let Ok(v) = std::env::var(key) {
                    c.env(key, v);
                }
            }
        }
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