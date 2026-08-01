//! Session directory layout under `~/.cache/skiff/sessions/`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::paths::cache_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub pid: u32,
    pub source: String,
    pub transport: String,
    pub created_at: f64,
    #[serde(default)]
    pub idle_secs: u64,
    /// Unix timestamp of last IPC activity (for idle remaining).
    #[serde(default)]
    pub last_activity_at: f64,
}

pub fn sessions_dir() -> PathBuf {
    cache_dir().join("sessions")
}

pub fn session_meta_path(name: &str) -> PathBuf {
    sessions_dir().join(format!("{name}.json"))
}

pub fn session_sock_path(name: &str) -> PathBuf {
    sessions_dir().join(format!("{name}.sock"))
}

pub fn session_log_path(name: &str) -> PathBuf {
    sessions_dir().join(format!("{name}.log"))
}

pub fn session_config_path(name: &str) -> PathBuf {
    sessions_dir().join(format!("{name}.config.json"))
}

pub fn ensure_sessions_dir() -> Result<()> {
    crate::fsutil::ensure_dir_0700(&sessions_dir())
}

pub fn is_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // signal 0: existence check
    let rc = unsafe { libc::kill(pid as i32, 0) };
    rc == 0
}

pub fn session_is_alive(meta: &SessionMeta) -> bool {
    is_process_alive(meta.pid)
}

pub fn load_meta(name: &str) -> Result<Option<SessionMeta>> {
    let path = session_meta_path(name);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    match serde_json::from_str(&text) {
        Ok(m) => Ok(Some(m)),
        Err(_) => Ok(None),
    }
}

pub fn write_meta(name: &str, meta: &SessionMeta) -> Result<()> {
    ensure_sessions_dir()?;
    let path = session_meta_path(name);
    let json = serde_json::to_vec_pretty(meta)?;
    crate::fsutil::atomic_write_0600(&path, &json)?;
    Ok(())
}

/// Remove meta/sock/log (and leftover config) for a session name.
pub fn unlink_session_files(name: &str) {
    for p in [
        session_meta_path(name),
        session_sock_path(name),
        session_log_path(name),
        session_config_path(name),
    ] {
        let _ = fs::remove_file(p);
    }
}

/// If meta exists but PID is dead, clear leftovers. Returns true if cleaned.
pub fn clear_stale_session(name: &str) -> Result<bool> {
    if let Some(meta) = load_meta(name)? {
        if !session_is_alive(&meta) {
            unlink_session_files(name);
            return Ok(true);
        }
        return Ok(false);
    }
    // Orphan sock without meta
    let sock = session_sock_path(name);
    if sock.exists() {
        unlink_session_files(name);
        return Ok(true);
    }
    Ok(false)
}

pub fn validate_session_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return Err(Error::usage(format!(
            "invalid session name {name:?}; use a simple identifier"
        )));
    }
    Ok(())
}

pub fn chmod_0600(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{set_cache_dir_override, TEST_PATHS_LOCK};
    use tempfile::tempdir;

    #[test]
    fn paths_and_stale() {
        let _g = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempdir().unwrap();
        set_cache_dir_override(Some(dir.path().to_path_buf()));
        ensure_sessions_dir().unwrap();
        assert!(sessions_dir().ends_with("sessions"));

        write_meta(
            "demo",
            &SessionMeta {
                pid: 999_999_999,
                source: "echo".into(),
                transport: "stdio".into(),
                created_at: 1.0,
                idle_secs: 1800,
                last_activity_at: 1.0,
            },
        )
        .unwrap();
        assert!(clear_stale_session("demo").unwrap());
        assert!(!session_meta_path("demo").exists());
        set_cache_dir_override(None);
    }

    #[test]
    fn name_validation() {
        assert!(validate_session_name("myfs").is_ok());
        assert!(validate_session_name("../x").is_err());
        assert!(validate_session_name("a/b").is_err());
    }
}
