//! Restricted directory/file creation (`0o700` dirs, `0o600` files on Unix).
//!
//! Prefer [`atomic_write_0600`] / [`ensure_dir_0700`] over bare `fs::write` for
//! anything that may hold secrets or session metadata.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::Result;

/// Monotonic per-process counter mixed into temp filenames so that multiple
/// `atomic_write_0600` calls within the same process (even on the same
/// target path, e.g. from concurrent threads) never collide either.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Ensure `dir` exists with mode `0o700` on Unix.
pub fn ensure_dir_0700(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Atomically write bytes to `path` with mode `0o600` (create temp with mode, then rename).
pub fn atomic_write_0600(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir_0700(parent)?;
    }
    // Unique per-call temp filename (pid + monotonic counter) so concurrent
    // processes/threads writing the same target path never share a temp
    // inode and interleave writes before either renames into place.
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp.write.{}.{}", std::process::id(), counter));
    {
        let mut file = open_0600(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn open_0600(path: &Path) -> Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path).map_err(Into::into)
}
