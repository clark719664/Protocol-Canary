use canary_core::{
    CacheStore, ExecutionContext, FixtureStore, GitContext, NetworkContext, NetworkName,
    ProjectContext, ProjectType, ProtocolVersion, RunOptions, Status,
};
use canary_fixtures::LoadedFixture;
use canary_rpc::{HttpRpcClient, SimulationHostFunctionResult, SimulationResponse};
use canary_soroban::{
    parse_simulation_auth_entries, DefaultSorobanRunner, SimulationAuthEntry, SorobanFixture,
    SorobanRunner,
};
use serde_json::json;
use stellar_strkey::{ed25519::PublicKey as StrkeyPublicKey, Contract as StrkeyContract};
use stellar_xdr::{
    ContractId, Hash, InvokeContractArgs, Limits, ScAddress, ScSymbol, ScVal,
    SorobanAddressCredentials, SorobanAuthorizationEntry, SorobanAuthorizedFunction,
    SorobanAuthorizedInvocation, SorobanCredentials, VecM, WriteXdr,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_auth_entry_base64(use_address_credentials: bool, fn_name: &str) -> String {
    let credentials = if use_address_credentials {
        SorobanCredentials::Address(SorobanAddressCredentials {
            address: ScAddress::Contract(ContractId(Hash([7u8; 32]))),
            nonce: 42,
            signature_expiration_ledger: 1000,
            signature: ScVal::Void,
        })
    } else {
        SorobanCredentials::SourceAccount
    };

    let entry = SorobanAuthorizationEntry {
        credentials,
        root_invocation: SorobanAuthorizedInvocation {
            function: SorobanAuthorizedFunction::ContractFn(InvokeContractArgs {
                contract_address: ScAddress::Contract(ContractId(Hash([1u8; 32]))),
                function_name: ScSymbol(fn_name.try_into().unwrap()),
                args: VecM::default(),
            }),
            sub_invocations: VecM::default(),
        },
    };

    entry.to_xdr_base64(Limits::none()).unwrap()
}

fn fixture_with_expect(id: &str, expect_block: &str) -> LoadedFixture {
    let source_account = StrkeyPublicKey([0u8; 32]).to_string();
    let contract_id = StrkeyContract([0u8; 32]).to_string();
    let raw = format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"soroban\"\ncategory = \"cap-85\"\ndescription = \"auth test\"\nsource_account = \"{source_account}\"\ncontract_id = \"{contract_id}\"\nfunction = \"transfer\"\nsequence_number = 1\n\n[[args]]\nkind = \"symbol\"\nvalue = \"world\"\n\n[expect]\n{expect_block}"
    );
    canary_fixtures::parse_fixture_str(&raw, std::path::Path::new("auth_test.toml")).unwrap()
}

fn test_context() -> ExecutionContext {
    ExecutionContext {
        protocol: ProtocolVersion(28),
        project: ProjectContext {
            root: ".".into(),
            name: "test".into(),
            project_type: ProjectType::Soroban,
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
        cache: CacheStore::new(std::env::temp_dir().join("canary-soroban-auth-tests-cache")),
        options: RunOptions::default(),
    }
}

#[test]
fn parses_source_account_and_address_auth_entries_from_simulation_response() {
    let auth0 = make_auth_entry_base64(false, "transfer");
    let auth1 = make_auth_entry_base64(true, "approve");
    let response = SimulationResponse {
        latest_ledger: 1234,
        error: None,
        transaction_data: Some("AAAA".to_string()),
        min_resource_fee: Some("100".to_string()),
        results: vec![SimulationHostFunctionResult {
            xdr: None,
            auth: vec![auth0, auth1],
        }],
    };

    let entries = parse_simulation_auth_entries(&response).unwrap();
    assert_eq!(
        entries,
        vec![
            SimulationAuthEntry {
                credentials_kind: "source_account".to_string(),
                root_function: "transfer".to_string(),
                sub_invocation_count: 0,
            },
            SimulationAuthEntry {
                credentials_kind: "address".to_string(),
                root_function: "approve".to_string(),
                sub_invocation_count: 0,
            },
        ]
    );
}

#[tokio::test]
async fn runner_passes_when_simulation_auth_assertions_match() {
    let server = MockServer::start().await;
    let auth0 = make_auth_entry_base64(false, "transfer");
    let auth1 = make_auth_entry_base64(true, "approve");

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "latestLedger": 1000,
                "transactionData": "AAAA",
                "results": [
                    { "auth": [auth0, auth1] }
                ]
            }
        })))
        .mount(&server)
        .await;

    let loaded = fixture_with_expect(
        "p28-auth-pass",
        "kind = \"simulation-success\"\nauth_count = 2\nauth_credentials = [\"source_account\", \"address\"]\nauth_functions = [\"transfer\", \"approve\"]\n",
    );
    let fixture = SorobanFixture::from_loaded(&loaded).unwrap();
    let runner = DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&fixture, &test_context()).await.unwrap();
    assert_eq!(result.status, Status::Pass);
}

#[tokio::test]
async fn runner_fails_when_simulation_auth_count_or_function_mismatches() {
    let server = MockServer::start().await;
    let auth0 = make_auth_entry_base64(false, "transfer");

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "latestLedger": 1000,
                "transactionData": "AAAA",
                "results": [
                    { "auth": [auth0] }
                ]
            }
        })))
        .mount(&server)
        .await;

    let loaded_wrong_fn = fixture_with_expect(
        "p28-auth-wrong-fn",
        "kind = \"simulation-success\"\nauth_count = 1\nauth_functions = [\"mint\"]\n",
    );
    let fixture = SorobanFixture::from_loaded(&loaded_wrong_fn).unwrap();
    let runner = DefaultSorobanRunner::new(HttpRpcClient::new(server.uri()));
    let result = runner.run(&fixture, &test_context()).await.unwrap();
    assert_eq!(result.status, Status::Fail);
    assert!(result.summary.contains("auth root functions did not match"));
}

#[test]
fn rejects_unknown_auth_credentials_kind_in_fixture() {
    let loaded = fixture_with_expect(
        "p28-auth-invalid-cred",
        "kind = \"simulation-success\"\nauth_credentials = [\"multisig\"]\n",
    );
    let err = SorobanFixture::from_loaded(&loaded).unwrap_err();
    assert!(err
        .to_string()
        .contains("expected \"source_account\", \"address\""));
}
