//! File-backed `CredentialStore` under `~/.cache/skiff/oauth/<hash>/`.

use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rmcp::transport::auth::{AuthError, CredentialStore, StoredCredentials};

use crate::error::Result;
use crate::fsutil::{atomic_write_0600, ensure_dir_0700};
use crate::oauth::config::oauth_dir_for_server;

const CREDENTIALS_FILE: &str = "credentials.json";
const REDIRECT_META_FILE: &str = "redirect_uri.txt";

#[derive(Debug, Clone)]
pub struct FileCredentialStore {
    dir: PathBuf,
}

impl FileCredentialStore {
    pub fn for_server(server_url: &str) -> Self {
        Self {
            dir: oauth_dir_for_server(server_url),
        }
    }

    pub fn from_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn credentials_path(&self) -> PathBuf {
        self.dir.join(CREDENTIALS_FILE)
    }

    fn redirect_meta_path(&self) -> PathBuf {
        self.dir.join(REDIRECT_META_FILE)
    }

    pub fn load_sticky_redirect_uri(&self) -> Option<String> {
        fs::read_to_string(self.redirect_meta_path())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn save_sticky_redirect_uri(&self, uri: &str) -> Result<()> {
        ensure_dir_0700(&self.dir)?;
        atomic_write_0600(&self.redirect_meta_path(), uri.as_bytes())?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<()> {
        if self.dir.exists() {
            fs::remove_dir_all(&self.dir)?;
        }
        Ok(())
    }

    pub fn load_sync(&self) -> Result<Option<StoredCredentials>> {
        let path = self.credentials_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };
        match serde_json::from_str(&text) {
            Ok(c) => Ok(Some(c)),
            Err(_) => Ok(None),
        }
    }
}

fn atomic_write_restricted(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_write_0600(path, bytes)
}

#[async_trait]
impl CredentialStore for FileCredentialStore {
    async fn load(&self) -> std::result::Result<Option<StoredCredentials>, AuthError> {
        self.load_sync()
            .map_err(|e| AuthError::InternalError(e.to_string()))
    }

    async fn save(&self, credentials: StoredCredentials) -> std::result::Result<(), AuthError> {
        fs::create_dir_all(&self.dir).map_err(|e| AuthError::InternalError(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700));
        }
        let json = serde_json::to_vec_pretty(&credentials)
            .map_err(|e| AuthError::InternalError(e.to_string()))?;
        atomic_write_restricted(&self.credentials_path(), &json)
            .map_err(|e| AuthError::InternalError(e.to_string()))
    }

    async fn clear(&self) -> std::result::Result<(), AuthError> {
        let path = self.credentials_path();
        if path.exists() {
            fs::remove_file(&path).map_err(|e| AuthError::InternalError(e.to_string()))?;
        }
        Ok(())
    }
}

/// Clear all oauth credentials for a server URL (logout).
pub fn clear_server_credentials(server_url: &str) -> Result<()> {
    FileCredentialStore::for_server(server_url).clear_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{set_cache_dir_override, TEST_PATHS_LOCK};
    use rmcp::transport::auth::StoredCredentials;
    use tempfile::tempdir;

    #[tokio::test]
    async fn roundtrip_and_perms() {
        let dir = {
            let _g = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let dir = tempdir().unwrap();
            set_cache_dir_override(Some(dir.path().to_path_buf()));
            dir
        };
        let store = FileCredentialStore::for_server("https://mcp.example.com");
        let creds = StoredCredentials::new("cid".into(), None, vec![], None);
        store.save(creds.clone()).await.unwrap();
        let loaded = store.load().await.unwrap().unwrap();
        assert_eq!(loaded.client_id, "cid");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store.credentials_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
        store.clear().await.unwrap();
        assert!(store.load().await.unwrap().is_none());
        let _g = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_cache_dir_override(None);
        drop(dir);
    }

    #[test]
    fn sticky_redirect_roundtrip() {
        let _g = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempdir().unwrap();
        set_cache_dir_override(Some(dir.path().to_path_buf()));
        let store = FileCredentialStore::for_server("https://sticky.example");
        let uri = "http://127.0.0.1:4242/callback";
        store.save_sticky_redirect_uri(uri).unwrap();
        assert_eq!(store.load_sticky_redirect_uri().as_deref(), Some(uri));
        store.clear_all().unwrap();
        assert!(store.load_sticky_redirect_uri().is_none());
        set_cache_dir_override(None);
    }

    #[test]
    fn corrupt_returns_none() {
        let _g = TEST_PATHS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempdir().unwrap();
        set_cache_dir_override(Some(dir.path().to_path_buf()));
        let store = FileCredentialStore::for_server("https://x");
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(store.credentials_path(), "not-json").unwrap();
        assert!(store.load_sync().unwrap().is_none());
        set_cache_dir_override(None);
    }
}
