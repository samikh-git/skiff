//! Spawn and manage session daemon processes.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::session::paths::{
    clear_stale_session, ensure_sessions_dir, load_meta, session_config_path, session_is_alive,
    session_log_path, session_sock_path, sessions_dir, unlink_session_files, validate_session_name,
};
use crate::session::paths::chmod_0600;

pub const DEFAULT_IDLE_SECS: u64 = 1800;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub name: String,
    pub source: String,
    pub is_stdio: bool,
    pub auth_headers: Vec<(String, String)>,
    pub env_vars: BTreeMap<String, String>,
    pub transport: String,
    #[serde(default)]
    pub clean_env: bool,
    #[serde(default = "default_idle")]
    pub idle_secs: u64,
}

fn default_idle() -> u64 {
    DEFAULT_IDLE_SECS
}

/// Start a session daemon. Blocks until the socket appears (≤15s).
pub fn session_start(config: DaemonConfig) -> Result<()> {
    #[cfg(not(unix))]
    {
        return Err(crate::session::sessions_unsupported());
    }
    #[cfg(unix)]
    {
        session_start_unix(config)
    }
}

#[cfg(unix)]
fn session_start_unix(config: DaemonConfig) -> Result<()> {
    validate_session_name(&config.name)?;
    ensure_sessions_dir()?;
    let _ = clear_stale_session(&config.name)?;

    if let Some(meta) = load_meta(&config.name)? {
        if session_is_alive(&meta) {
            return Err(Error::runtime(format!(
                "session {:?} is already running (pid {})",
                config.name, meta.pid
            )));
        }
        unlink_session_files(&config.name);
    }

    let config_path = session_config_path(&config.name);
    let json = serde_json::to_vec_pretty(&config)?;
    fs::write(&config_path, &json)?;
    chmod_0600(&config_path)?;

    let log_path = session_log_path(&config.name);
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| Error::runtime(format!("cannot open session log: {e}")))?;
    chmod_0600(&log_path)?;

    let exe = std::env::current_exe()
        .map_err(|e| Error::runtime(format!("cannot resolve mcp2cli binary: {e}")))?;

    let mut cmd = Command::new(&exe);
    cmd.arg("__session_daemon")
        .arg(&config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file));
    // New process group so CLI Ctrl-C doesn't kill the daemon.
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::runtime(format!("failed to spawn session daemon: {e}")))?;

    let sock = session_sock_path(&config.name);
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if sock.exists() {
            println!(
                "Session '{}' started (pid {}). Use --session {} ...",
                config.name,
                child.id(),
                config.name
            );
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            unlink_session_files(&config.name);
            return Err(Error::runtime(format!(
                "session daemon exited early with {status}. See {}",
                log_path.display()
            )));
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    unlink_session_files(&config.name);
    Err(Error::runtime(format!(
        "session daemon did not become ready within 15s. See {}",
        log_path.display()
    )))
}

/// Stop a session daemon (SIGTERM, then SIGKILL).
pub fn session_stop(name: &str) -> Result<()> {
    validate_session_name(name)?;
    let Some(meta) = load_meta(name)? else {
        unlink_session_files(name);
        println!("Session '{name}' is not running.");
        return Ok(());
    };
    if session_is_alive(&meta) {
        let pid = meta.pid as i32;
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !crate::session::paths::is_process_alive(meta.pid) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        if crate::session::paths::is_process_alive(meta.pid) {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    unlink_session_files(name);
    println!("Session '{name}' stopped.");
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionListEntry {
    pub name: String,
    pub pid: u32,
    pub alive: bool,
    pub source: String,
    pub transport: String,
    pub idle_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_remaining_secs: Option<u64>,
}

/// List known sessions (from meta files).
pub fn session_list() -> Result<Vec<SessionListEntry>> {
    ensure_sessions_dir()?;
    let mut out = Vec::new();
    let dir = sessions_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // skip *.config.json
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.ends_with(".config.json"))
            .unwrap_or(false)
        {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let Ok(Some(meta)) = load_meta(&name) else {
            continue;
        };
        let alive = session_is_alive(&meta);
        let idle_remaining_secs = if meta.idle_secs == 0 || !alive {
            None
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let last = if meta.last_activity_at > 0.0 {
                meta.last_activity_at
            } else {
                meta.created_at
            };
            let elapsed = (now - last).max(0.0) as u64;
            Some(meta.idle_secs.saturating_sub(elapsed))
        };
        out.push(SessionListEntry {
            name,
            pid: meta.pid,
            alive,
            source: meta.source,
            transport: meta.transport,
            idle_secs: meta.idle_secs,
            idle_remaining_secs,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn resolve_idle_secs(cli: Option<u64>) -> u64 {
    if let Some(v) = cli {
        return v;
    }
    if let Ok(v) = std::env::var("MCP2CLI_SESSION_IDLE_SECS") {
        if let Ok(n) = v.parse::<u64>() {
            return n;
        }
    }
    DEFAULT_IDLE_SECS
}
