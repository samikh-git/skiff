//! `skiff doctor` — self-check for agents and humans.

use std::fs;
use std::process::Command;

use serde_json::json;

use crate::bake::load_baked_all;
use crate::error::Result;
use crate::paths::{cache_dir, config_dir};
use crate::spool::spool_dir;

/// Print environment diagnostics (JSON with `--json`).
pub fn handle_doctor(json: bool) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "(unknown)".into());

    let on_path = which_skiff();
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

    let report = json!({
        "ok": cache_ok && config_ok,
        "version": version,
        "binary": exe,
        "on_path": on_path,
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
        "hints": doctor_hints(on_path.as_ref(), cache_ok, config_ok),
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("skiff doctor {}", version);
    println!("  binary:     {exe}");
    match &on_path {
        Some(p) => println!("  on PATH:    {p}"),
        None => println!("  on PATH:    (not found — install via brew/cargo; see skill)"),
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
    for h in doctor_hints(on_path.as_ref(), cache_ok, config_ok) {
        println!("  hint: {h}");
    }
    Ok(())
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

fn ensure_dir_report(path: &std::path::Path) -> bool {
    if path.is_dir() {
        return true;
    }
    fs::create_dir_all(path).is_ok()
}

fn count_files(dir: &std::path::Path) -> usize {
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .count()
}

fn count_subdirs(dir: &std::path::Path) -> usize {
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .count()
}

fn doctor_hints(on_path: Option<&String>, cache_ok: bool, config_ok: bool) -> Vec<String> {
    let mut hints = Vec::new();
    if on_path.is_none() {
        hints.push(
            "Install binary: brew tap samikh-git/tools && brew install skiff \
             — or: cargo install skiff-cli"
                .into(),
        );
    }
    if !cache_ok {
        hints.push("Cannot create cache dir; check SKIFF_CACHE_DIR permissions".into());
    }
    if !config_ok {
        hints.push("Cannot create config dir; check SKIFF_CONFIG_DIR permissions".into());
    }
    if hints.is_empty() {
        hints.push(
            "First run: skiff --mcp-stdio 'npx -y @modelcontextprotocol/server-filesystem /tmp' --agent --list"
                .into(),
        );
    }
    hints
}
