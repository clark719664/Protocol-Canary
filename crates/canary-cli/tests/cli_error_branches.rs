//! CLI integration tests asserting exit codes and stderr diagnostics for
//! `inspect`, `fixtures`, and `check` error branches: missing config files,
//! invalid flags, malformed configuration, and invalid project/fixture paths.

mod support;

use support::{run_in, stderr, TempProject};

#[test]
fn inspect_with_missing_explicit_config_exits_with_code_2_and_hint() {
    let dir = TempProject::new("cli-err-inspect-missing-cfg");
    let output = run_in(&dir.path, &["inspect", "--config", "does-not-exist.toml"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(err.contains("error:"), "stderr was: {err}");
    assert!(
        err.contains("does-not-exist.toml") || err.contains("configuration"),
        "stderr was: {err}"
    );
    assert!(err.contains("Hint:"), "expected actionable hint: {err}");
}

#[test]
fn inspect_with_malformed_config_exits_with_code_2_and_diagnostic() {
    let dir = TempProject::new("cli-err-inspect-bad-cfg");
    dir.write(".stellar-canary.toml", "version = \"not-an-int\"\n");
    let output = run_in(&dir.path, &["inspect"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr(&output);
    assert!(err.contains("error:"), "stderr was: {err}");
    assert!(
        err.contains(".stellar-canary.toml") || err.contains("configuration"),
        "stderr was: {err}"
    );
}

#[test]
fn fixtures_with_missing_or_malformed_config_exits_with_code_2() {
    let dir = TempProject::new("cli-err-fixtures-bad-cfg");
    let missing = run_in(&dir.path, &["fixtures", "--config", "missing-config.toml"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        stderr(&missing).contains("error:"),
        "stderr: {}",
        stderr(&missing)
    );

    dir.write(".stellar-canary.toml", "version = 999\nprotocol = 28\n");
    let unsupported = run_in(&dir.path, &["fixtures"]);
    assert_eq!(unsupported.status.code(), Some(2));
    assert!(
        stderr(&unsupported).contains("error:"),
        "stderr: {}",
        stderr(&unsupported)
    );
}

#[test]
fn fixtures_with_corrupt_fixture_file_reports_fixture_error_on_stderr() {
    let dir = TempProject::new("cli-err-fixtures-corrupt");
    dir.write(
        "fixtures/xdr/broken.toml",
        "id = \"p28-broken\"\nprotocol = \"bad\"\n",
    );
    let output = run_in(&dir.path, &["fixtures", "--protocol", "28"]);
    assert!(!output.status.success());
    let code = output.status.code().unwrap_or(0);
    assert!(
        code == 2 || code == 4,
        "expected exit code 2 or 4, got {code}"
    );
    let err = stderr(&output);
    assert!(err.contains("error:"), "stderr was: {err}");
    assert!(err.contains("broken.toml"), "stderr was: {err}");
}

#[test]
fn check_with_missing_config_or_nonexistent_project_root_exits_with_code_2() {
    let dir = TempProject::new("cli-err-check-bad-inputs");
    let missing_cfg = run_in(&dir.path, &["check", "--config", "no-such-config.toml"]);
    assert_eq!(missing_cfg.status.code(), Some(2));
    let err = stderr(&missing_cfg);
    assert!(err.contains("error:"), "stderr was: {err}");
    assert!(err.contains("Hint:"), "stderr was: {err}");

    let bad_project = run_in(
        &dir.path,
        &[
            "check",
            "--project",
            "non-existent-project-dir",
            "--offline",
        ],
    );
    assert!(!bad_project.status.success());
    let code = bad_project.status.code().unwrap_or(0);
    assert!(
        code == 1 || code == 2,
        "expected exit code 1 or 2, got {code}"
    );
    assert!(
        stderr(&bad_project).contains("error:"),
        "stderr was: {}",
        stderr(&bad_project)
    );
}

#[test]
fn invalid_flags_and_command_combinations_exit_nonzero_with_clap_help_hints() {
    let dir = TempProject::new("cli-err-invalid-flags");

    for args in [
        &["inspect", "--not-a-valid-flag"][..],
        &["fixtures", "--unknown-flag"][..],
        &["check", "--unknown-flag"][..],
        &["check", "--surface", "not-a-surface"][..],
    ] {
        let output = run_in(&dir.path, args);
        assert!(
            !output.status.success(),
            "expected failure for args {args:?}"
        );
        let code = output.status.code().unwrap_or(0);
        assert!(
            code == 1 || code == 2,
            "expected exit code 1 or 2 for {args:?}, got {code}"
        );
        let err = stderr(&output);
        assert!(
            err.contains("error:") && (err.contains("--help") || err.contains("Usage:")),
            "expected clap error and help hint for {args:?}, got: {err}"
        );
    }
}
