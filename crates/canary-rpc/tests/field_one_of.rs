//! Integration tests for `field-one-of` assertions and the `integer`
//! JSON type (CA-01 §4.2).

use std::path::Path;

use canary_fixtures::parse_fixture_str;
use canary_rpc::{FieldAssertion, FieldOneOfAssertion, JsonType, RpcFixture, RpcFixtureError};
use serde_json::json;

fn load_assertion(entry_toml: &str) -> Result<FieldAssertion, RpcFixtureError> {
    let text = format!(
        "id = \"p28-rpc-one-of\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"ledger\"\n\
         description = \"field-one-of assertion test\"\nmethod = \"get-network\"\n\n\
         [[assert]]\n{entry_toml}"
    );
    let loaded = parse_fixture_str(&text, Path::new("rpc/one_of.toml")).unwrap();
    Ok(RpcFixture::from_loaded(&loaded)?.assertions.remove(0))
}

#[test]
fn parses_and_evaluates_string_integer_and_boolean_one_of_assertions() {
    let status = load_assertion(
        "kind = \"field-one-of\"\nfield = \"status\"\nvalues = [\"SUCCESS\", \"NOT_FOUND\", \"FAILED\"]\n",
    )
    .unwrap();
    assert_eq!(
        status,
        FieldAssertion::OneOf(FieldOneOfAssertion {
            field: "status".to_string(),
            allowed: vec![json!("SUCCESS"), json!("NOT_FOUND"), json!("FAILED")],
        })
    );
    assert!(status.check(&json!({ "status": "SUCCESS" })).is_ok());
    assert!(status.check(&json!({ "status": "NOT_FOUND" })).is_ok());
    let err = status.check(&json!({ "status": "UNKNOWN" })).unwrap_err();
    assert!(err.contains("\"UNKNOWN\""), "{err}");
    assert!(
        err.contains("[\"SUCCESS\", \"NOT_FOUND\", \"FAILED\"]"),
        "{err}"
    );

    let protocol =
        load_assertion("kind = \"field-one-of\"\nfield = \"protocolVersion\"\nvalues = [28, 29]\n")
            .unwrap();
    assert!(protocol.check(&json!({ "protocolVersion": 28 })).is_ok());
    assert!(protocol.check(&json!({ "protocolVersion": 29 })).is_ok());
    let err = protocol
        .check(&json!({ "protocolVersion": 27 }))
        .unwrap_err();
    assert!(err.contains("[28, 29]"), "{err}");
    assert!(err.contains("but found 27"), "{err}");

    let boolean =
        load_assertion("kind = \"field-one-of\"\nfield = \"ready\"\nallowed = [true]\n").unwrap();
    assert!(boolean.check(&json!({ "ready": true })).is_ok());
    let err = boolean.check(&json!({ "ready": false })).unwrap_err();
    assert!(err.contains("[true]"), "{err}");
    assert!(err.contains("but found false"), "{err}");
}

#[test]
fn preserves_64bit_signed_and_unsigned_integer_precision_without_coercion() {
    let high_i64 = i64::MAX;
    let low_i64 = i64::MIN;
    let high_u64 = u64::MAX;

    let assertion = FieldAssertion::OneOf(FieldOneOfAssertion::new(
        "ledgerSeq",
        vec![json!(high_i64), json!(low_i64), json!(high_u64)],
    ));

    assert!(assertion.check(&json!({ "ledgerSeq": high_i64 })).is_ok());
    assert!(assertion.check(&json!({ "ledgerSeq": low_i64 })).is_ok());
    assert!(assertion.check(&json!({ "ledgerSeq": high_u64 })).is_ok());
    assert!(assertion
        .check(&json!({ "ledgerSeq": high_i64 - 1 }))
        .is_err());
    assert!(assertion
        .check(&json!({ "ledgerSeq": high_u64 - 1 }))
        .is_err());
    assert!(assertion
        .check(&json!({ "ledgerSeq": high_i64.to_string() }))
        .is_err());
}

#[test]
fn parses_and_evaluates_integer_expected_type() {
    let assertion = load_assertion(
        "kind = \"field-type\"\nfield = \"protocolVersion\"\nexpected_type = \"integer\"\n",
    )
    .unwrap();
    assert_eq!(
        assertion,
        FieldAssertion::TypeIs {
            field: "protocolVersion".to_string(),
            expected: JsonType::Integer,
        }
    );

    assert!(assertion.check(&json!({ "protocolVersion": 28 })).is_ok());
    assert!(assertion
        .check(&json!({ "protocolVersion": i64::MIN }))
        .is_ok());
    assert!(assertion
        .check(&json!({ "protocolVersion": u64::MAX }))
        .is_ok());
    assert!(assertion
        .check(&json!({ "protocolVersion": 28.0 }))
        .is_err());
    assert!(assertion
        .check(&json!({ "protocolVersion": "28" }))
        .is_err());
}
