//! Session daemon: hold one MCP client, serve NDJSON over AF_UNIX.
//!
//! MCP RPCs are serialized with a mutex. Idle exit and SIGTERM remove session
//! files.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

use crate::error::{Error, Result};
use crate::mcp::{
    call_tool_on, connect_http, connect_stdio_with, get_prompt_on, list_prompts_on,
    list_resource_templates_on, list_resources_on, list_tools_on, read_resource_on, McpClient,
    TransportMode,
};
use crate::oauth::{authorize, OAuthReady};
use crate::session::paths::{
    chmod_0600, session_meta_path, session_sock_path, unlink_session_files, write_meta, SessionMeta,
};
use crate::session::peer::peer_uid_matches_self;
use crate::session::protocol::{SessionMethod, SessionRequest, SessionResponse};
use crate::session::spawn::DaemonConfig;
use crate::tools_index::{build_compact_index, search_compact, CompactIndex};

const IPC_MAX_REQUEST_BYTES: usize = 1024 * 1024;
const IPC_IO_TIMEOUT: Duration = Duration::from_secs(30);

struct DaemonState {
    name: String,
    config: DaemonConfig,
    client: Option<McpClient>,
    /// OAuth client used to refresh HTTP session credentials.
    oauth: Option<OAuthReady>,
    /// Token currently configured on the HTTP connection.
    bearer_token: Option<String>,
    tools_cache: Option<Vec<Value>>,
    /// In-memory search index rebuilt with `tools_cache`.
    tools_index: Option<CompactIndex>,
    last_activity: Instant,
}

fn cache_tools(st: &mut DaemonState, tools: Vec<Value>) {
    st.tools_index = Some(build_compact_index(&tools, false));
    st.tools_cache = Some(tools);
}

/// Entry point for `__session_daemon <config-path>`.
pub fn run_session_daemon(config_path: PathBuf) -> Result<()> {
    let raw = fs::read_to_string(&config_path)
        .map_err(|e| Error::runtime(format!("cannot read session config: {e}")))?;
    let _ = fs::remove_file(&config_path);
    let config: DaemonConfig = serde_json::from_str(&raw)
        .map_err(|e| Error::runtime(format!("invalid session config: {e}")))?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::runtime(e.to_string()))?;
    rt.block_on(daemon_main(config))
}

async fn daemon_main(config: DaemonConfig) -> Result<()> {
    let name = config.name.clone();
    let sock_path = session_sock_path(&name);
    let _ = fs::remove_file(&sock_path);

    let idle_secs = config.idle_secs;
    let (client, oauth, bearer_token) = connect_mcp_fresh(&config).await?;
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    write_meta(
        &name,
        &SessionMeta {
            pid: std::process::id(),
            source: config.source.clone(),
            transport: if config.is_stdio {
                "stdio".into()
            } else {
                "http".into()
            },
            created_at,
            idle_secs,
            last_activity_at: created_at,
        },
    )?;

    // Bind with owner-only permissions; chmod below preserves that invariant.
    #[cfg(unix)]
    let prev_umask = unsafe { libc::umask(0o177) };
    let bind_result = UnixListener::bind(&sock_path);
    #[cfg(unix)]
    unsafe {
        libc::umask(prev_umask);
    }
    let listener =
        bind_result.map_err(|e| Error::runtime(format!("cannot bind session socket: {e}")))?;
    chmod_0600(&sock_path)?;
    if let Some(parent) = sock_path.parent() {
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }

    let state = Arc::new(Mutex::new(DaemonState {
        name: name.clone(),
        config,
        client: Some(client),
        oauth,
        bearer_token,
        tools_cache: None,
        tools_index: None,
        last_activity: Instant::now(),
    }));

    let idle = Duration::from_secs(idle_secs);
    let idle_enabled = idle_secs > 0;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                break;
            }
            _ = wait_sigterm() => {
                break;
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        let std_stream = match stream.into_std() {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::debug!("session into_std: {e}");
                                continue;
                            }
                        };
                        if !peer_uid_matches_self(&std_stream) {
                            tracing::warn!("rejected session connection: peer UID mismatch");
                            continue;
                        }
                        let _ = std_stream.set_nonblocking(true);
                        let stream = match tokio::net::UnixStream::from_std(std_stream) {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::debug!("session from_std: {e}");
                                continue;
                            }
                        };
                        let state = Arc::clone(&state);
                        tokio::spawn(async move {
                            if let Err(e) = handle_client(stream, state).await {
                                tracing::debug!("session client error: {e}");
                            }
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        tracing::debug!("session accept error: {e}");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if idle_enabled {
                    let last = state.lock().await.last_activity;
                    if last.elapsed() >= idle {
                        tracing::info!("session idle timeout; shutting down");
                        break;
                    }
                }
            }
        }
    }

    {
        let mut st = state.lock().await;
        if let Some(client) = st.client.take() {
            let _ = client.cancel().await;
        }
    }
    unlink_session_files(&name);
    let _ = fs::remove_file(&sock_path);
    let _ = fs::remove_file(session_meta_path(&name));
    Ok(())
}

async fn wait_sigterm() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            sigterm.recv().await;
            return;
        }
    }
    std::future::pending::<()>().await
}

async fn connect_mcp_fresh(
    config: &DaemonConfig,
) -> Result<(McpClient, Option<OAuthReady>, Option<String>)> {
    if config.is_stdio {
        let client = connect_stdio_with(&config.source, &config.env_vars, config.clean_env).await?;
        return Ok((client, None, None));
    }

    let transport = TransportMode::parse(&config.transport)?;
    let mut headers = config.auth_headers.clone();
    let mut oauth_ready = None;
    let mut bearer = None;

    if let Some(oauth_cfg) = &config.oauth {
        let opts = oauth_cfg.to_options()?;
        let ready = authorize(&config.source, &opts).await.map_err(|e| {
            Error::runtime(format!(
                "OAuth for session failed: {e}. Fix credentials then --session-stop and --session-start again"
            ))
        })?;
        let token = ready.access_token().await.map_err(|e| {
            Error::runtime(format!(
                "OAuth token unavailable for session: {e}. Re-run with --oauth or check cached credentials under $SKIFF_CACHE_DIR/oauth/"
            ))
        })?;
        headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
        headers.push(("Authorization".into(), format!("Bearer {token}")));
        bearer = Some(token);
        oauth_ready = Some(ready);
    }

    let client = connect_http(&config.source, &headers, transport, None)
        .await
        .map_err(map_connect_err)?;
    Ok((client, oauth_ready, bearer))
}

fn map_connect_err(e: Error) -> Error {
    let msg = e.to_string();
    if looks_like_auth_error(&msg) {
        Error::runtime(format!(
            "{msg}. Auth likely expired — for OAuth sessions the daemon refreshes on the next RPC; otherwise --session-stop then --session-start with fresh credentials"
        ))
    } else {
        e
    }
}

fn looks_like_auth_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("invalid_token")
        || lower.contains("expired")
}

/// Refresh OAuth token from store and reconnect if the Bearer changed.
async fn ensure_fresh_auth(st: &mut DaemonState) -> Result<()> {
    if st.config.is_stdio || st.config.oauth.is_none() {
        return Ok(());
    }

    if st.oauth.is_none() {
        let (client, oauth, bearer) = connect_mcp_fresh(&st.config).await?;
        if let Some(old) = st.client.take() {
            let _ = old.cancel().await;
        }
        st.client = Some(client);
        st.oauth = oauth;
        st.bearer_token = bearer;
        st.tools_cache = None;
        st.tools_index = None;
        return Ok(());
    }

    let token = {
        let oauth = st.oauth.as_ref().unwrap();
        match oauth.access_token().await {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!("OAuth access_token failed ({e}); re-authorizing");
                let (client, oauth, bearer) = connect_mcp_fresh(&st.config).await?;
                if let Some(old) = st.client.take() {
                    let _ = old.cancel().await;
                }
                st.client = Some(client);
                st.oauth = oauth;
                st.bearer_token = bearer;
                st.tools_cache = None;
                st.tools_index = None;
                return Ok(());
            }
        }
    };

    if st.bearer_token.as_deref() == Some(token.as_str()) {
        return Ok(());
    }

    tracing::info!("session OAuth token rotated; reconnecting MCP client");
    let transport = TransportMode::parse(&st.config.transport)?;
    let mut headers = st.config.auth_headers.clone();
    headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
    headers.push(("Authorization".into(), format!("Bearer {token}")));

    let client = connect_http(&st.config.source, &headers, transport, None)
        .await
        .map_err(|e| {
            Error::runtime(format!(
                "MCP reconnect after OAuth refresh failed: {e}. Run --session-stop {} then --session-start again",
                st.name
            ))
        })?;
    if let Some(old) = st.client.take() {
        let _ = old.cancel().await;
    }
    st.client = Some(client);
    st.bearer_token = Some(token);
    Ok(())
}

fn map_rpc_err(name: &str, e: Error) -> Error {
    let msg = e.to_string();
    if looks_like_auth_error(&msg) {
        return Error::runtime(format!(
            "{msg}. Auth failed — OAuth sessions refresh automatically; if this persists, --session-stop {name} then --session-start with fresh credentials"
        ));
    }
    if msg.contains("closed")
        || msg.contains("broken pipe")
        || msg.contains("Connection reset")
        || msg.contains("os error 32")
        || msg.contains("transport")
    {
        return Error::runtime(format!(
            "{msg}. MCP child or HTTP session likely died — run --session-stop {name} then --session-start {name} again"
        ));
    }
    e
}

async fn handle_client(
    stream: tokio::net::UnixStream,
    state: Arc<Mutex<DaemonState>>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let reader = BufReader::new(reader);
    let mut line = Vec::new();
    let len = tokio::time::timeout(
        IPC_IO_TIMEOUT,
        reader
            .take((IPC_MAX_REQUEST_BYTES + 1) as u64)
            .read_until(b'\n', &mut line),
    )
    .await
    .map_err(|_| Error::runtime("session request timed out"))?
    .map_err(|e| Error::runtime(e.to_string()))?;
    if len == 0 {
        return Ok(());
    }
    if line.len() > IPC_MAX_REQUEST_BYTES {
        return Err(Error::runtime("session request exceeds 1 MiB"));
    }
    let req: SessionRequest = serde_json::from_slice(&line)
        .map_err(|e| Error::runtime(format!("bad session request: {e}")))?;
    let resp = match dispatch(&state, &req).await {
        Ok(v) => SessionResponse::ok(req.id, v),
        Err(e) => SessionResponse::err(req.id, e.to_string()),
    };
    let mut out = serde_json::to_string(&resp)?;
    out.push('\n');
    tokio::time::timeout(IPC_IO_TIMEOUT, writer.write_all(out.as_bytes()))
        .await
        .map_err(|_| Error::runtime("session response timed out"))?
        .map_err(|e| Error::runtime(e.to_string()))?;
    let _ = writer.shutdown().await;
    Ok(())
}

async fn dispatch(state: &Arc<Mutex<DaemonState>>, req: &SessionRequest) -> Result<Value> {
    let method = SessionMethod::parse(&req.method)
        .ok_or_else(|| Error::runtime(format!("Unknown method: {}", req.method)))?;

    let mut st = state.lock().await;
    st.last_activity = Instant::now();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    if let Ok(Some(mut meta)) = crate::session::paths::load_meta(&st.name) {
        meta.last_activity_at = now;
        let _ = write_meta(&st.name, &meta);
    }

    ensure_fresh_auth(&mut st).await?;
    let session_name = st.name.clone();

    let result = dispatch_locked(&mut st, method, req).await;
    match result {
        Ok(v) => Ok(v),
        Err(e) if looks_like_auth_error(&e.to_string()) && st.config.oauth.is_some() => {
            tracing::debug!("RPC auth error; forcing OAuth reconnect: {e}");
            st.bearer_token = None;
            st.oauth = None;
            ensure_fresh_auth(&mut st).await?;
            dispatch_locked(&mut st, method, req)
                .await
                .map_err(|err| map_rpc_err(&session_name, err))
        }
        Err(e) => Err(map_rpc_err(&session_name, e)),
    }
}

async fn dispatch_locked(
    st: &mut DaemonState,
    method: SessionMethod,
    req: &SessionRequest,
) -> Result<Value> {
    let name = st.name.clone();
    let client = st.client.as_ref().ok_or_else(|| {
        Error::runtime(format!(
            "session MCP client is shut down. Run --session-stop {name} then --session-start {name}"
        ))
    })?;

    match method {
        SessionMethod::ListTools => {
            let refresh = req
                .params
                .get("refresh")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !refresh {
                if let Some(cached) = &st.tools_cache {
                    return Ok(Value::Array(cached.clone()));
                }
            }
            let tools = list_tools_on(client).await?;
            cache_tools(st, tools.clone());
            Ok(Value::Array(tools))
        }
        SessionMethod::ListToolsLight => {
            let refresh = req
                .params
                .get("refresh")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let search = req
                .params
                .get("search")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            if refresh || st.tools_cache.is_none() || st.tools_index.is_none() {
                let tools = list_tools_on(client).await?;
                cache_tools(st, tools);
            }
            let index = st
                .tools_index
                .as_ref()
                .ok_or_else(|| Error::runtime("session tools index missing"))?;
            let entries = if let Some(pat) = search {
                search_compact(index, pat)
            } else {
                index.to_entries()
            };
            Ok(index.to_light_tools_json(&entries))
        }
        SessionMethod::GetTool => {
            let tool_name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::usage("get_tool requires params.name"))?;
            let refresh = req
                .params
                .get("refresh")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if refresh || st.tools_cache.is_none() {
                let tools = list_tools_on(client).await?;
                cache_tools(st, tools);
            }
            let tools = st.tools_cache.as_ref().unwrap();
            let found = tools.iter().find(|t| {
                let mcp = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
                mcp == tool_name || crate::coerce::to_kebab(mcp) == tool_name
            });
            Ok(found.cloned().unwrap_or(Value::Null))
        }
        SessionMethod::CallTool => {
            let tool_name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::usage("call_tool requires params.name"))?;
            let arguments = req
                .params
                .get("arguments")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let full = req
                .params
                .get("full_envelope")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            call_tool_on(client, tool_name, arguments, full).await
        }
        SessionMethod::ListResources => list_resources_on(client).await,
        SessionMethod::ReadResource => {
            let uri = req
                .params
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::usage("read_resource requires params.uri"))?;
            read_resource_on(client, uri).await
        }
        SessionMethod::ListResourceTemplates => list_resource_templates_on(client).await,
        SessionMethod::ListPrompts => list_prompts_on(client).await,
        SessionMethod::GetPrompt => {
            let prompt_name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::usage("get_prompt requires params.name"))?;
            let args = req
                .params
                .get("arguments")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            get_prompt_on(client, prompt_name, args).await
        }
    }
}
