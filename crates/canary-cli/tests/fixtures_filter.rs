//! Integration tests for `stellar-canary fixtures --surface` and `--category` (#467).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_stellar-canary")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "stellar-canary-fixtures-filter-{label}-{}-{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn write(&self, rel: &str, contents: &str) {
        let full = self.path.join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(full, contents).expect("write fixture file");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn stellar-canary")
}

fn seed_fixtures(dir: &TempDir) {
    dir.write(
        "fixtures/xdr/p28-xdr-envelope.toml",
        r#"
id = "p28-xdr-envelope"
protocol = 28
surface = "xdr"
category = "envelope"
description = "XDR transaction envelope fixture"
type = "TransactionEnvelope"
kind = "roundtrip"
"#,
    );
    dir.write(
        "fixtures/rpc/p28-rpc-health.toml",
        r#"
id = "p28-rpc-health"
protocol = 28
surface = "rpc"
category = "health"
description = "RPC getHealth response fixture"
method = "getHealth"
"#,
    );
    dir.write(
        "fixtures/rpc/p28-rpc-envelope.toml",
        r#"
id = "p28-rpc-envelope"
protocol = 28
surface = "rpc"
category = "envelope"
description = "RPC envelope simulation fixture"
method = "simulateTransaction"
"#,
    );
    dir.write(
        "fixtures/soroban/p28-soroban-auth.toml",
        r#"
id = "p28-soroban-auth"
protocol = 28
surface = "soroban"
category = "auth"
description = "Soroban auth invocation fixture"
function = "transfer"
"#,
    );
}

#[test]
fn unfiltered_fixtures_lists_all_surfaces_and_categories() {
    let dir = TempDir::new("unfiltered");
    seed_fixtures(&dir);

    let out = run(&dir.path, &["fixtures"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("p28-xdr-envelope"));
    assert!(stdout.contains("p28-rpc-health"));
    assert!(stdout.contains("p28-rpc-envelope"));
    assert!(stdout.contains("p28-soroban-auth"));
}

#[test]
fn surface_filter_outputs_only_matching_surface_in_terminal_and_json() {
    let dir = TempDir::new("surface");
    seed_fixtures(&dir);

    let out = run(&dir.path, &["fixtures", "--surface", "xdr"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("p28-xdr-envelope"));
    assert!(!stdout.contains("p28-rpc-health"));
    assert!(!stdout.contains("p28-soroban-auth"));

    let json_out = run(
        &dir.path,
        &["fixtures", "--surface", "xdr", "--format", "json"],
    );
    assert_eq!(json_out.status.code(), Some(0));
    let doc: Value = serde_json::from_slice(&json_out.stdout).expect("valid JSON");
    assert_eq!(doc["counts"]["total"], 1);
    assert_eq!(doc["counts"]["xdr"], 1);
    assert_eq!(doc["counts"]["rpc"], 0);
    assert_eq!(doc["counts"]["soroban"], 0);
    let fixtures = doc["fixtures"].as_array().expect("fixtures array");
    assert_eq!(fixtures.len(), 1);
    assert_eq!(fixtures[0]["id"], "p28-xdr-envelope");
}

#[test]
fn category_filter_and_combined_surface_category_filter_work() {
    let dir = TempDir::new("category");
    seed_fixtures(&dir);

    let cat_out = run(&dir.path, &["fixtures", "--category", "envelope"]);
    assert_eq!(cat_out.status.code(), Some(0));
    let cat_stdout = String::from_utf8_lossy(&cat_out.stdout);
    assert!(cat_stdout.contains("p28-xdr-envelope"));
    assert!(cat_stdout.contains("p28-rpc-envelope"));
    assert!(!cat_stdout.contains("p28-rpc-health"));
    assert!(!cat_stdout.contains("p28-soroban-auth"));

    let combined_out = run(
        &dir.path,
        &[
            "fixtures",
            "--surface",
            "rpc",
            "--category",
            "envelope",
            "--format",
            "json",
        ],
    );
    assert_eq!(combined_out.status.code(), Some(0));
    let doc: Value = serde_json::from_slice(&combined_out.stdout).expect("valid JSON");
    assert_eq!(doc["counts"]["total"], 1);
    assert_eq!(doc["counts"]["rpc"], 1);
    let fixtures = doc["fixtures"].as_array().expect("fixtures array");
    assert_eq!(fixtures.len(), 1);
    assert_eq!(fixtures[0]["id"], "p28-rpc-envelope");
}

#[test]
fn unknown_surface_fails_with_clear_error_naming_valid_surfaces() {
    let dir = TempDir::new("unknown-surface");
    seed_fixtures(&dir);

    let out = run(&dir.path, &["fixtures", "--surface", "unknown"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown surface"));
    assert!(stderr.contains("xdr"));
    assert!(stderr.contains("rpc"));
    assert!(stderr.contains("soroban"));
}
