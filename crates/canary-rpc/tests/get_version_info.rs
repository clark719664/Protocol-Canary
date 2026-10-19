//! Unit and mock-server integration tests for `getVersionInfo` (`[E-017]`).

use std::path::Path;

use canary_core::{
    CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext, NetworkName,
    ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::{
    observe_version_info, DefaultRpcRunner, HttpRpcClient, RpcClient, RpcError, RpcFixture,
    RpcMethod, RpcRunner, VersionInfoResponse,
};
use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn context() -> ExecutionContext {
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
        fixtures: FixtureStore::default(),
        git: GitContext::default(),
        cache: CacheStore::new(
            std::env::temp_dir().join("canary-rpc-get-version-info-tests-cache"),
        ),
        options: RunOptions::default(),
    }
}

fn load_fixture(id: &str, body_toml: &str) -> RpcFixture {
    let loaded = parse_fixture_str(
        &format!(
            "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"network\"\ndescription = \"getVersionInfo check\"\n{body_toml}"
        ),
        Path::new("get_version_info.toml"),
    )
    .expect("valid fixture header");
    RpcFixture::from_loaded(&loaded).expect("valid rpc fixture")
}

#[tokio::test]
async fn get_version_info_dispatches_jsonrpc_and_parses_ca01_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getVersionInfo",
            "params": {}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "version": "29.0.0-b2b701685c79aee17fe4eb22dbd08a5dfd11594d",
                "commitHash": "b2b701685c79aee17fe4eb22dbd08a5dfd11594d",
                "buildTimestamp": "2026-09-22T14:52:44",
                "captiveCoreVersion": "stellar-core 29.0.0",
                "protocolVersion": 29
            }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = client
        .get_version_info()
        .await
        .expect("getVersionInfo succeeds");
    assert_eq!(
        response,
        VersionInfoResponse {
            app_version: "29.0.0-b2b701685c79aee17fe4eb22dbd08a5dfd11594d".to_string(),
            commit_hash: "b2b701685c79aee17fe4eb22dbd08a5dfd11594d".to_string(),
            build_timestamp: "2026-09-22T14:52:44".to_string(),
            captive_core_version: Some("stellar-core 29.0.0".to_string()),
            protocol_version: 29,
        }
    );
    assert_eq!(response.observed_protocol(), 29);
}

#[tokio::test]
async fn get_version_info_parses_snake_case_and_app_version_aliases() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getVersionInfo",
            "params": {}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "app_version": "28.1.0",
                "commit_hash": "deadbeef1234",
                "build_timestamp": "2026-08-01T00:00:00Z",
                "protocol_version": 28
            }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let response = observe_version_info(&client)
        .await
        .expect("observe_version_info succeeds");
    assert_eq!(response.app_version, "28.1.0");
    assert_eq!(response.commit_hash, "deadbeef1234");
    assert_eq!(response.build_timestamp, "2026-08-01T00:00:00Z");
    assert_eq!(response.protocol_version, 28);
    assert_eq!(response.observed_protocol(), 28);
}

#[tokio::test]
async fn get_version_info_missing_protocol_version_returns_deserialization_error_diagnostic() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getVersionInfo",
            "params": {}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "version": "28.0.0",
                "commitHash": "abcdef123456",
                "buildTimestamp": "2026-08-01T00:00:00Z"
            }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client
        .get_version_info()
        .await
        .expect_err("missing protocolVersion must fail deserialization");
    match &err {
        RpcError::InvalidResponse { method, reason } => {
            assert_eq!(method, "getVersionInfo");
            assert!(
                reason.contains("protocolVersion") || reason.contains("protocol_version"),
                "expected deserialization error mentioning missing protocol_version, got: {reason}"
            );
        }
        other => panic!("expected RpcError::InvalidResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn default_rpc_runner_executes_get_version_info_fixtures() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "version": "29.0.0",
                "commitHash": "b2b7016",
                "buildTimestamp": "2026-09-22T14:52:44",
                "captiveCoreVersion": "stellar-core 29.0.0",
                "protocolVersion": 29
            }
        })))
        .mount(&server)
        .await;

    let fixture = load_fixture(
        "p29-rpc-version-info",
        r#"
        method = "get-version-info"

        [[assert]]
        kind = "field-equals"
        field = "protocolVersion"
        value = 29

        [[assert]]
        kind = "field-exists"
        field = "commitHash"
        "#,
    );
    assert_eq!(fixture.method, RpcMethod::GetVersionInfo);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner
        .run(&fixture, &context())
        .await
        .expect("runner succeeds");
    assert_eq!(result.status, Status::Pass);
}
