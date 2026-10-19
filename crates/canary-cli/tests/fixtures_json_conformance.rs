//! Schema and golden-file conformance for `stellar-canary fixtures --format json`
//! (CA-02-B, CA-02-C, issue #483).
//!
//! `docs/schemas/fixtures-v1.schema.json` is the formal JSON Schema (draft
//! 2020-12) for the inventory at `inventoryVersion` 1. These tests pin both
//! directions: the command's real stdout validates and matches the committed
//! golden byte for byte, and a document that drops required fixture metadata
//! is rejected by that same schema.
//!
//! The validator interprets the schema as data, using only the keywords the
//! schema document contains, so the checks exercise the published file rather
//! than a second copy of the contract. Unknown properties are allowed, which
//! is what CA-02 section 5 requires of consumers.

mod support;

use serde_json::Value;

use support::{run_in, stderr, stdout, TempProject, VALID_STELLAR_VALUE_BASE64};

const SCHEMA_TEXT: &str = include_str!("../../../docs/schemas/fixtures-v1.schema.json");
const GOLDEN: &str = include_str!("golden/fixtures-inventory-v1.json");
const CA02: &str = include_str!("../../../docs/contracts/ca-02-fixtures-json.md");

const SUPPORTED_KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "title",
    "description",
    "type",
    "enum",
    "const",
    "required",
    "properties",
    "items",
    "minimum",
    "additionalProperties",
];

const CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn schema() -> Value {
    serde_json::from_str(SCHEMA_TEXT).expect("the published schema is valid JSON")
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Validates `instance` against `schema`, returning the first violation
/// with a path a reviewer can point at.
fn validate(instance: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("type") {
        let matches = match expected {
            Value::String(wanted) => type_of(instance) == wanted.as_str(),
            Value::Array(wanted) => wanted.iter().any(|w| w.as_str() == Some(type_of(instance))),
            other => return Err(format!("{path}: schema has an unusable `type`: {other}")),
        };
        if !matches {
            return Err(format!(
                "{path}: expected type {expected}, found {}",
                type_of(instance)
            ));
        }
    }
    if let Some(constant) = schema.get("const") {
        if instance != constant {
            return Err(format!("{path}: expected {constant}, found {instance}"));
        }
    }
    if let Some(values) = schema.get("enum") {
        let allowed = values.as_array().is_some_and(|v| v.contains(instance));
        if !allowed {
            return Err(format!("{path}: {instance} is not one of {values}"));
        }
    }
    if let Some(minimum) = schema.get("minimum") {
        match (instance.as_f64(), minimum.as_f64()) {
            (Some(value), Some(min)) if value >= min => {}
            _ => return Err(format!("{path}: {instance} is below the minimum {minimum}")),
        }
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        if let Some(object) = instance.as_object() {
            for field in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(field) {
                    return Err(format!("{path}: required field `{field}` is missing"));
                }
            }
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        if let Some(object) = instance.as_object() {
            for (name, sub_schema) in properties {
                if let Some(value) = object.get(name) {
                    validate(value, sub_schema, &format!("{path}.{name}"))?;
                }
            }
        }
    }
    if let Some(extra) = schema
        .get("additionalProperties")
        .filter(|extra| extra.is_object())
    {
        if let Some(object) = instance.as_object() {
            let known: Vec<&str> = schema
                .get("properties")
                .and_then(Value::as_object)
                .map(|properties| properties.keys().map(String::as_str).collect())
                .unwrap_or_default();
            for (name, value) in object {
                if !known.contains(&name.as_str()) {
                    validate(value, extra, &format!("{path}.{name}"))?;
                }
            }
        }
    }
    if let Some(items) = schema.get("items") {
        if let Some(array) = instance.as_array() {
            for (index, value) in array.iter().enumerate() {
                validate(value, items, &format!("{path}[{index}]"))?;
            }
        }
    }
    Ok(())
}

fn validate_inventory(json_text: &str) -> Result<(), String> {
    let document: Value = serde_json::from_str(json_text).expect("inventory is valid JSON");
    validate(&document, &schema(), "$")
}

/// The three-surface tree CA-02 section 7 asks the golden file to cover,
/// plus one fixture for another protocol so `otherProtocols` is not empty,
/// plus one verification record so that optional object is in the golden.
fn write_inventory_tree(dir: &TempProject, names: (&str, &str, &str)) {
    let (rpc_path, xdr_path, soroban_path) = names;
    dir.write(".stellar-canary.toml", CONFIG);
    dir.write(
        rpc_path,
        r#"id = "a-rpc-network"
protocol = 28
surface = "rpc"
category = "network"
description = "network fixture"
source_reference = "https://example.invalid/ref"
required_capabilities = ["rpc-client"]
method = "get-network"

[verification]
verificationVersion = 1
fixtureId = "a-rpc-network"
method = "source-checked"
checkedAt = "2026-09-28T07:20:00Z"
outcome = "matched"
"#,
    );
    dir.write(
        xdr_path,
        &format!(
            "id = \"m-xdr-value\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"ledger\"\ndescription = \"ledger value fixture\"\nrequired_capabilities = [\"stellar-sdk-dependency\"]\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
    dir.write(
        soroban_path,
        r#"id = "z-soroban-token"
protocol = 28
surface = "soroban"
category = "token"
description = "token contract fixture"
required_capabilities = ["wasm-artifact", "soroban-contract"]
function = "balance"
"#,
    );
    dir.write(
        "fixtures/xdr/old.toml",
        &format!(
            "id = \"p27-old\"\nprotocol = 27\nsurface = \"xdr\"\ncategory = \"legacy\"\ndescription = \"legacy fixture\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n"
        ),
    );
}

fn inventory_stdout(dir: &TempProject) -> String {
    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert!(
        output.status.success(),
        "fixtures --format json failed: {}",
        stderr(&output)
    );
    assert!(
        stderr(&output).is_empty(),
        "JSON mode writes diagnostics only on failure: {}",
        stderr(&output)
    );
    stdout(&output)
}

fn without_paths(document: &str) -> Value {
    let mut value: Value = serde_json::from_str(document).expect("inventory parses");
    for fixture in value["fixtures"].as_array_mut().expect("fixtures") {
        fixture.as_object_mut().expect("fixture").remove("path");
    }
    value
}

#[test]
fn the_published_schema_is_draft_2020_12_for_inventory_version_1() {
    let schema = schema();
    assert_eq!(
        schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
        "the published schema targets draft 2020-12"
    );
    assert!(schema["$id"]
        .as_str()
        .is_some_and(|id| id.contains("fixtures-v1.schema.json")));
    assert_eq!(schema["properties"]["inventoryVersion"]["const"], 1);
    assert_eq!(schema["properties"]["fixtureFormat"]["const"], 1);

    let required = schema["required"].as_array().expect("required");
    for field in [
        "inventoryVersion",
        "toolVersion",
        "fixtureFormat",
        "protocol",
        "directoryFound",
        "counts",
        "otherProtocols",
        "fixtures",
    ] {
        assert!(
            required.iter().any(|value| value == field),
            "`{field}` is required by CA-02 section 3"
        );
    }
    assert_eq!(required.len(), 8, "no optional field may be required");

    let fixture_required = schema["properties"]["fixtures"]["items"]["required"]
        .as_array()
        .expect("fixture required");
    for field in [
        "id",
        "protocol",
        "surface",
        "category",
        "description",
        "path",
        "requiredCapabilities",
    ] {
        assert!(
            fixture_required.iter().any(|value| value == field),
            "fixture `{field}` is required metadata"
        );
    }
}

#[test]
fn the_schema_uses_only_keywords_the_test_validator_interprets() {
    fn walk(node: &Value) {
        let Some(map) = node.as_object() else {
            return;
        };
        for (keyword, value) in map {
            assert!(
                SUPPORTED_KEYWORDS.contains(&keyword.as_str()),
                "the schema uses `{keyword}`, which this test does not interpret"
            );
            match keyword.as_str() {
                "properties" => value
                    .as_object()
                    .into_iter()
                    .flat_map(|properties| properties.values())
                    .for_each(walk),
                "items" | "additionalProperties" => {
                    if value.is_object() {
                        walk(value);
                    }
                }
                _ => {}
            }
        }
    }
    walk(&schema());
}

#[test]
fn the_command_output_matches_the_golden_file_and_the_schema() {
    let dir = TempProject::new("fixtures-json-golden");
    write_inventory_tree(
        &dir,
        (
            "fixtures/rpc/network.toml",
            "fixtures/xdr/value.toml",
            "fixtures/soroban/contract.toml",
        ),
    );

    let first = inventory_stdout(&dir);
    let second = inventory_stdout(&dir);
    assert_eq!(
        first, second,
        "two runs over the same bytes must be identical"
    );

    validate_inventory(&first).unwrap_or_else(|error| {
        panic!("command output does not match fixtures-v1.schema.json: {error}\n{first}")
    });
    assert_eq!(
        first, GOLDEN,
        "stdout drifted from crates/canary-cli/tests/golden/fixtures-inventory-v1.json"
    );

    let document: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(document["toolVersion"], env!("CARGO_PKG_VERSION"));
    let paths: Vec<&str> = document["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|fixture| fixture["path"].as_str().unwrap())
        .collect();
    assert!(
        paths
            .iter()
            .all(|path| !path.contains('\\') && !path.starts_with('/') && !path.contains("..")),
        "paths stay relative with '/' separators: {paths:?}"
    );
}

#[test]
fn renaming_or_moving_files_changes_only_the_path_field() {
    let laid_out = TempProject::new("fixtures-json-layout-a");
    write_inventory_tree(
        &laid_out,
        (
            "fixtures/rpc/network.toml",
            "fixtures/xdr/value.toml",
            "fixtures/soroban/contract.toml",
        ),
    );
    let moved = TempProject::new("fixtures-json-layout-b");
    write_inventory_tree(
        &moved,
        (
            "fixtures/deep/rpc/renamed-network.toml",
            "fixtures/moved-value.toml",
            "fixtures/soroban/nested/token.toml",
        ),
    );

    let left = inventory_stdout(&laid_out);
    let right = inventory_stdout(&moved);
    assert_ne!(
        left, right,
        "the path field has to record the new locations"
    );
    assert_eq!(
        without_paths(&left),
        without_paths(&right),
        "id order, counts and the other fields do not depend on file names"
    );
}

#[test]
fn a_missing_directory_still_validates() {
    let dir = TempProject::new("fixtures-json-missing-schema");
    let output = run_in(
        &dir.path,
        &[
            "fixtures",
            "--format",
            "json",
            "--fixtures-dir",
            "does-not-exist",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    validate_inventory(&text).unwrap_or_else(|error| panic!("{error}\n{text}"));
    let document: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(document["directoryFound"], false);
    assert_eq!(document["fixtures"], serde_json::json!([]));
}

#[test]
fn a_fixture_file_missing_required_metadata_exits_4_with_empty_stdout() {
    let dir = TempProject::new("fixtures-json-missing-metadata");
    dir.write(".stellar-canary.toml", CONFIG);
    // `description` is required fixture metadata. The command must fail
    // the load rather than emit an inventory the schema would reject.
    dir.write(
        "fixtures/rpc/broken.toml",
        "id = \"broken\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"network\"\nmethod = \"get-network\"\n",
    );

    let output = run_in(&dir.path, &["fixtures", "--format", "json"]);
    assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
    assert!(
        stdout(&output).is_empty(),
        "a failure never prints a partial JSON document: {}",
        stdout(&output)
    );
    assert!(
        stderr(&output).contains("description"),
        "the error names the missing metadata: {}",
        stderr(&output)
    );
}

#[test]
fn an_inventory_missing_fixture_metadata_fails_schema_conformance() {
    let mut document: Value = serde_json::from_str(GOLDEN).expect("golden parses");
    document["fixtures"][0]
        .as_object_mut()
        .expect("fixture")
        .remove("description");

    let err =
        validate(&document, &schema(), "$").expect_err("description is required fixture metadata");
    assert!(
        err.contains("$.fixtures[0].description") || err.contains("description"),
        "{err}"
    );
}

#[test]
fn an_unknown_surface_is_rejected() {
    let mut document: Value = serde_json::from_str(GOLDEN).expect("golden parses");
    document["fixtures"][1]["surface"] = Value::String("classic".into());

    let err = validate(&document, &schema(), "$").expect_err("surface is a closed set");
    assert!(err.contains("$.fixtures[1].surface"), "{err}");
}

#[test]
fn the_ca02_documented_example_validates() {
    let doc = CA02.replace("\r\n", "\n");
    let marker = "<!-- contract-example: fixtures-json-v1 -->";
    let at = doc
        .find(marker)
        .expect("CA-02 has a fixtures-json-v1 example");
    let rest = &doc[at + marker.len()..];
    let fence = "```json\n";
    let start = rest.find(fence).expect("example opens a json fence") + fence.len();
    let end = rest[start..].find("```").expect("example fence closes") + start;
    let example = &rest[start..end];

    validate_inventory(example).unwrap_or_else(|error| panic!("{error}\n{example}"));
}
