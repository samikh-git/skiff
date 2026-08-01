//! `skiff doctor` — self-check for agents and humans.
//!
//! Detects a stale PATH binary (older mtime / missing recent features) so agents
//! do not debug against an outdated `~/.cargo/bin/skiff` while developing from source.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

use serde_json::json;

use crate::bake::load_baked_all;
use crate::error::Result;
use crate::paths::{cache_dir, config_dir};
use crate::spool::spool_dir;

/// Features that a current skiff install should expose (probed via PATH binary help).
const EXPECTED_FEATURES: &[&str] = &["completion", "bake_import", "describe", "agent"];

/// Print environment diagnostics (JSON with `--json`).
pub fn handle_doctor(json_out: bool) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let exe_path = std::env::current_exe().ok();
    let exe = exe_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unknown)".into());

    let on_path = which_skiff();
    let path_probe = on_path
        .as_ref()
        .map(|p| probe_path_binary(Path::new(p)))
        .unwrap_or_default();

    let same_binary = match (&exe_path, &on_path) {
        (Some(exe), Some(path)) => paths_same(exe, Path::new(path)),
        _ => false,
    };

    let stale = diagnose_stale(
        version,
        exe_path.as_deref(),
        on_path.as_deref().map(Path::new),
        same_binary,
        &path_probe,
    );

    let cache = cache_dir();
    let config = config_dir();
    let spool = spool_dir();
    let oauth_dir = cache.join("oauth");

    let cache_ok = ensure_dir_report(&cache);
    let config_ok = ensure_dir_report(&config);
    let spool_count = count_files(&spool);
    let oauth_servers = count_subdirs(&oauth_dir);
    let baked = load_baked_all()
        .map(|s| s.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    #[cfg(unix)]
    let sessions = crate::session::session_list()
        .unwrap_or_default()
        .into_iter()
        .map(|e| {
            json!({
                "name": e.name,
                "alive": e.alive,
                "pid": e.pid,
                "source": e.source,
                "transport": e.transport,
            })
        })
        .collect::<Vec<_>>();
    #[cfg(not(unix))]
    let sessions: Vec<serde_json::Value> = Vec::new();

    let ok = cache_ok && config_ok && !stale.is_stale;
    let hints = doctor_hints(on_path.as_ref(), cache_ok, config_ok, &stale, same_binary);

    let report = json!({
        "ok": ok,
        "version": version,
        "binary": exe,
        "on_path": on_path,
        "path_version": path_probe.version,
        "path_same_as_running": same_binary,
        "path_mtime_secs": path_probe.mtime_secs,
        "running_mtime_secs": mtime_secs(exe_path.as_deref()),
        "path_features": path_probe.features,
        "stale_path_binary": stale.is_stale,
        "stale_reasons": stale.reasons,
        "cache_dir": cache.display().to_string(),
        "cache_ok": cache_ok,
        "config_dir": config.display().to_string(),
        "config_ok": config_ok,
        "spool_dir": spool.display().to_string(),
        "spool_files": spool_count,
        "oauth_servers": oauth_servers,
        "baked": baked,
        "sessions": sessions,
        "sessions_supported": cfg!(unix),
        "hints": hints,
    });

    if json_out {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("skiff doctor {version}");
    println!("  binary:     {exe}");
    match &on_path {
        Some(p) => {
            let ver = path_probe.version.as_deref().unwrap_or("(version unknown)");
            let same = if same_binary {
                "same as running"
            } else {
                "differs from running"
            };
            println!("  on PATH:    {p} ({ver}; {same})");
        }
        None => println!("  on PATH:    (not found — install via brew/cargo; see skill)"),
    }
    if stale.is_stale {
        println!("  stale PATH: YES");
        for r in &stale.reasons {
            println!("    - {r}");
        }
    } else if on_path.is_some() {
        println!("  stale PATH: no");
    }
    println!(
        "  cache:      {} {}",
        cache.display(),
        if cache_ok { "ok" } else { "MISSING" }
    );
    println!(
        "  config:     {} {}",
        config.display(),
        if config_ok { "ok" } else { "MISSING" }
    );
    println!(
        "  spool:      {} ({} file(s))",
        spool.display(),
        spool_count
    );
    println!("  oauth:      {oauth_servers} server dir(s)");
    if baked.is_empty() {
        println!("  baked:      (none)");
    } else {
        println!("  baked:      {}", baked.join(", "));
    }
    #[cfg(unix)]
    {
        if sessions.is_empty() {
            println!("  sessions:   (none)");
        } else {
            println!("  sessions:");
            for e in &sessions {
                let name = e.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let alive = e.get("alive").and_then(|v| v.as_bool()).unwrap_or(false);
                let status = if alive { "alive" } else { "dead" };
                println!("    {name} ({status})");
            }
        }
    }
    #[cfg(not(unix))]
    {
        println!("  sessions:   unsupported on this platform");
    }
    for h in hints {
        println!("  hint: {h}");
    }
    Ok(())
}

#[derive(Debug, Default)]
struct PathProbe {
    version: Option<String>,
    mtime_secs: Option<u64>,
    features: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Default)]
struct StaleDiag {
    is_stale: bool,
    reasons: Vec<String>,
}

fn probe_path_binary(path: &Path) -> PathProbe {
    let mut probe = PathProbe {
        mtime_secs: mtime_secs(Some(path)),
        ..Default::default()
    };

    if let Ok(out) = Command::new(path).arg("--version").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            // "skiff 0.1.2" or similar
            if let Some(v) = s.split_whitespace().nth(1) {
                probe.version = Some(v.trim().to_string());
            } else {
                probe.version = Some(s.trim().to_string());
            }
        }
    }

    // Probe recent surfaces without requiring network.
    let bake_help = Command::new(path)
        .args(["bake", "--help"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
        })
        .unwrap_or_default();
    let top_help = Command::new(path)
        .args(["--help"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
        })
        .unwrap_or_default();
    let completion_ok = Command::new(path)
        .args(["completion", "bash"])
        .output()
        .map(|o| {
            o.status.success() && String::from_utf8_lossy(&o.stdout).contains("_skiff_complete")
        })
        .unwrap_or(false);

    probe.features.insert(
        "completion".into(),
        json!(completion_ok || top_help.contains("completion")),
    );
    probe.features.insert(
        "bake_import".into(),
        json!(bake_help.contains("import") || bake_help.contains("Import")),
    );
    probe
        .features
        .insert("describe".into(), json!(top_help.contains("--describe")));
    probe
        .features
        .insert("agent".into(), json!(top_help.contains("--agent")));

    probe
}

fn diagnose_stale(
    running_version: &str,
    running_exe: Option<&Path>,
    path_exe: Option<&Path>,
    same_binary: bool,
    probe: &PathProbe,
) -> StaleDiag {
    let mut diag = StaleDiag::default();
    let Some(path_exe) = path_exe else {
        return diag;
    };

    if same_binary {
        return diag;
    }

    if let Some(path_ver) = &probe.version {
        if path_ver != running_version {
            diag.is_stale = true;
            diag.reasons.push(format!(
                "PATH reports skiff {path_ver} but this process is {running_version}"
            ));
        }
    }

    if let (Some(path_m), Some(run_m)) = (probe.mtime_secs, mtime_secs(running_exe)) {
        if path_m + 60 < run_m {
            // PATH binary more than a minute older than the running binary.
            diag.is_stale = true;
            diag.reasons.push(format!(
                "PATH binary is older than the running binary (mtime {path_m} < {run_m})"
            ));
        }
    }

    for feat in EXPECTED_FEATURES {
        let present = probe
            .features
            .get(*feat)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !present {
            diag.is_stale = true;
            diag.reasons.push(format!(
                "PATH binary missing `{feat}` (reinstall: cargo install --path . --force)"
            ));
        }
    }

    // Dedup-ish: if we already flagged version/mtime, still keep feature reasons —
    // they tell the user *what* is wrong.
    let _ = path_exe;
    diag
}

fn paths_same(a: &Path, b: &Path) -> bool {
    let ca = fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let cb = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    ca == cb
}

fn mtime_secs(path: Option<&Path>) -> Option<u64> {
    let path = path?;
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    Some(
        modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()?
            .as_secs(),
    )
}

fn which_skiff() -> Option<String> {
    let output = Command::new("which").arg("skiff").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn ensure_dir_report(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    fs::create_dir_all(path).is_ok()
}

fn count_files(dir: &Path) -> usize {
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .count()
}

fn count_subdirs(dir: &Path) -> usize {
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .count()
}

fn doctor_hints(
    on_path: Option<&String>,
    cache_ok: bool,
    config_ok: bool,
    stale: &StaleDiag,
    same_binary: bool,
) -> Vec<String> {
    let mut hints = Vec::new();
    if on_path.is_none() {
        hints.push(
            "Install binary: brew tap samikh-git/tools && brew install skiff \
             — or: cargo install skiff-cli"
                .into(),
        );
    }
    if stale.is_stale {
        hints.push(
            "PATH skiff looks stale vs this binary. Update with: \
             cargo install --path . --force \
             — or: brew upgrade skiff \
             — then re-run: skiff doctor"
                .into(),
        );
        if !same_binary {
            if let Some(p) = on_path {
                hints.push(format!(
                    "Running {} while PATH resolves to {p}",
                    std::env::current_exe()
                        .map(|x| x.display().to_string())
                        .unwrap_or_else(|_| "(this binary)".into())
                ));
            }
        }
    }
    if !cache_ok {
        hints.push("Cannot create cache dir; check SKIFF_CACHE_DIR permissions".into());
    }
    if !config_ok {
        hints.push("Cannot create config dir; check SKIFF_CONFIG_DIR permissions".into());
    }
    if hints.is_empty() {
        hints.push(
            "Try: skiff bake import --dry-run \
             — or: skiff --mcp-stdio 'npx -y @modelcontextprotocol/server-filesystem /tmp' --agent --list"
                .into(),
        );
        hints.push("Shell completion: eval \"$(skiff completion bash)\"".into());
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnose_same_binary_not_stale() {
        let probe = PathProbe::default();
        let d = diagnose_stale("0.1.3", None, Some(Path::new("/tmp/skiff")), true, &probe);
        assert!(!d.is_stale);
    }

    #[test]
    fn diagnose_version_mismatch_is_stale() {
        let mut probe = PathProbe {
            version: Some("0.1.0".into()),
            ..Default::default()
        };
        for f in EXPECTED_FEATURES {
            probe.features.insert((*f).into(), json!(true));
        }
        let d = diagnose_stale("0.1.3", None, Some(Path::new("/tmp/skiff")), false, &probe);
        assert!(d.is_stale);
        assert!(d.reasons.iter().any(|r| r.contains("0.1.0")));
    }

    #[test]
    fn diagnose_missing_feature_is_stale() {
        let mut probe = PathProbe {
            version: Some("0.1.3".into()),
            ..Default::default()
        };
        probe.features.insert("agent".into(), json!(true));
        probe.features.insert("describe".into(), json!(true));
        // completion + bake_import missing
        let d = diagnose_stale("0.1.3", None, Some(Path::new("/tmp/skiff")), false, &probe);
        assert!(d.is_stale);
        assert!(d.reasons.iter().any(|r| r.contains("completion")));
        assert!(d.reasons.iter().any(|r| r.contains("bake_import")));
    }
}
