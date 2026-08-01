//! OAuth CLI validation and bake masking tests.

use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn skiff() -> Command {
    let mut cmd = Command::new(cargo_bin!("skiff"));
    let dir = tempdir().unwrap();
    let cache = dir.path().join("cache");
    let config = dir.path().join("config");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    cmd.env("SKIFF_CACHE_DIR", &cache);
    cmd.env("SKIFF_CONFIG_DIR", &config);
    std::mem::forget(dir);
    cmd
}

#[test]
fn oauth_secret_requires_client_id() {
    skiff()
        .args([
            "--mcp",
            "http://127.0.0.1:9/mcp",
            "--oauth-client-secret",
            "sekrit",
            "--list",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--oauth-client-id"));
}

#[test]
fn oauth_rejected_with_stdio() {
    skiff()
        .args(["--mcp-stdio", "echo hi", "--oauth", "--list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not supported with --mcp-stdio"));
}

#[test]
fn oauth_redirect_https_rejected() {
    skiff()
        .args([
            "--mcp",
            "http://127.0.0.1:9/mcp",
            "--oauth",
            "--oauth-redirect-uri",
            "https://127.0.0.1:8080/callback",
            "--list",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("http://"));
}

#[test]
fn oauth_clear_requires_discovery_url() {
    skiff().args(["--oauth-clear"]).assert().failure();
}

#[test]
fn oauth_clear_with_mcp_url() {
    skiff()
        .args(["--mcp", "http://127.0.0.1:9/mcp", "--oauth-clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleared OAuth credentials"));
}

#[test]
fn bake_show_masks_oauth_client_secret() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&cache).unwrap();

    Command::new(cargo_bin!("skiff"))
        .env("SKIFF_CONFIG_DIR", &config)
        .env("SKIFF_CACHE_DIR", &cache)
        .args([
            "bake",
            "create",
            "oauthed",
            "--mcp",
            "http://example.com/mcp",
            "--oauth",
            "--oauth-client-id",
            "cid",
            "--oauth-client-secret",
            "supersecretvalue",
        ])
        .assert()
        .success();

    let out = Command::new(cargo_bin!("skiff"))
        .env("SKIFF_CONFIG_DIR", &config)
        .env("SKIFF_CACHE_DIR", &cache)
        .args(["bake", "show", "oauthed"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(!text.contains("supersecretvalue"));
    assert!(text.contains("supe****") || text.contains("****"));
}

#[test]
fn bake_show_keeps_env_secret_prefix() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&cache).unwrap();

    Command::new(cargo_bin!("skiff"))
        .env("SKIFF_CONFIG_DIR", &config)
        .env("SKIFF_CACHE_DIR", &cache)
        .args([
            "bake",
            "create",
            "envsec",
            "--mcp",
            "http://example.com/mcp",
            "--oauth-client-id",
            "env:CID",
            "--oauth-client-secret",
            "env:CSEC",
        ])
        .assert()
        .success();

    let out = Command::new(cargo_bin!("skiff"))
        .env("SKIFF_CONFIG_DIR", &config)
        .env("SKIFF_CACHE_DIR", &cache)
        .args(["bake", "show", "envsec"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("env:CSEC"));
}
