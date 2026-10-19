//! Tests verifying error diagnostics when parsing malformed RPC fixtures,
//! missing `method`, invalid `[request]` parameters, unknown body/assertion
//! keys, and unsupported assertion kinds/types.

use std::path::Path;

use canary_fixtures::parse_fixture_str;
use canary_rpc::{RpcFixture, RpcFixtureError};

fn parse_rpc_fixture(body: &str) -> Result<RpcFixture, RpcFixtureError> {
    let raw = format!(
        r#"id = "p28-rpc-parse-test"
protocol = 28
surface = "rpc"
category = "ledger"
description = "RPC fixture parsing diagnostic test"
{body}
"#
    );
    let loaded = parse_fixture_str(&raw, Path::new("rpc/parse_test.toml")).unwrap();
    RpcFixture::from_loaded(&loaded)
}

#[test]
fn rejects_missing_method_with_descriptive_error() {
    let err = parse_rpc_fixture(
        r#"
[[assert]]
kind = "field-exists"
field = "protocolVersion"
"#,
    )
    .unwrap_err();

    match &err {
        RpcFixtureError::InvalidFixtureBody {
            source_path,
            reason,
        } => {
            assert_eq!(source_path, Path::new("rpc/parse_test.toml"));
            assert!(
                reason.contains("missing required string field \"method\""),
                "unexpected reason: {reason}"
            );
        }
    }
}

#[test]
fn rejects_malformed_request_params_and_unknown_top_level_keys() {
    let unknown_key_err = parse_rpc_fixture(
        r#"
method = "get-network"
params = { bad = 1 }

[[assert]]
kind = "field-exists"
field = "protocolVersion"
"#,
    )
    .unwrap_err();
    assert!(
        unknown_key_err.to_string().contains("\"params\""),
        "expected unknown key 'params' in error: {unknown_key_err}"
    );

    let bad_request_err = parse_rpc_fixture(
        r#"
method = "get-transaction"

[request]
hash = "not-a-64-char-hex-hash"

[[assert]]
kind = "field-exists"
field = "status"
"#,
    )
    .unwrap_err();
    assert!(
        bad_request_err.to_string().contains("hash"),
        "expected offending request parameter 'hash' in error: {bad_request_err}"
    );
}

#[test]
fn rejects_invalid_assertion_kinds_missing_fields_and_unknown_assertion_keys() {
    let bad_kind = parse_rpc_fixture(
        r#"
method = "get-network"

[[assert]]
kind = "field-matches-regex"
field = "passphrase"
"#,
    )
    .unwrap_err();
    assert!(
        bad_kind
            .to_string()
            .contains("unsupported assertion kind \"field-matches-regex\""),
        "unexpected error: {bad_kind}"
    );

    let unknown_assert_key = parse_rpc_fixture(
        r#"
method = "get-network"

[[assert]]
kind = "field-equals"
field = "protocolVersion"
value = 28
operator = "=="
"#,
    )
    .unwrap_err();
    assert!(
        unknown_assert_key.to_string().contains("\"operator\""),
        "expected offending assertion key 'operator' in error: {unknown_assert_key}"
    );

    let empty_asserts = parse_rpc_fixture(
        r#"
method = "get-network"
"#,
    )
    .unwrap_err();
    assert!(
        empty_asserts
            .to_string()
            .contains("needs at least one [[assert]] table"),
        "unexpected error: {empty_asserts}"
    );
}
