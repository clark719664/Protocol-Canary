use canary_core::{
    ExecutionContext, GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType,
    ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::{
    observe_ledgers, DefaultRpcRunner, GetLedgersResponse, HttpRpcClient, RpcClient, RpcError,
    RpcFixture, RpcMethod, RpcRunner,
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
            std::env::temp_dir().join("canary-rpc-get-ledgers-tests-cache"),
        ),
        options: RunOptions::default(),
    }
}

#[test]
fn parses_get_ledgers_response_with_ledger_headers() {
    let raw = json!({
        "ledgers": [
            {
                "hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "sequence": 1900001,
                "ledgerCloseTime": "1760000001",
                "headerXdr": "AAAABgAAAA==",
                "metadataXdr": "AAAAAgAAAA==",
                "protocolVersion": 28
            },
            {
                "hash": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
                "sequence": 1900002,
                "closeTime": "1760000006",
                "headerXdr": "AAAABgAAAB=="
            }
        ],
        "latestLedger": 1900050,
        "latestLedgerCloseTime": 1760000250u64,
        "oldestLedger": 1800000,
        "oldestLedgerCloseTime": 1759500000u64,
        "cursor": "1900002"
    });
    let parsed: GetLedgersResponse = serde_json::from_value(raw).unwrap();
    assert_eq!(parsed.ledgers.len(), 2);
    assert_eq!(parsed.ledgers[0].sequence, 1900001);
    assert_eq!(parsed.ledgers[0].ledger_close_time, "1760000001");
    assert_eq!(parsed.ledgers[0].protocol_version, Some(28));
    assert_eq!(parsed.ledgers[1].sequence, 1900002);
    assert_eq!(parsed.ledgers[1].ledger_close_time, "1760000006");
    assert_eq!(parsed.latest_ledger, 1900050);
    assert_eq!(parsed.cursor.as_deref(), Some("1900002"));
}

#[tokio::test]
async fn rejects_zero_start_ledger() {
    let server = MockServer::start().await;
    let client = HttpRpcClient::new(server.uri());
    let err = client.get_ledgers(0, Some(5)).await.unwrap_err();
    match err {
        RpcError::InvalidRequest { method, reason } => {
            assert_eq!(method, "getLedgers");
            assert!(reason.contains("greater than 0"), "reason was: {reason}");
        }
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn dispatches_get_ledgers_with_start_ledger_and_pagination_limit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_partial_json(json!({
            "jsonrpc": "2.0",
            "method": "getLedgers",
            "params": {
                "startLedger": 1900001,
                "pagination": {
                    "limit": 2
                }
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "ledgers": [
                    {
                        "hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                        "sequence": 1900001,
                        "ledgerCloseTime": "1760000001",
                        "headerXdr": "AAAABgAAAA=="
                    }
                ],
                "latestLedger": 1900050
            }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let resp = observe_ledgers(&client, 1900001, Some(2)).await.unwrap();
    assert_eq!(resp.ledgers.len(), 1);
    assert_eq!(resp.ledgers[0].sequence, 1900001);
}

#[tokio::test]
async fn runner_executes_get_ledgers_fixture() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "ledgers": [],
                "latestLedger": 1900050
            }
        })))
        .mount(&server)
        .await;

    let raw = r#"
id = "p28-rpc-get-ledgers"
protocol = 28
surface = "rpc"
category = "ledger"
description = "Verify getLedgers response shape"
method = "get-ledgers"

[request]
startLedger = 1900001

[request.pagination]
limit = 5

[[assert]]
kind = "field-exists"
field = "ledgers"
"#;

    let loaded = parse_fixture_str(raw, Path::new("ledgers.toml")).unwrap();
    let rpc_fixture = RpcFixture::from_loaded(&loaded).unwrap();
    assert_eq!(rpc_fixture.method, RpcMethod::GetLedgers);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&rpc_fixture, &sample_context()).await.unwrap();
    assert_eq!(result.status, Status::Pass);
}
