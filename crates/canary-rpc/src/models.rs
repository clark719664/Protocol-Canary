//! Typed request/response shapes for the small subset of Stellar RPC this
//! crate calls: `getNetwork`, `getLatestLedger`, `getHealth`,
//! `getLedgerEntries`, and `simulateTransaction`.
//!
//! These mirror the documented Stellar RPC JSON-RPC methods; XDR-bearing
//! fields stay as base64 strings here; the Soroban runner decodes them
//! with `canary-xdr`/`stellar-xdr` rather than this crate reimplementing
//! that.

use serde::{Deserialize, Serialize};

/// The parameters of `getLedgerEntries`.
///
/// Reference: <https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getLedgerEntries>
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLedgerEntriesRequest {
    /// Base64-encoded `LedgerKey` XDR strings.
    pub keys: Vec<String>,
}

/// One ledger entry returned by `getLedgerEntries`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntryResult {
    /// Base64-encoded `LedgerKey` XDR string.
    pub key: String,
    /// Base64-encoded `LedgerEntryData` XDR string.
    pub xdr: String,
    /// Ledger sequence when the entry was last modified.
    #[serde(alias = "last_modified_ledger_seq")]
    pub last_modified_ledger_seq: u32,
    /// Optional ledger sequence until which the entry is live.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "live_until_ledger_seq"
    )]
    pub live_until_ledger_seq: Option<u32>,
}

/// The result of `getLedgerEntries`.
///
/// Reference: <https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getLedgerEntries>
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLedgerEntriesResponse {
    #[serde(default)]
    pub entries: Vec<LedgerEntryResult>,
    pub latest_ledger: u32,
}

/// The result of `getHealth`.
///
/// Reference: <https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getHealth>
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetHealthResponse {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ledger: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_ledger: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger_retention_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_ledger_close_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_ledger_close_time: Option<String>,
}

/// The result of `getNetwork`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friendbot_url: Option<String>,
    pub passphrase: String,
    pub protocol_version: u32,
}

/// The result of `getLatestLedger`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestLedger {
    pub id: String,
    pub protocol_version: u32,
    pub sequence: u32,
}

/// The parameters of `simulateTransaction`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationRequest {
    /// A base64-encoded, unsigned `TransactionEnvelope` XDR.
    pub transaction: String,
}

/// The result of `simulateTransaction`.
///
/// Only the fields this project's fixtures currently assert on are
/// modeled; unrecognized fields are ignored rather than rejected (see the
/// project's rule that an unrelated new optional field must not break a
/// check).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationResponse {
    pub latest_ledger: u32,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub transaction_data: Option<String>,
    #[serde(default)]
    pub min_resource_fee: Option<String>,
}

impl SimulationResponse {
    pub fn succeeded(&self) -> bool {
        self.error.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_info_deserializes_camel_case_fields() {
        let json = r#"{"friendbotUrl":"https://friendbot.stellar.org","passphrase":"Test SDF Network ; September 2015","protocolVersion":28}"#;
        let info: NetworkInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.protocol_version, 28);
        assert_eq!(
            info.friendbot_url.as_deref(),
            Some("https://friendbot.stellar.org")
        );
    }

    #[test]
    fn network_info_tolerates_missing_optional_friendbot_url() {
        let json = r#"{"passphrase":"Public Global Stellar Network ; September 2015","protocolVersion":28}"#;
        let info: NetworkInfo = serde_json::from_str(json).unwrap();
        assert!(info.friendbot_url.is_none());
    }

    #[test]
    fn network_info_tolerates_unrecognized_extra_fields() {
        let json = r#"{"passphrase":"x","protocolVersion":28,"someNewField":true}"#;
        let info: NetworkInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.protocol_version, 28);
    }

    #[test]
    fn simulation_response_reports_failure_when_error_is_present() {
        let json = r#"{"latestLedger":100,"error":"host invocation failed"}"#;
        let resp: SimulationResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.succeeded());
    }

    #[test]
    fn simulation_response_reports_success_when_error_is_absent() {
        let json = r#"{"latestLedger":100,"transactionData":"AAAA"}"#;
        let resp: SimulationResponse = serde_json::from_str(json).unwrap();
        assert!(resp.succeeded());
    }
}
