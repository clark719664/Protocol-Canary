//! Integration tests for `field-length` assertions on JSON arrays (CA-01 §4.2).

use std::path::Path;

use canary_fixtures::parse_fixture_str;
use canary_rpc::{FieldAssertion, FieldLengthAssertion, RpcFixture, RpcFixtureError};
use serde_json::json;

fn load_assertion(entry_toml: &str) -> Result<FieldAssertion, RpcFixtureError> {
    let text = format!(
        "id = \"p28-rpc-length\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"ledger\"\n\
         description = \"field-length assertion test\"\nmethod = \"get-network\"\n\n\
         [[assert]]\n{entry_toml}"
    );
    let loaded = parse_fixture_str(&text, Path::new("rpc/length.toml")).unwrap();
    Ok(RpcFixture::from_loaded(&loaded)?.assertions.remove(0))
}

#[test]
fn parses_and_evaluates_exact_length_assertion() {
    let assertion =
        load_assertion("kind = \"field-length\"\nfield = \"items\"\nexact = 3\n").unwrap();
    assert_eq!(
        assertion,
        FieldAssertion::Length(FieldLengthAssertion {
            field: "items".to_string(),
            exact: Some(3),
            min: None,
            max: None,
        })
    );

    assert!(assertion
        .check(&json!({ "items": ["a", "b", "c"] }))
        .is_ok());
    let err = assertion
        .check(&json!({ "items": ["a", "b"] }))
        .unwrap_err();
    assert!(err.contains("== 3"), "{err}");
    assert!(err.contains("length 2"), "{err}");
}

#[test]
fn parses_and_evaluates_min_items_and_max_items_bounds() {
    let assertion = load_assertion(
        "kind = \"field-length\"\nfield = \"entries\"\nmin_items = 2\nmax_items = 4\n",
    )
    .unwrap();

    assert!(assertion.check(&json!({ "entries": [1, 2] })).is_ok());
    assert!(assertion.check(&json!({ "entries": [1, 2, 3] })).is_ok());
    assert!(assertion.check(&json!({ "entries": [1, 2, 3, 4] })).is_ok());

    let short = assertion.check(&json!({ "entries": [1] })).unwrap_err();
    assert!(short.contains("[2, 4]"), "{short}");
    assert!(short.contains("length 1"), "{short}");

    let long = assertion
        .check(&json!({ "entries": [1, 2, 3, 4, 5] }))
        .unwrap_err();
    assert!(long.contains("[2, 4]"), "{long}");
    assert!(long.contains("length 5"), "{long}");
}

#[test]
fn supports_min_and_max_shorthand_keys_and_empty_array_boundary() {
    let empty_allowed =
        load_assertion("kind = \"field-length\"\nfield = \"events\"\nmin = 0\nmax = 0\n").unwrap();
    assert!(empty_allowed.check(&json!({ "events": [] })).is_ok());
    assert!(empty_allowed.check(&json!({ "events": [1] })).is_err());
}

#[test]
fn non_array_targets_fail_with_expected_array_diagnostic() {
    let assertion =
        load_assertion("kind = \"field-length\"\nfield = \"entries\"\nmin_items = 1\n").unwrap();

    for non_array in [
        json!("not-an-array"),
        json!(42),
        json!(true),
        json!(null),
        json!({ "0": "a" }),
    ] {
        let err = assertion
            .check(&json!({ "entries": non_array }))
            .unwrap_err();
        assert!(
            err.contains("expected array"),
            "expected 'expected array' in diagnostic, got: {err}"
        );
    }
}

#[test]
fn missing_field_fails_assertion() {
    let assertion =
        load_assertion("kind = \"field-length\"\nfield = \"entries\"\nmin_items = 0\n").unwrap();
    let err = assertion.check(&json!({})).unwrap_err();
    assert!(err.contains("missing"), "{err}");
}
