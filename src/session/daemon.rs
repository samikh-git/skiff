//! Session daemon: hold one MCP client, serve NDJSON over AF_UNIX.
//!
//! Accepts concurrent IPC clients; MCP RPCs are serialized with a mutex.
//! Tool schemas are cached in-memory (`list_tools` + `refresh: true` busts).
//! Idle / SIGTERM exits unlink meta + sock.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

use crate::error::{Error, Result};
use crate::mcp::{
    call_tool_on, connect_http, connect_stdio_with, list_tools_on, McpClient, TransportMode,
};
use crate::session::paths::{
    chmod_0600, session_meta_path, session_sock_path, unlink_session_files, write_meta, SessionMeta,
};
use crate::session::peer::peer_uid_matches_self;
use crate::session::protocol::{SessionMethod, SessionRequest, SessionResponse};
use crate::session::spawn::DaemonConfig;

struct DaemonState {
    name: String,
    client: Option<McpClient>,
    tools_cache: Option<Vec<Value>>,
    last_activity: Instant,
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

    let client = connect_mcp(&config).await?;
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
            idle_secs: config.idle_secs,
            last_activity_at: created_at,
        },
    )?;

    let listener = UnixListener::bind(&sock_path)
        .map_err(|e| Error::runtime(format!("cannot bind session socket: {e}")))?;
    chmod_0600(&sock_path)?;
    if let Some(parent) = sock_path.parent() {
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }

    let state = Arc::new(Mutex::new(DaemonState {
        name: name.clone(),
        client: Some(client),
        tools_cache: None,
        last_activity: Instant::now(),
    }));

    let idle = Duration::from_secs(config.idle_secs);
    let idle_enabled = config.idle_secs > 0;

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

async fn connect_mcp(config: &DaemonConfig) -> Result<McpClient> {
    if config.is_stdio {
        connect_stdio_with(&config.source, &config.env_vars, config.clean_env).await
    } else {
        let transport = TransportMode::parse(&config.transport)?;
        connect_http(&config.source, &config.auth_headers, transport, None).await
    }
}

async fn handle_client(
    stream: tokio::net::UnixStream,
    state: Arc<Mutex<DaemonState>>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| Error::runtime(e.to_string()))?
    else {
        return Ok(());
    };
    let req: SessionRequest = serde_json::from_str(&line)
        .map_err(|e| Error::runtime(format!("bad session request: {e}")))?;
    let resp = match dispatch(&state, &req).await {
        Ok(v) => SessionResponse::ok(req.id, v),
        Err(e) => SessionResponse::err(req.id, e.to_string()),
    };
    let mut out = serde_json::to_string(&resp)?;
    out.push('\n');
    writer
        .write_all(out.as_bytes())
        .await
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
    let client = st
        .client
        .as_ref()
        .ok_or_else(|| Error::runtime("session MCP client is shut down"))?;

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
            st.tools_cache = Some(tools.clone());
            Ok(Value::Array(tools))
        }
        SessionMethod::CallTool => {
            let name = req
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
            call_tool_on(client, name, arguments, full).await
        }
        SessionMethod::ListResources => {
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
        SessionMethod::ReadResource => {
            let uri = req
                .params
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::usage("read_resource requires params.uri"))?;
            let result = client
                .read_resource(rmcp::model::ReadResourceRequestParams::new(uri))
                .await
                .map_err(|e| Error::runtime(format!("read_resource: {e}")))?;
            Ok(serde_json::to_value(result).unwrap_or(Value::Null))
        }
        SessionMethod::ListResourceTemplates => {
            let templates = client
                .list_all_resource_templates()
                .await
                .map_err(|e| Error::runtime(format!("list_resource_templates: {e}")))?;
            Ok(serde_json::to_value(templates).unwrap_or(Value::Null))
        }
        SessionMethod::ListPrompts => {
            let prompts = client
                .list_all_prompts()
                .await
                .map_err(|e| Error::runtime(format!("list_prompts: {e}")))?;
            Ok(serde_json::to_value(prompts).unwrap_or(Value::Null))
        }
        SessionMethod::GetPrompt => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::usage("get_prompt requires params.name"))?;
            let mut params = rmcp::model::GetPromptRequestParams::new(name);
            if let Some(args) = req.params.get("arguments").and_then(|v| v.as_object()) {
                params = params.with_arguments(args.clone());
            }
            let result = client
                .get_prompt(params)
                .await
                .map_err(|e| Error::runtime(format!("get_prompt: {e}")))?;
            Ok(serde_json::to_value(result).unwrap_or(Value::Null))
        }
    }
}
