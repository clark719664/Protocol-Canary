use canary_core::{
    ExecutionContext, GitContext, NetworkContext, NetworkName, ProjectContext, ProjectType,
    ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::{
    observe_fee_stats, DefaultRpcRunner, FeeStatsResponse, HttpRpcClient, RpcClient, RpcError,
    RpcFixture, RpcMethod, RpcRunner,
};
use serde_json::json;
use std::path::Path;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sample_fee_stats_json() -> serde_json::Value {
    json!({
        "sorobanInclusionFee": {
            "max": "100000",
            "min": "100",
            "mode": "200",
            "p10": "100",
            "p20": "120",
            "p30": "140",
            "p40": "160",
            "p50": "200",
            "p60": "250",
            "p70": "300",
            "p80": "400",
            "p90": "600",
            "p95": "800",
            "p99": "1200",
            "transactionCount": "1450",
            "ledgerCount": 5
        },
        "inclusionFee": {
            "max": "1000",
            "min": "100",
            "mode": "100",
            "p10": "100",
            "p20": "100",
            "p30": "100",
            "p40": "100",
            "p50": "100",
            "p60": "100",
            "p70": "100",
            "p80": "100",
            "p90": "150",
            "p95": "200",
            "p99": "500",
            "transactionCount": "4300",
            "ledgerCount": 5
        },
        "latestLedger": 18940200
    })
}

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
            std::env::temp_dir().join("canary-rpc-fee-stats-tests-cache"),
        ),
        options: RunOptions::default(),
    }
}

#[test]
fn fee_stats_response_deserializes_standard_payload() {
    let payload = sample_fee_stats_json();
    let parsed: FeeStatsResponse = serde_json::from_value(payload).unwrap();
    assert_eq!(parsed.latest_ledger, 18940200);
    assert_eq!(parsed.soroban_inclusion_fee.p50, "200");
    assert_eq!(parsed.soroban_inclusion_fee.p95.as_deref(), Some("800"));
    assert_eq!(parsed.soroban_inclusion_fee.transaction_count, "1450");
    assert_eq!(parsed.soroban_inclusion_fee.ledger_count, 5);
    assert_eq!(parsed.inclusion_fee.max, "1000");
    assert_eq!(parsed.inclusion_fee.ledger_count, 5);
}

#[test]
fn fee_stats_response_rejects_string_for_numerical_ledger_count() {
    let mut bad = sample_fee_stats_json();
    bad["sorobanInclusionFee"]["ledgerCount"] = json!("not-a-number");
    let err = serde_json::from_value::<FeeStatsResponse>(bad).unwrap_err();
    assert!(
        err.to_string().contains("invalid type: string"),
        "unexpected error: {err}"
    );
}

#[test]
fn fee_stats_response_rejects_string_for_latest_ledger() {
    let mut bad = sample_fee_stats_json();
    bad["latestLedger"] = json!("eighteen-million");
    let err = serde_json::from_value::<FeeStatsResponse>(bad).unwrap_err();
    assert!(
        err.to_string().contains("invalid type: string"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn http_client_dispatches_get_fee_stats_and_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_partial_json(json!({
            "jsonrpc": "2.0",
            "method": "getFeeStats"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": sample_fee_stats_json()
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let stats = observe_fee_stats(&client).await.unwrap();
    assert_eq!(stats.latest_ledger, 18940200);
    assert_eq!(stats.soroban_inclusion_fee.p99, "1200");
    assert_eq!(stats.inclusion_fee.p50, "100");
}

#[tokio::test]
async fn http_client_surfaces_invalid_response_on_bad_numerical_field() {
    let server = MockServer::start().await;
    let mut bad = sample_fee_stats_json();
    bad["latestLedger"] = json!("bad-ledger");

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": bad
        })))
        .mount(&server)
        .await;

    let client = HttpRpcClient::new(server.uri());
    let err = client.get_fee_stats().await.unwrap_err();
    match err {
        RpcError::InvalidResponse { method, reason } => {
            assert_eq!(method, "getFeeStats");
            assert!(reason.contains("invalid type"), "reason was: {reason}");
        }
        other => panic!("expected InvalidResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn runner_executes_get_fee_stats_fixture_against_raw_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": sample_fee_stats_json()
        })))
        .mount(&server)
        .await;

    let raw = r#"
id = "p28-rpc-fee-stats"
protocol = 28
surface = "rpc"
category = "network"
description = "Verify getFeeStats response shape"
method = "get-fee-stats"

[[assert]]
kind = "field-type"
field = "sorobanInclusionFee"
expected_type = "object"

[[assert]]
kind = "field-type"
field = "inclusionFee"
expected_type = "object"

[[assert]]
kind = "field-type"
field = "latestLedger"
expected_type = "number"
"#;

    let loaded = parse_fixture_str(raw, Path::new("fee_stats.toml")).unwrap();
    let rpc_fixture = RpcFixture::from_loaded(&loaded).unwrap();
    assert_eq!(rpc_fixture.method, RpcMethod::GetFeeStats);

    let runner = DefaultRpcRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&rpc_fixture, &sample_context()).await.unwrap();
    assert_eq!(result.status, Status::Pass);
}
