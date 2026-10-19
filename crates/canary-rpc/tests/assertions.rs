use canary_core::{
    ExecutionContext, GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType,
    ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::{
    parse_path_str, parse_toml_path, resolve_path, DefaultRpcRunner, HttpRpcClient,
    PathResolutionError, PathSegment, RpcFixture, RpcRunner, MAX_PATH_DEPTH,
};
use serde_json::json;
use std::path::Path;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_context() -> ExecutionContext {
    ExecutionContext {
        protocol: ProtocolVersion(28),
        project: ProjectContext {
            root: ".".into(),
            name: "test".into(),
            project_type: ProjectType::Unknown,
            capabilities: vec![],
        },
        network: NetworkContext {
            name: NetworkName::Testnet,
            rpc_url: "https://example.invalid".into(),
            passphrase: "Test SDF Network ; September 2015".into(),
            observed_protocol: None,
        },
        fixtures: canary_core::FixtureStore::default(),
        git: GitContext::default(),
        cache: canary_core::CacheStore::new(
            std::env::temp_dir().join("canary-rpc-path-assertions-tests-cache"),
        ),
        options: RunOptions::default(),
    }
}

#[test]
fn resolves_dot_and_bracket_paths_in_deep_json_objects() {
    let payload = json!({
        "result": {
            "fee": {
                "max": "5000",
                "min": "100"
            },
            "items": [
                { "id": "first", "tags": ["a", "b"] },
                { "id": "second", "tags": ["c"] }
            ]
        },
        "sorobanInclusionFee": {
            "p50": "250"
        }
    });

    assert_eq!(
        resolve_path(&payload, "result.fee.max").unwrap(),
        &json!("5000")
    );
    assert_eq!(
        resolve_path(&payload, "result.items[0].id").unwrap(),
        &json!("first")
    );
    assert_eq!(
        resolve_path(&payload, "result.items[0].tags[1]").unwrap(),
        &json!("b")
    );
    assert_eq!(
        resolve_path(&payload, "sorobanInclusionFee.p50").unwrap(),
        &json!("250")
    );
}

#[test]
fn returns_descriptive_errors_for_missing_keys_and_out_of_bounds_indices() {
    let payload = json!({
        "result": {
            "fee": {
                "max": "5000"
            },
            "items": [
                { "id": "first" }
            ],
            "nullable": null
        }
    });

    let err = resolve_path(&payload, "result.fee.median").unwrap_err();
    assert_eq!(
        err,
        PathResolutionError::FieldNotFound {
            segment: "median".to_string(),
            at: "result.fee".to_string(),
        }
    );
    assert!(err.to_string().contains("median"));
    assert!(err.to_string().contains("result.fee"));

    let oob = resolve_path(&payload, "result.items[3].id").unwrap_err();
    assert_eq!(
        oob,
        PathResolutionError::IndexOutOfBounds {
            index: 3,
            len: 1,
            at: "result.items".to_string(),
        }
    );

    // Indexing into a non-array returns TypeMismatch
    let not_array = resolve_path(&payload, "result.fee[0]").unwrap_err();
    assert_eq!(
        not_array,
        PathResolutionError::TypeMismatch {
            segment: "index [0]".to_string(),
            actual_type: "object",
            at: "result.fee".to_string(),
        }
    );

    // Accessing a property on a primitive or null returns TypeMismatch
    let on_prim = resolve_path(&payload, "result.fee.max.sub").unwrap_err();
    assert_eq!(
        on_prim,
        PathResolutionError::TypeMismatch {
            segment: "key \"sub\"".to_string(),
            actual_type: "string",
            at: "result.fee.max".to_string(),
        }
    );

    let on_null = resolve_path(&payload, "result.nullable.child").unwrap_err();
    assert_eq!(
        on_null,
        PathResolutionError::TypeMismatch {
            segment: "key \"child\"".to_string(),
            actual_type: "null",
            at: "result.nullable".to_string(),
        }
    );
}

#[test]
fn enforces_path_depth_limit_of_eight_elements() {
    let depth_8 = "a.b.c.d.e.f.g.h";
    assert_eq!(parse_path_str(depth_8).unwrap().len(), MAX_PATH_DEPTH);

    let depth_9 = "a.b.c.d.e.f.g.h.i";
    assert_eq!(
        parse_path_str(depth_9).unwrap_err(),
        PathResolutionError::DepthExceeded {
            depth: 9,
            max: MAX_PATH_DEPTH,
        }
    );

    let toml_arr: toml::Value =
        toml::from_str(r#"p = ["a", "b", "c", "d", "e", "f", "g", "h", "i"]"#).unwrap();
    let err = parse_toml_path(toml_arr.get("p").unwrap()).unwrap_err();
    assert!(err.contains("maximum depth of 8"), "err was: {err}");

    let toml_valid: toml::Value =
        toml::from_str(r#"p = ["entries", 0, "liveUntilLedgerSeq"]"#).unwrap();
    assert_eq!(
        parse_toml_path(toml_valid.get("p").unwrap()).unwrap(),
        vec![
            PathSegment::Key("entries".to_string()),
            PathSegment::Index(0),
            PathSegment::Key("liveUntilLedgerSeq".to_string()),
        ]
    );
}

#[tokio::test]
async fn runner_evaluates_nested_path_assertions_on_rpc_fixtures() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "friendbotUrl": "https://friendbot.stellar.org",
                "passphrase": "Test SDF Network ; September 2015",
                "protocolVersion": 28,
                "sorobanInclusionFee": {
                    "max": "5000",
                    "p50": "100"
                },
                "entries": [
                    { "liveUntilLedgerSeq": 250000 }
                ]
            }
        })))
        .mount(&server)
        .await;

    let raw = r#"
id = "p28-rpc-nested-paths"
protocol = 28
surface = "rpc"
category = "network"
description = "Verify nested path assertions"
method = "get-network"

[[assert]]
kind = "field-equals"
path = ["sorobanInclusionFee", "p50"]
value = "100"

[[assert]]
kind = "field-equals"
path = ["entries", 0, "liveUntilLedgerSeq"]
value = 250000

[[assert]]
kind = "field-exists"
field = "sorobanInclusionFee.max"

[[assert]]
kind = "field-absent"
path = ["sorobanInclusionFee", "nonExistent"]
"#;

    let loaded = parse_fixture_str(raw, Path::new("nested.toml")).unwrap();
    let rpc_fixture = RpcFixture::from_loaded(&loaded).unwrap();
    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&rpc_fixture, &sample_context()).await.unwrap();
    assert_eq!(result.status, Status::Pass, "details: {:?}", result.details);
}
