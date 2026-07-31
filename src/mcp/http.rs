//! MCP streamable HTTP client via rmcp.

use std::sync::Arc;
use std::time::Duration;

use rmcp::{
    transport::streamable_http_client::StreamableHttpClientTransportConfig,
    transport::StreamableHttpClientTransport, ServiceExt,
};
use serde_json::{Map, Value};

use crate::cache::{load_cached, save_cache};
use crate::error::{Error, Result};
use crate::mcp::common::{
    auth_headers_to_http, call_tool_on, list_tools_on, McpClient,
};

pub async fn fetch_mcp_tools_http(
    url: &str,
    auth_headers: &[(String, String)],
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

    let tools = list_tools_http(url, auth_headers).await?;
    save_cache(&tools_key, &Value::Array(tools.clone()))?;
    Ok(tools)
}

pub async fn list_tools_http(
    url: &str,
    auth_headers: &[(String, String)],
) -> Result<Vec<Value>> {
    let client = connect_streamable(url, auth_headers).await?;
    let tools = list_tools_on(&client).await?;
    let _ = client.cancel().await;
    Ok(tools)
}

pub async fn call_tool_http(
    url: &str,
    auth_headers: &[(String, String)],
    tool_name: &str,
    arguments: Map<String, Value>,
    full_envelope: bool,
) -> Result<Value> {
    let client = connect_streamable(url, auth_headers).await?;
    let result = call_tool_on(&client, tool_name, arguments, full_envelope).await?;
    let _ = client.cancel().await;
    Ok(result)
}

pub async fn connect_streamable(
    url: &str,
    auth_headers: &[(String, String)],
) -> Result<McpClient> {
    let custom_headers = auth_headers_to_http(auth_headers)?;
    let config =
        StreamableHttpClientTransportConfig::with_uri(Arc::<str>::from(url)).custom_headers(custom_headers);

    let transport = StreamableHttpClientTransport::from_config(config);
    tokio::time::timeout(Duration::from_secs(30), ().serve(transport))
        .await
        .map_err(|_| Error::runtime("MCP HTTP initialize timed out after 30s"))?
        .map_err(|e| Error::runtime(format!("MCP HTTP initialize failed: {e}")))
}
