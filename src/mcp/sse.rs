//! Legacy MCP SSE client (pre-streamable-HTTP transport).
//!
//! rmcp 3.x no longer ships a dedicated SSE client; we bridge the classic
//! GET-/sse + POST-/messages/ protocol into a Sink/Stream transport.

use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::{Sink, Stream};
use http::{HeaderMap, HeaderName, HeaderValue};
use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::ServiceExt;
use serde_json::{Map, Value};
use sse_stream::SseStream;
use tokio::sync::mpsc;
use url::Url;

use crate::cache::{load_cached, save_cache};
use crate::error::{Error, Result};
use crate::mcp::common::{auth_headers_to_http, call_tool_on, list_tools_on, McpClient};
use crate::oauth::OAuthReady;

type ClientMsg = ClientJsonRpcMessage;
type ServerMsg = ServerJsonRpcMessage;

struct MpscSink {
    tx: mpsc::Sender<ClientMsg>,
}

impl Sink<ClientMsg> for MpscSink {
    type Error = std::io::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        if self.tx.is_closed() {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "sse write channel closed",
            )))
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(self: Pin<&mut Self>, item: ClientMsg) -> std::result::Result<(), Self::Error> {
        self.tx.try_send(item).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, format!("sse send: {e}"))
        })
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

struct MpscStream {
    rx: mpsc::Receiver<ServerMsg>,
}

impl Stream for MpscStream {
    type Item = ServerMsg;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

fn header_map_from(headers: &HashMap<HeaderName, HeaderValue>) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        map.insert(k.clone(), v.clone());
    }
    map
}

pub async fn fetch_mcp_tools_sse(
    url: &str,
    auth_headers: &[(String, String)],
    cache_key: &str,
    ttl: u64,
    refresh: bool,
    oauth: Option<&OAuthReady>,
) -> Result<Vec<Value>> {
    let tools_key = format!("{cache_key}_tools");
    if !refresh {
        if let Some(cached) = load_cached(&tools_key, ttl)? {
            if let Some(arr) = cached.as_array() {
                return Ok(arr.clone());
            }
        }
    }
    let tools = list_tools_sse(url, auth_headers, oauth).await?;
    save_cache(&tools_key, &Value::Array(tools.clone()))?;
    Ok(tools)
}

pub async fn list_tools_sse(
    url: &str,
    auth_headers: &[(String, String)],
    oauth: Option<&OAuthReady>,
) -> Result<Vec<Value>> {
    let client = connect_sse(url, auth_headers, oauth).await?;
    let tools = list_tools_on(&client).await?;
    let _ = client.cancel().await;
    Ok(tools)
}

pub async fn call_tool_sse(
    url: &str,
    auth_headers: &[(String, String)],
    tool_name: &str,
    arguments: Map<String, Value>,
    full_envelope: bool,
    oauth: Option<&OAuthReady>,
) -> Result<Value> {
    let client = connect_sse(url, auth_headers, oauth).await?;
    let result = call_tool_on(&client, tool_name, arguments, full_envelope).await?;
    let _ = client.cancel().await;
    Ok(result)
}

pub async fn connect_sse(
    url: &str,
    auth_headers: &[(String, String)],
    oauth: Option<&OAuthReady>,
) -> Result<McpClient> {
    let mut headers = auth_headers.to_vec();
    if let Some(oauth) = oauth {
        let token = oauth.access_token().await?;
        // Prefer OAuth bearer; keep other custom headers.
        headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
        headers.push(("Authorization".into(), format!("Bearer {token}")));
    }
    let custom = auth_headers_to_http(&headers)?;
    let header_map = header_map_from(&custom);

    let (write_tx, write_rx) = mpsc::channel::<ClientMsg>(64);
    let (read_tx, read_rx) = mpsc::channel::<ServerMsg>(64);

    let sse_url = url.to_string();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| Error::runtime(format!("http client: {e}")))?;

    // Bridge task: open SSE, wait for endpoint event, then POST outbound messages.
    tokio::spawn(async move {
        if let Err(e) = run_sse_bridge(client, sse_url, header_map, write_rx, read_tx).await {
            tracing::debug!("SSE bridge ended: {e}");
        }
    });

    let transport = (MpscSink { tx: write_tx }, MpscStream { rx: read_rx });
    tokio::time::timeout(Duration::from_secs(30), ().serve(transport))
        .await
        .map_err(|_| Error::runtime("MCP SSE initialize timed out after 30s"))?
        .map_err(|e| Error::runtime(format!("MCP SSE initialize failed: {e}")))
}

async fn run_sse_bridge(
    client: reqwest::Client,
    sse_url: String,
    headers: HeaderMap,
    mut write_rx: mpsc::Receiver<ClientMsg>,
    read_tx: mpsc::Sender<ServerMsg>,
) -> Result<()> {
    let mut req = client
        .get(&sse_url)
        .header(http::header::ACCEPT, "text/event-stream");
    for (k, v) in &headers {
        req = req.header(k, v);
    }
    let response = req
        .send()
        .await
        .map_err(|e| Error::runtime(format!("SSE connect failed: {e}")))?
        .error_for_status()
        .map_err(|e| Error::runtime(format!("SSE connect status: {e}")))?;

    let mut stream = SseStream::from_bytes_stream(response.bytes_stream());
    let mut endpoint: Option<String> = None;

    // Wait for the endpoint event before accepting POSTs.
    while let Some(event) = futures::StreamExt::next(&mut stream).await {
        let event = event.map_err(|e| Error::runtime(format!("SSE read: {e}")))?;
        let ev = event.event.as_deref().unwrap_or("message");
        if ev == "endpoint" {
            let data = event.data.unwrap_or_default();
            let joined = Url::parse(&sse_url)
                .ok()
                .and_then(|base| base.join(&data).ok())
                .map(|u| u.to_string())
                .unwrap_or(data);
            // Origin check
            if let (Ok(base), Ok(ep)) = (Url::parse(&sse_url), Url::parse(&joined)) {
                if base.scheme() != ep.scheme() || base.host_str() != ep.host_str() {
                    return Err(Error::runtime(format!(
                        "SSE endpoint origin mismatch: {joined}"
                    )));
                }
            }
            endpoint = Some(joined);
            break;
        } else if ev == "message" {
            if let Some(data) = event.data {
                if let Ok(msg) = serde_json::from_str::<ServerMsg>(&data) {
                    let _ = read_tx.send(msg).await;
                }
            }
        }
    }

    let endpoint =
        endpoint.ok_or_else(|| Error::runtime("SSE server never sent endpoint event"))?;

    loop {
        tokio::select! {
            msg = write_rx.recv() => {
                let Some(msg) = msg else { break; };
                let mut req = client.post(&endpoint).json(&msg);
                for (k, v) in &headers {
                    req = req.header(k, v);
                }
                let resp = req.send().await.map_err(|e| Error::runtime(format!("SSE POST: {e}")))?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(Error::runtime(format!("SSE POST {status}: {body}")));
                }
            }
            event = futures::StreamExt::next(&mut stream) => {
                let Some(event) = event else { break; };
                let event = event.map_err(|e| Error::runtime(format!("SSE read: {e}")))?;
                let ev = event.event.as_deref().unwrap_or("message");
                if ev == "message" {
                    if let Some(data) = event.data {
                        if let Ok(msg) = serde_json::from_str::<ServerMsg>(&data) {
                            if read_tx.send(msg).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
