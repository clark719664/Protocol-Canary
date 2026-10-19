//! Integration tests for execution error result categorization and cache-hit
//! branches across RPC and Soroban checks.

use std::path::{Path, PathBuf};
use std::time::Duration;

use canary_core::{
    CachePolicy, CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext,
    NetworkName, ProjectContext, ProjectType, ProtocolVersion, ResultSource, RunOptions, Status,
    Surface,
};
use canary_fixtures::parse_fixture_str;
use canary_rpc::RpcFixture;
use canary_runner::{execute, CompatibilityPlan};
use canary_soroban::SorobanFixture;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn unique_cache_dir(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "canary-exec-cache-{prefix}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    path
}

fn build_context(cache_root: PathBuf, policy: CachePolicy, endpoint: &str) -> ExecutionContext {
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
            rpc_url: endpoint.into(),
            passphrase: "Test SDF Network ; September 2015".into(),
            observed_protocol: None,
        },
        fixtures: FixtureStore::default(),
        git: GitContext::default(),
        cache: CacheStore::new(cache_root).with_policy(policy),
        options: RunOptions {
            rpc_timeout: 1,
            rpc_max_retries: 0,
            rpc_retry_delay_ms: 10,
            ..RunOptions::default()
        },
    }
}

fn rpc_fixture(id: &str) -> RpcFixture {
    RpcFixture::from_loaded(
        &parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"rpc\"\ncategory = \"network\"\n\
                 description = \"rpc check\"\nmethod = \"get-network\"\n\n\
                 [[assert]]\nkind = \"field-equals\"\nfield = \"protocolVersion\"\nvalue = 28\n"
            ),
            Path::new("rpc.toml"),
        )
        .unwrap(),
    )
    .unwrap()
}

fn soroban_fixture(id: &str, expect_kind: &str) -> SorobanFixture {
    use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
    let source_account = StrkeyPublicKey([0u8; 32]).to_string();
    let contract_id = StrkeyContract([0u8; 32]).to_string();
    SorobanFixture::from_loaded(
        &parse_fixture_str(
            &format!(
                "id = \"{id}\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"cap-85\"\n\
                 description = \"soroban check\"\nsource_account = \"{source_account}\"\n\
                 contract_id = \"{contract_id}\"\nfunction = \"hello\"\nsequence_number = 1\n\n\
                 [[args]]\nkind = \"symbol\"\nvalue = \"world\"\n\n\
                 [expect]\nkind = \"{expect_kind}\"\n"
            ),
            Path::new("soroban.toml"),
        )
        .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn distinguishes_soroban_simulation_mismatch_failures_from_network_timeout_errors() {
    let server = MockServer::start().await;
    // Endpoint returns a simulation error payload when the fixture expects simulation-success:
    // this is a compatibility assertion failure (`Status::Fail`), not a runtime error.
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "latestLedger": 1000,
                "error": "HostError: Error(Contract, #1)"
            }
        })))
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.soroban
        .push(soroban_fixture("s-sim-fail", "simulation-success"));

    let cache_dir = unique_cache_dir("sim-vs-timeout");
    let ctx = build_context(cache_dir.clone(), CachePolicy::default(), &server.uri());
    let results = execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].surface, Surface::Soroban);
    assert_eq!(
        results[0].status,
        Status::Fail,
        "simulation error against simulation-success expectation must be Status::Fail"
    );

    // Now test against an unresponsive endpoint that exceeds rpc_timeout:
    // this is a runtime execution error (`Status::Error`), never cached.
    let timeout_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": { "latestLedger": 1000, "transactionData": "AAAA" }
                })),
        )
        .mount(&timeout_server)
        .await;

    let timeout_ctx = build_context(
        cache_dir.clone(),
        CachePolicy {
            enabled: true,
            live_max_age: Some(Duration::from_secs(300)),
        },
        &timeout_server.uri(),
    );
    let timeout_results = execute(&plan, &timeout_ctx, &timeout_server.uri()).await;
    assert_eq!(timeout_results.len(), 1);
    assert_eq!(
        timeout_results[0].status,
        Status::Error,
        "network timeout must be categorized as Status::Error"
    );
    assert_eq!(timeout_results[0].source, ResultSource::Live);
    let _ = timeout_ctx.cache.clear();
}

#[tokio::test]
async fn rpc_and_soroban_cache_hits_return_replayed_cache_source_without_network_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "passphrase": "Test SDF Network ; September 2015",
                "protocolVersion": 28,
                "latestLedger": 1000,
                "transactionData": "AAAA"
            }
        })))
        .expect(2) // Only 1 RPC + 1 Soroban live request across TWO execute() runs!
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.rpc.push(rpc_fixture("rpc-cached-1"));
    plan.soroban
        .push(soroban_fixture("soroban-cached-1", "simulation-success"));

    let cache_dir = unique_cache_dir("cache-hit-live");
    let policy = CachePolicy {
        enabled: true,
        live_max_age: Some(Duration::from_secs(300)),
    };
    let ctx = build_context(cache_dir.clone(), policy, &server.uri());

    // First run executes live and populates cache
    let first = execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].status, Status::Pass);
    assert_eq!(first[0].source, ResultSource::Live);
    assert_eq!(first[1].status, Status::Pass);
    assert_eq!(first[1].source, ResultSource::Live);

    // Second run hits the cache for both RPC and Soroban without calling the mock server
    let second = execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(second.len(), 2);
    assert_eq!(second[0].status, Status::Pass);
    assert_eq!(
        second[0].source,
        ResultSource::Cache,
        "RPC cache hit must mark source as ResultSource::Cache (replayed)"
    );
    assert_eq!(second[1].status, Status::Pass);
    assert_eq!(
        second[1].source,
        ResultSource::Cache,
        "Soroban cache hit must mark source as ResultSource::Cache (replayed)"
    );

    let _ = ctx.cache.clear();
}

#[tokio::test]
async fn bypasses_cache_when_disabled_and_reexecutes_when_cache_file_is_corrupted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "passphrase": "Test SDF Network ; September 2015",
                "protocolVersion": 28
            }
        })))
        .expect(3)
        .mount(&server)
        .await;

    let mut plan = CompatibilityPlan::default();
    plan.rpc.push(rpc_fixture("rpc-corrupt-test"));

    let cache_dir = unique_cache_dir("corrupt-and-disabled");
    let enabled_policy = CachePolicy {
        enabled: true,
        live_max_age: Some(Duration::from_secs(300)),
    };
    let ctx = build_context(cache_dir.clone(), enabled_policy, &server.uri());

    // 1. Populate cache
    let first = execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(first[0].source, ResultSource::Live);

    // 2. Corrupt all JSON files in the cache directory and verify invalidation + live re-execution
    for entry in std::fs::read_dir(&cache_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            std::fs::write(entry.path(), b"{ corrupted json cache entry ").unwrap();
        }
    }
    let after_corrupt = execute(&plan, &ctx, &server.uri()).await;
    assert_eq!(after_corrupt[0].status, Status::Pass);
    assert_eq!(
        after_corrupt[0].source,
        ResultSource::Live,
        "corrupted cache entry must be treated as a cache miss and re-executed live"
    );

    // 3. Disable cache (`enabled: false`) and verify it bypasses the valid entry just written
    let disabled_ctx = build_context(
        cache_dir.clone(),
        CachePolicy {
            enabled: false,
            live_max_age: Some(Duration::from_secs(300)),
        },
        &server.uri(),
    );
    let disabled_run = execute(&plan, &disabled_ctx, &server.uri()).await;
    assert_eq!(disabled_run[0].status, Status::Pass);
    assert_eq!(
        disabled_run[0].source,
        ResultSource::Live,
        "disabled cache must bypass cached entries and execute live"
    );

    let _ = ctx.cache.clear();
}
