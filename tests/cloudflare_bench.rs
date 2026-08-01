//! Deterministic Cloudflare MCP token/byte bench (env-gated).
//!
//! ```bash
//! export CF_API_TOKEN=…
//! export MCP2CLI_BENCH_CF=1
//! cargo test --test cloudflare_bench -- --ignored --nocapture
//! ```

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use serde_json::Value;
use std::time::Duration;
use tempfile::tempdir;

fn enabled() -> bool {
    matches!(
        std::env::var("MCP2CLI_BENCH_CF").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    ) && std::env::var("CF_API_TOKEN").is_ok()
}

fn mcp2cli() -> Command {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let mut cmd = Command::new(cargo_bin!("mcp2cli"));
    cmd.env("MCP2CLI_CACHE_DIR", &cache);
    std::mem::forget(dir);
    cmd
}

fn run_list(url: &str, detail: &str) -> (usize, usize) {
    let token = std::env::var("CF_API_TOKEN").unwrap();
    let out = mcp2cli()
        .args([
            "--mcp",
            url,
            "--transport",
            "streamable",
            "--auth-header",
            &format!("Authorization:Bearer:{token}"),
            "--agent",
            "--list",
            "--detail",
            detail,
            "--inline",
        ])
        .timeout(Duration::from_secs(120))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let bytes = out.len();
    let approx_tokens = bytes.div_ceil(4);
    (bytes, approx_tokens)
}

#[test]
#[ignore = "set MCP2CLI_BENCH_CF=1 and CF_API_TOKEN"]
fn cloudflare_docs_progressive_discovery_bytes() {
    if !enabled() {
        eprintln!("skip: MCP2CLI_BENCH_CF / CF_API_TOKEN not set");
        return;
    }

    let url = std::env::var("MCP2CLI_BENCH_URL")
        .unwrap_or_else(|_| "https://docs.mcp.cloudflare.com/mcp".into());

    let (names_b, names_t) = run_list(&url, "names");
    let (brief_b, brief_t) = run_list(&url, "brief");
    let (full_b, full_t) = run_list(&url, "full");

    println!("=== Cloudflare MCP token bench ({url}) ===");
    println!("detail=names  bytes={names_b}  ~tokens={names_t}");
    println!("detail=brief  bytes={brief_b}  ~tokens={brief_t}");
    println!("detail=full   bytes={full_b}  ~tokens={full_t}");
    println!(
        "progressive savings vs full: names {:.1}%, brief {:.1}%",
        100.0 * (1.0 - names_b as f64 / full_b.max(1) as f64),
        100.0 * (1.0 - brief_b as f64 / full_b.max(1) as f64),
    );

    assert!(names_b <= brief_b);
    assert!(brief_b <= full_b);

    // Optional: describe first tool from names list
    let token = std::env::var("CF_API_TOKEN").unwrap();
    let names_out = mcp2cli()
        .args([
            "--mcp",
            &url,
            "--transport",
            "streamable",
            "--auth-header",
            &format!("Authorization:Bearer:{token}"),
            "--list",
            "--json",
            "--detail",
            "names",
            "--inline",
        ])
        .timeout(Duration::from_secs(120))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let names: Value = serde_json::from_slice(&names_out).unwrap();
    if let Some(first) = names
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
    {
        let help = mcp2cli()
            .args([
                "--mcp",
                &url,
                "--transport",
                "streamable",
                "--auth-header",
                &format!("Authorization:Bearer:{token}"),
                "--json",
                "--describe",
                first,
                "--inline",
            ])
            .timeout(Duration::from_secs(120))
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        println!(
            "describe={first}  bytes={}  ~tokens={}",
            help.len(),
            help.len().div_ceil(4)
        );
        let progressive = names_b + help.len();
        println!(
            "names+describe total bytes={progressive} (~{}) vs full catalog {full_b}",
            progressive.div_ceil(4)
        );
        assert!(progressive < full_b || full_b < 500);
    }
}
