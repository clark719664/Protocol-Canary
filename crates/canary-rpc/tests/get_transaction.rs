use canary_core::{
    ExecutionContext, GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType,
    ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::{
    observe_transaction, DefaultRpcRunner, GetTransactionResponse, HttpRpcClient, RpcClient,
    RpcError, RpcFixture, RpcMethod, RpcRunner, TransactionStatus,
};
use serde_json::json;
use std::path::Path;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SAMPLE_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

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
            std::env::temp_dir().join("canary-rpc-get-tx-tests-cache"),
        ),
        options: RunOptions::default(),
    }
}

#[test]
fn parses_success_failed_and_not_found_transaction_statuses() {
    let success_json = json!({
        "status": "SUCCESS",
        "txHash": SAMPLE_HASH,
        "latestLedger": 1900100,
        "latestLedgerCloseTime": "1760000100",
        "oldestLedger": 1800000,
        "oldestLedgerCloseTime": "1759500000",
        "ledger": 1900050,
        "createdAt": "1760000050",
        "applicationOrder": 1,
        "feeBump": false,
        "envelopeXdr": "AAAAAgAAAAB=",
        "resultXdr": "AAAAAAAAAGQ=",
        "resultMetaXdr": "AAAAAwAAAAA="
    });
    let success: GetTransactionResponse = serde_json::from_value(success_json).unwrap();
    assert_eq!(success.status, TransactionStatus::Success);
    assert_eq!(success.ledger, Some(1900050));
    assert_eq!(success.envelope_xdr.as_deref(), Some("AAAAAgAAAAB="));
    assert_eq!(success.result_xdr.as_deref(), Some("AAAAAAAAAGQ="));
    assert_eq!(success.result_meta_xdr.as_deref(), Some("AAAAAwAAAAA="));

    let failed_json = json!({
        "status": "FAILED",
        "txHash": SAMPLE_HASH,
        "latestLedger": 1900100,
        "ledger": 1900055,
        "envelopeXdr": "AAAAAgAAAAB=",
        "resultXdr": "AAAAAAAAAGT/////AAAAAQ==",
        "resultMetaXdr": "AAAAAwAAAAA="
    });
    let failed: GetTransactionResponse = serde_json::from_value(failed_json).unwrap();
    assert_eq!(failed.status, TransactionStatus::Failed);
    assert_eq!(
        failed.result_xdr.as_deref(),
        Some("AAAAAAAAAGT/////AAAAAQ==")
    );

    let not_found_json = json!({
        "status": "NOT_FOUND",
        "latestLedger": 1900100,
        "oldestLedger": 1800000
    });
    let not_found: GetTransactionResponse = serde_json::from_value(not_found_json).unwrap();
    assert_eq!(not_found.status, TransactionStatus::NotFound);
    assert!(not_found.result_xdr.is_none());
    assert!(not_found.envelope_xdr.is_none());
}

#[tokio::test]
async fn rejects_invalid_transaction_hash_lengths_and_characters() {
    let server = MockServer::start().await;
    let client = HttpRpcClient::new(server.uri());

    for bad_hash in [
        "abcd",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde", // 63 chars
        "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF", // uppercase
    ] {
        let err = client.get_transaction(bad_hash).await.unwrap_err();
        match err {
            RpcError::InvalidRequest { method, reason } => {
                assert_eq!(method, "getTransaction");
                assert!(reason.contains("64 lowercase hex"), "reason was: {reason}");
            }
            other => panic!("expected InvalidRequest for {bad_hash:?}, got {other:?}"),
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn dispatches_get_transaction_with_hash_param() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_partial_json(json!({
            "jsonrpc": "2.0",
            "method": "getTransaction",
            "params": {
                "hash": SAMPLE_HASH
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "SUCCESS",
                "txHash": SAMPLE_HASH,
                "latestLedger": 1900100,
                "ledger": 1900050,
                "envelopeXdr": "AAAAAgAAAAB=",
                "resultXdr": "AAAAAAAAAGQ=",
                "resultMetaXdr": "AAAAAwAAAAA="
            }
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let resp = observe_transaction(&client, SAMPLE_HASH).await.unwrap();
    assert_eq!(resp.status, TransactionStatus::Success);
    assert_eq!(resp.tx_hash.as_deref(), Some(SAMPLE_HASH));
    assert_eq!(resp.result_xdr.as_deref(), Some("AAAAAAAAAGQ="));
}

#[tokio::test]
async fn runner_executes_get_transaction_fixture() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "NOT_FOUND",
                "latestLedger": 1900100
            }
        })))
        .mount(&server)
        .await;

    let raw = format!(
        r#"
id = "p28-rpc-get-tx"
protocol = 28
surface = "rpc"
category = "ledger"
description = "Verify getTransaction response shape"
method = "get-transaction"

[request]
hash = "{SAMPLE_HASH}"

[[assert]]
kind = "field-equals"
field = "status"
value = "NOT_FOUND"
"#
    );

    let loaded = parse_fixture_str(&raw, Path::new("tx.toml")).unwrap();
    let rpc_fixture = RpcFixture::from_loaded(&loaded).unwrap();
    assert_eq!(rpc_fixture.method, RpcMethod::GetTransaction);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&rpc_fixture, &sample_context()).await.unwrap();
    assert_eq!(result.status, Status::Pass);
}
