//! Unit tests verifying error diagnostics when parsing malformed RPC fixture
//! TOML files, missing metadata/surface fields, and invalid syntax.

use std::path::Path;

use canary_core::Surface;
use canary_fixtures::{parse_fixture_str, unknown_body_keys, FixtureError};

#[test]
fn rejects_malformed_toml_with_parse_error_and_offending_location() {
    let raw = r#"
id = "p28-rpc-bad-toml"
protocol = 28
surface = "rpc"
category = "ledger"
description = "unterminated string

[[assert]]
kind = "field-exists"
field = "latestLedger"
"#;

    let err = parse_fixture_str(raw, Path::new("rpc/bad_syntax.toml")).unwrap_err();
    match &err {
        FixtureError::Parse { path, .. } => {
            assert_eq!(path, Path::new("rpc/bad_syntax.toml"));
        }
        other => panic!("expected FixtureError::Parse, got {other:?}"),
    }
    let rendered = err.to_string();
    assert!(
        rendered.contains("rpc/bad_syntax.toml"),
        "expected path in error: {rendered}"
    );
}

#[test]
fn rejects_missing_required_metadata_fields_and_invalid_surface() {
    let missing_surface = r#"
id = "p28-rpc-no-surface"
protocol = 28
category = "ledger"
description = "missing surface"
method = "get-network"
"#;
    let err = parse_fixture_str(missing_surface, Path::new("rpc/no_surface.toml")).unwrap_err();
    assert!(matches!(err, FixtureError::Parse { .. }));
    assert!(
        err.to_string().contains("surface"),
        "expected missing surface in error: {err}"
    );

    let invalid_surface = r#"
id = "p28-rpc-bad-surface"
protocol = 28
surface = "not-a-surface"
category = "ledger"
description = "invalid surface"
method = "get-network"
"#;
    let err = parse_fixture_str(invalid_surface, Path::new("rpc/bad_surface.toml")).unwrap_err();
    assert!(matches!(err, FixtureError::Parse { .. }));
    assert!(
        err.to_string().contains("not-a-surface") || err.to_string().contains("surface"),
        "expected invalid surface diagnostic: {err}"
    );
}

#[test]
fn preserves_rpc_body_and_reports_unknown_body_keys_and_assertion_keys() {
    let raw = r#"
id = "p28-rpc-unknown-keys"
protocol = 28
surface = "rpc"
category = "ledger"
description = "fixture with misspelled top-level and assertion keys"
method = "get-network"
params = { invalid = true }

[[asert]]
kind = "field-invalid-operator"
field = "protocolVersion"
unexpected_key = 42
"#;

    let loaded = parse_fixture_str(raw, Path::new("rpc/unknown_keys.toml")).unwrap();
    assert_eq!(loaded.metadata.surface, Surface::Rpc);

    let body_table = loaded.body.as_table().expect("body table");
    let body_diag = unknown_body_keys(body_table, &["method", "request", "assert"])
        .expect("should flag unknown top-level keys");
    assert!(
        body_diag.contains("\"asert\"") && body_diag.contains("\"params\""),
        "expected unknown keys 'asert' and 'params' in diagnostic: {body_diag}"
    );
    assert!(
        body_diag.contains("\"assert\"")
            && body_diag.contains("\"method\"")
            && body_diag.contains("\"request\""),
        "expected accepted keys in diagnostic: {body_diag}"
    );

    let asert_entries = body_table
        .get("asert")
        .and_then(toml::Value::as_array)
        .expect("asert array");
    let assert_table = asert_entries[0].as_table().expect("assert entry table");
    let assert_diag = unknown_body_keys(
        assert_table,
        &["kind", "field", "value", "expected_type", "min", "max"],
    )
    .expect("should flag unknown assertion entry key");
    assert!(
        assert_diag.contains("\"unexpected_key\""),
        "expected offending key in diagnostic: {assert_diag}"
    );
}

#[test]
fn detects_missing_method_and_invalid_request_params_in_parsed_rpc_body() {
    let no_method = r#"
id = "p28-rpc-missing-method"
protocol = 28
surface = "rpc"
category = "ledger"
description = "missing method field"

[[assert]]
kind = "field-exists"
field = "protocolVersion"
"#;

    let loaded = parse_fixture_str(no_method, Path::new("rpc/missing_method.toml")).unwrap();
    let table = loaded.body.as_table().unwrap();
    assert!(
        table.get("method").is_none(),
        "expected method to be absent from body table"
    );
}
