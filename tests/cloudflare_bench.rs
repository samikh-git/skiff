//! Deterministic Cloudflare MCP token + speed bench (env-gated).
//!
//! ```bash
//! export CF_API_TOKEN=…   # or CLOUDFLARE_API_TOKEN via .env
//! export SKIFF_BENCH_CF=1
//! cargo test --test cloudflare_bench -- --ignored --nocapture
//! ```

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use serde_json::Value;
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn enabled() -> bool {
    matches!(
        std::env::var("SKIFF_BENCH_CF").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    ) && (std::env::var("CF_API_TOKEN").is_ok() || std::env::var("CLOUDFLARE_API_TOKEN").is_ok())
}

fn skiff_cached(cache: &std::path::Path) -> Command {
    let mut cmd = Command::new(cargo_bin!("skiff"));
    cmd.env("SKIFF_CACHE_DIR", cache);
    cmd
}

struct RunOut {
    bytes: usize,
    tokens: usize,
    ms: u128,
    stdout: Vec<u8>,
}

fn run(cache: &std::path::Path, args: &[&str]) -> RunOut {
    // Prefer env: so the token never appears on argv (skiff rejects literals).
    if std::env::var_os("CF_API_TOKEN").is_none() {
        if let Ok(t) = std::env::var("CLOUDFLARE_API_TOKEN") {
            std::env::set_var("CF_API_TOKEN", t);
        }
    }
    let auth = "Authorization:Bearer:env:CF_API_TOKEN";
    let mut argv = vec![
        "--transport",
        "streamable",
        "--auth-header",
        auth,
        "--inline",
    ];
    argv.extend_from_slice(args);
    let start = Instant::now();
    let out = skiff_cached(cache)
        .args(&argv)
        .timeout(Duration::from_secs(180))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let ms = start.elapsed().as_millis();
    let bytes = out.len();
    RunOut {
        bytes,
        tokens: bytes.div_ceil(4),
        ms,
        stdout: out,
    }
}

fn print_row(label: &str, r: &RunOut) {
    println!(
        "{label:<36} bytes={:<8} ~tokens={:<8} {:>6} ms",
        r.bytes, r.tokens, r.ms
    );
}

#[test]
#[ignore = "set SKIFF_BENCH_CF=1 and CF_API_TOKEN"]
fn cloudflare_docs_progressive_discovery_bytes() {
    if !enabled() {
        eprintln!("skip: SKIFF_BENCH_CF / token not set");
        return;
    }

    let url = std::env::var("SKIFF_BENCH_URL")
        .unwrap_or_else(|_| "https://docs.mcp.cloudflare.com/mcp".into());
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();

    println!("=== Docs MCP token+speed ({url}) ===");
    let cold_names = run(
        &cache,
        &["--mcp", &url, "--agent", "--list", "--detail", "names"],
    );
    print_row("cold names", &cold_names);
    let warm_names = run(
        &cache,
        &["--mcp", &url, "--agent", "--list", "--detail", "names"],
    );
    print_row("warm names (index)", &warm_names);
    let brief = run(
        &cache,
        &["--mcp", &url, "--agent", "--list", "--detail", "brief"],
    );
    print_row("warm brief", &brief);
    let full = run(
        &cache,
        &["--mcp", &url, "--agent", "--list", "--detail", "full"],
    );
    print_row("warm full", &full);

    assert!(cold_names.bytes <= brief.bytes);
    assert!(brief.bytes <= full.bytes);
    assert!(
        warm_names.ms <= cold_names.ms || warm_names.ms < 500,
        "warm path should be fast (cold={}ms warm={}ms)",
        cold_names.ms,
        warm_names.ms
    );
}

#[test]
#[ignore = "set SKIFF_BENCH_CF=1 and CF_API_TOKEN"]
fn cloudflare_code_mode_vs_fat_native() {
    if !enabled() {
        eprintln!("skip: SKIFF_BENCH_CF / token not set");
        return;
    }

    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();

    let cm = "https://mcp.cloudflare.com/mcp";
    let fat = "https://mcp.cloudflare.com/mcp?codemode=false";

    println!("=== Code Mode vs fat native (token + speed) ===");

    // Warm Code Mode
    let _ = run(
        &cache,
        &["--mcp", cm, "--agent", "--list", "--detail", "names"],
    );
    let cm_names = run(
        &cache,
        &["--mcp", cm, "--agent", "--list", "--detail", "names"],
    );
    let cm_full = run(
        &cache,
        &["--mcp", cm, "--agent", "--list", "--detail", "full"],
    );
    print_row("codemode names", &cm_names);
    print_row("codemode full (schemas)", &cm_full);

    // Fat: cold then warm search with agent defaults (--top 20, names)
    let fat_cold_search = run(
        &cache,
        &["--mcp", fat, "--agent", "--search", "workers", "--refresh"],
    );
    print_row("fat search workers COLD", &fat_cold_search);
    let fat_warm_search = run(&cache, &["--mcp", fat, "--agent", "--search", "workers"]);
    print_row("fat search workers WARM", &fat_warm_search);
    let fat_search_uncapped = run(
        &cache,
        &[
            "--mcp", fat, "--agent", "--search", "workers", "--detail", "names", "--top", "99999",
        ],
    );
    print_row("fat search workers uncapped", &fat_search_uncapped);

    let fat_names = run(
        &cache,
        &["--mcp", fat, "--agent", "--list", "--detail", "names"],
    );
    print_row("fat list all names", &fat_names);

    println!(
        "agent search vs Code Mode full: {} tok vs {} tok",
        fat_warm_search.tokens, cm_full.tokens
    );
    println!(
        "warm search speedup vs cold: {:.1}x ({} ms → {} ms)",
        fat_cold_search.ms as f64 / fat_warm_search.ms.max(1) as f64,
        fat_cold_search.ms,
        fat_warm_search.ms
    );

    // Agent capped search should beat uncapped on tokens
    assert!(fat_warm_search.tokens <= fat_search_uncapped.tokens);
    // And ideally near or under Code Mode fixed footprint for a focused search
    assert!(
        fat_warm_search.tokens < 3000,
        "agent --search --top should stay modest, got {}",
        fat_warm_search.tokens
    );

    // Parse agent search output: either array or prefix-groups object
    let v: Value = serde_json::from_slice(&fat_warm_search.stdout).unwrap();
    assert!(v.is_array() || v.get("groups").is_some());
}
