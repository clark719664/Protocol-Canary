use canary_core::{
    ExecutionContext, GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType,
    ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::{
    observe_ledger_entries, DefaultRpcRunner, GetLedgerEntriesRequest, GetLedgerEntriesResponse,
    HttpRpcClient, RpcClient, RpcError, RpcFixture, RpcMethod, RpcRunner,
};
use serde_json::json;
use std::path::Path;
use wiremock::matchers::{body_partial_json, method, path};
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
            std::env::temp_dir().join("canary-rpc-ledger-entries-tests-cache"),
        ),
        options: RunOptions::default(),
    }
}

#[test]
fn serializes_keys_array_in_request_params() {
    let req = GetLedgerEntriesRequest {
        keys: vec!["AAAAAA==".to_string(), "BBBBBB==".to_string()],
    };
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val, json!({ "keys": ["AAAAAA==", "BBBBBB=="] }));
}

#[test]
fn parses_entries_and_latest_ledger_and_handles_empty_entries() {
    let full = json!({
        "entries": [
            {
                "key": "AAAAAA==",
                "xdr": "BBBBBB==",
                "lastModifiedLedgerSeq": 18500000,
                "liveUntilLedgerSeq": 18600000
            }
        ],
        "latestLedger": 18500123
    });
    let parsed: GetLedgerEntriesResponse = serde_json::from_value(full).unwrap();
    assert_eq!(parsed.latest_ledger, 18500123);
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].key, "AAAAAA==");
    assert_eq!(parsed.entries[0].xdr, "BBBBBB==");
    assert_eq!(parsed.entries[0].last_modified_ledger_seq, 18500000);
    assert_eq!(parsed.entries[0].live_until_ledger_seq, Some(18600000));

    let empty = json!({
        "entries": [],
        "latestLedger": 18500123
    });
    let parsed_empty: GetLedgerEntriesResponse = serde_json::from_value(empty).unwrap();
    assert!(parsed_empty.entries.is_empty());
    assert_eq!(parsed_empty.latest_ledger, 18500123);
}

#[tokio::test]
async fn rejects_invalid_base64_keys_before_network_dispatch() {
    let server = MockServer::start().await;
    let client = HttpRpcClient::new(server.uri());

    let keys = vec!["not-valid-base64!!!".to_string()];
    let err = client.get_ledger_entries(&keys).await.unwrap_err();
    match err {
        RpcError::InvalidRequest { method, reason } => {
            assert_eq!(method, "getLedgerEntries");
            assert!(reason.contains("base64"), "unexpected reason: {reason}");
        }
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "client should not dispatch a network call when base64 format check fails"
    );
}

#[tokio::test]
async fn dispatches_get_ledger_entries_and_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_partial_json(json!({
            "jsonrpc": "2.0",
            "method": "getLedgerEntries",
            "params": {
                "keys": ["AAAAAA=="]
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "entries": [
                    {
                        "key": "AAAAAA==",
                        "xdr": "CCCCCC==",
                        "lastModifiedLedgerSeq": 900,
                        "liveUntilLedgerSeq": 1200
                    }
                ],
                "latestLedger": 950
            }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let keys = vec!["AAAAAA==".to_string()];
    let resp = observe_ledger_entries(&client, &keys).await.unwrap();
    assert_eq!(resp.latest_ledger, 950);
    assert_eq!(resp.entries.len(), 1);
    assert_eq!(resp.entries[0].xdr, "CCCCCC==");
}

#[tokio::test]
async fn runner_executes_get_ledger_entries_fixture() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "entries": [],
                "latestLedger": 950
            }
        })))
        .mount(&server)
        .await;

    let raw = r#"
id = "p28-rpc-ledger-entries"
protocol = 28
surface = "rpc"
category = "ledger"
description = "Verify getLedgerEntries response shape"
method = "get-ledger-entries"

[request]
keys = ["AAAAAA=="]

[[assert]]
kind = "field-type"
field = "entries"
expected_type = "array"

[[assert]]
kind = "field-type"
field = "latestLedger"
expected_type = "number"
"#;

    let loaded = parse_fixture_str(raw, Path::new("entries.toml")).unwrap();
    let rpc_fixture = RpcFixture::from_loaded(&loaded).unwrap();
    assert_eq!(rpc_fixture.method, RpcMethod::GetLedgerEntries);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&rpc_fixture, &sample_context()).await.unwrap();
    assert_eq!(result.status, Status::Pass);
}
