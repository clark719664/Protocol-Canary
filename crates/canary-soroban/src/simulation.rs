//! Calling `simulateTransaction` for a built envelope and decoding returned
//! Soroban authorization entries.

use canary_rpc::{RpcClient, RpcError, SimulationRequest, SimulationResponse};
use stellar_xdr::{
    Limits, ReadXdr, SorobanAuthorizationEntry, SorobanAuthorizedFunction, SorobanCredentials,
};

/// Parsed representation of one `SorobanAuthorizationEntry` returned in
/// `simulateTransaction.results[..].auth`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationAuthEntry {
    /// `"source_account"` or `"address"`.
    pub credentials_kind: String,
    /// Name of the root authorized contract function (or `"create_contract_host_fn"` /
    /// `"create_contract_v2_host_fn"` for contract-creation host functions).
    pub root_function: String,
    /// Number of nested `sub_invocations` attached to the root invocation.
    pub sub_invocation_count: usize,
}

/// Decodes every base64 `SorobanAuthorizationEntry` across `response.results[..].auth`.
pub fn parse_simulation_auth_entries(
    response: &SimulationResponse,
) -> Result<Vec<SimulationAuthEntry>, String> {
    let mut entries = Vec::new();
    for (result_idx, result) in response.results.iter().enumerate() {
        for (auth_idx, raw_b64) in result.auth.iter().enumerate() {
            let entry = SorobanAuthorizationEntry::from_xdr_base64(raw_b64, Limits::none())
                .map_err(|err| {
                    format!("failed to decode results[{result_idx}].auth[{auth_idx}] XDR: {err}")
                })?;
            let credentials_kind = match &entry.credentials {
                SorobanCredentials::SourceAccount => "source_account".to_string(),
                SorobanCredentials::Address(_) => "address".to_string(),
                SorobanCredentials::AddressV2(_) => "address_v2".to_string(),
                SorobanCredentials::AddressWithDelegates(_) => "address_with_delegates".to_string(),
            };
            let root_function = match &entry.root_invocation.function {
                SorobanAuthorizedFunction::ContractFn(call) => {
                    call.function_name.to_utf8_string_lossy()
                }
                SorobanAuthorizedFunction::CreateContractHostFn(_) => {
                    "create_contract_host_fn".to_string()
                }
                SorobanAuthorizedFunction::CreateContractV2HostFn(_) => {
                    "create_contract_v2_host_fn".to_string()
                }
            };
            let sub_invocation_count = entry.root_invocation.sub_invocations.len();
            entries.push(SimulationAuthEntry {
                credentials_kind,
                root_function,
                sub_invocation_count,
            });
        }
    }
    Ok(entries)
}

/// Simulates `envelope_base64` (an unsigned `TransactionEnvelope`) against
/// `client`. This never signs or submits anything.
pub async fn simulate(
    client: &impl RpcClient,
    envelope_base64: String,
) -> Result<SimulationResponse, RpcError> {
    client
        .simulate_transaction(SimulationRequest {
            transaction: envelope_base64,
        })
        .await
}
