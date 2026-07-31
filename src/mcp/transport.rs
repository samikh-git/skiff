//! MCP HTTP transport selection: streamable, legacy SSE, or auto.

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::mcp::common::McpClient;
use crate::mcp::http::{call_tool_http, connect_streamable, fetch_mcp_tools_http};
use crate::mcp::sse::{call_tool_sse, connect_sse, fetch_mcp_tools_sse};
use crate::oauth::OAuthReady;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    Auto,
    Sse,
    Streamable,
}

impl TransportMode {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "auto" => Ok(Self::Auto),
            "sse" => Ok(Self::Sse),
            "streamable" => Ok(Self::Streamable),
            other => Err(Error::usage(format!(
                "invalid --transport {other:?}; expected auto|sse|streamable"
            ))),
        }
    }
}

pub async fn fetch_mcp_tools(
    url: &str,
    auth_headers: &[(String, String)],
    cache_key: &str,
    ttl: u64,
    refresh: bool,
    transport: TransportMode,
    oauth: Option<&OAuthReady>,
) -> Result<Vec<Value>> {
    match transport {
        TransportMode::Streamable => {
            fetch_mcp_tools_http(url, auth_headers, cache_key, ttl, refresh, oauth).await
        }
        TransportMode::Sse => {
            fetch_mcp_tools_sse(url, auth_headers, cache_key, ttl, refresh, oauth).await
        }
        TransportMode::Auto => {
            match fetch_mcp_tools_http(url, auth_headers, cache_key, ttl, refresh, oauth).await {
                Ok(tools) => Ok(tools),
                Err(streamable_err) => {
                    tracing::debug!("streamable HTTP failed, trying SSE: {streamable_err}");
                    fetch_mcp_tools_sse(url, auth_headers, cache_key, ttl, refresh, oauth)
                        .await
                        .map_err(|sse_err| {
                            Error::runtime(format!(
                                "MCP HTTP failed (streamable: {streamable_err}; sse: {sse_err})"
                            ))
                        })
                }
            }
        }
    }
}

pub async fn call_tool(
    url: &str,
    auth_headers: &[(String, String)],
    tool_name: &str,
    arguments: Map<String, Value>,
    full_envelope: bool,
    transport: TransportMode,
    oauth: Option<&OAuthReady>,
) -> Result<Value> {
    match transport {
        TransportMode::Streamable => {
            call_tool_http(url, auth_headers, tool_name, arguments, full_envelope, oauth).await
        }
        TransportMode::Sse => {
            call_tool_sse(url, auth_headers, tool_name, arguments, full_envelope, oauth).await
        }
        TransportMode::Auto => {
            match call_tool_http(
                url,
                auth_headers,
                tool_name,
                arguments.clone(),
                full_envelope,
                oauth,
            )
            .await
            {
                Ok(v) => Ok(v),
                Err(streamable_err) => {
                    tracing::debug!("streamable call failed, trying SSE: {streamable_err}");
                    call_tool_sse(url, auth_headers, tool_name, arguments, full_envelope, oauth)
                        .await
                        .map_err(|sse_err| {
                            Error::runtime(format!(
                                "MCP call failed (streamable: {streamable_err}; sse: {sse_err})"
                            ))
                        })
                }
            }
        }
    }
}

/// Open a long-lived HTTP MCP client (for session daemons).
pub async fn connect_http(
    url: &str,
    auth_headers: &[(String, String)],
    transport: TransportMode,
    oauth: Option<&OAuthReady>,
) -> Result<McpClient> {
    match transport {
        TransportMode::Streamable => connect_streamable(url, auth_headers, oauth).await,
        TransportMode::Sse => connect_sse(url, auth_headers, oauth).await,
        TransportMode::Auto => match connect_streamable(url, auth_headers, oauth).await {
            Ok(c) => Ok(c),
            Err(streamable_err) => {
                tracing::debug!("streamable connect failed, trying SSE: {streamable_err}");
                connect_sse(url, auth_headers, oauth)
                    .await
                    .map_err(|sse_err| {
                        Error::runtime(format!(
                            "MCP connect failed (streamable: {streamable_err}; sse: {sse_err})"
                        ))
                    })
            }
        },
    }
}
