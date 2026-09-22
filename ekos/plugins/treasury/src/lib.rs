//! DAO treasury on-chain transaction observer (RFC 0032).
//!
//! Observes the transaction history of one treasury address through a block-explorer REST API
//! (Etherscan-family: `module=account` with `txlist`, `txlistinternal`, `tokentx`) and emits one
//! `ObservationArtifact` per **outgoing transfer** — a payment. `TreasuryClient` is the seam:
//! [`RealTreasuryClient`] is `reqwest` code against the documented API, [`MockTreasuryClient`] is
//! the only path unit tests take, so the mapping logic is exercised with no network dependency
//! (the same two-tier discipline as the GitHub and Confluence connectors).
//!
//! **Status of the real client:** it is written to the explorer API's documented response shapes
//! and its parsing is unit-tested against those shapes, but it has **not been run against a live
//! explorer** — that needs an API key and a real treasury address. Treat live behaviour (rate
//! limits, pagination past 10,000 rows, chain-specific quirks) as unverified.
//!
//! **Multisig batching.** A Safe-style multi-send is one on-chain transaction that pays several
//! recipients. Each sub-transfer the explorer exposes (internal transactions, token transfers)
//! becomes its own payment, keyed by `(tx_hash, index)`, so a batch holding one approved and one
//! unapproved payout can never read as fully approved.
//!
//! Non-goals (RFC 0032): ABI-decoding arbitrary calldata and reading an RPC node directly. Only
//! native transfers and standard ERC-20 transfers the explorer already decodes are covered.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

/// One transfer touching the treasury address, in the shape RFC 0032 specifies plus `index` (the
/// position of this transfer within its transaction, needed to tell batch sub-transfers apart).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnChainTx {
    pub tx_hash: String,
    /// Position within the transaction: `0` for the transaction's own transfer, then each
    /// internal / token transfer in explorer order.
    pub index: u32,
    pub from: String,
    pub to: String,
    /// Decimal string in **whole units** (`"50000"`, `"1.5"`) — never a float, so no precision loss.
    pub value: String,
    /// Token symbol; `None` = the chain's native asset.
    pub token: Option<String>,
    /// Decoded input-data text, when the input is printable text (e.g. a payment reference).
    pub memo: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub block_number: u64,
}

#[derive(Debug, Error)]
pub enum TreasuryClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("explorer api error: {0}")]
    Api(String),
    #[error("unexpected explorer response: {0}")]
    Shape(String),
}

#[async_trait]
pub trait TreasuryClient: Send + Sync {
    /// Every transfer touching `address`, block-explorer-decoded, oldest first.
    async fn list_transactions(&self, address: &str)
    -> Result<Vec<OnChainTx>, TreasuryClientError>;
}

// ── Pure parsing (unit-tested with no network) ─────────────────────────────────────────────────

/// Insert the decimal point into a raw integer amount: `("1500000", 6)` → `"1.5"`. String math
/// only — token amounts routinely exceed `f64`'s 2^53 exact-integer range.
pub fn scale_decimal(raw: &str, decimals: u32) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return "0".into();
    }
    let d = decimals as usize;
    let padded = if digits.len() <= d {
        format!("{}{}", "0".repeat(d - digits.len() + 1), digits)
    } else {
        digits.to_string()
    };
    let (int, frac) = padded.split_at(padded.len() - d);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        int.to_string()
    } else {
        format!("{int}.{frac}")
    }
}

/// A payment reference typed into a transaction's input data, if the input is readable text.
/// Contract calls (a 4-byte selector followed by ABI words) contain non-printable bytes and
/// decode to `None`.
pub fn decode_memo(input_hex: &str) -> Option<String> {
    let hex = input_hex.strip_prefix("0x").unwrap_or(input_hex);
    if hex.len() < 8 || !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect();
    let text = String::from_utf8(bytes?).ok()?;
    let text = text.trim();
    (!text.is_empty() && text.chars().all(|c| !c.is_control())).then(|| text.to_string())
}

/// Unwrap an Etherscan-family envelope `{status, message, result}` into its result rows. Status
/// `"0"` with "No transactions found" is an empty history, not an error.
pub fn parse_explorer_result(body: &Value) -> Result<Vec<Value>, TreasuryClientError> {
    let status = body["status"].as_str().unwrap_or_default();
    let message = body["message"].as_str().unwrap_or_default();
    match (status, body["result"].as_array()) {
        ("1", Some(rows)) => Ok(rows.clone()),
        (_, Some(rows)) if rows.is_empty() => Ok(Vec::new()),
        ("0", _)
            if message
                .to_ascii_lowercase()
                .contains("no transactions found") =>
        {
            Ok(Vec::new())
        }
        _ => Err(TreasuryClientError::Api(format!(
            "status={status:?} message={message:?} result={}",
            body["result"]
        ))),
    }
}

fn s(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}

fn n(v: &Value, key: &str) -> u64 {
    v[key]
        .as_str()
        .and_then(|x| x.parse().ok())
        .unwrap_or_default()
}

fn ts(v: &Value) -> DateTime<Utc> {
    Utc.timestamp_opt(n(v, "timeStamp") as i64, 0)
        .single()
        .unwrap_or_default()
}

/// A native-asset transfer from `txlist` (failed transactions moved no value and are dropped).
pub fn map_native(row: &Value, native_decimals: u32) -> Option<OnChainTx> {
    if row["isError"].as_str() == Some("1") {
        return None;
    }
    Some(OnChainTx {
        tx_hash: s(row, "hash"),
        index: 0,
        from: s(row, "from"),
        to: s(row, "to"),
        value: scale_decimal(&s(row, "value"), native_decimals),
        token: None,
        memo: decode_memo(&s(row, "input")),
        timestamp: ts(row),
        block_number: n(row, "blockNumber"),
    })
}

/// An internal transfer from `txlistinternal` — how a Safe multi-send's sub-payments appear.
pub fn map_internal(row: &Value, native_decimals: u32) -> Option<OnChainTx> {
    if row["isError"].as_str() == Some("1") {
        return None;
    }
    Some(OnChainTx {
        tx_hash: s(row, "hash"),
        index: 0,
        from: s(row, "from"),
        to: s(row, "to"),
        value: scale_decimal(&s(row, "value"), native_decimals),
        token: None,
        memo: None,
        timestamp: ts(row),
        block_number: n(row, "blockNumber"),
    })
}

/// A standard ERC-20 transfer from `tokentx`.
pub fn map_token(row: &Value) -> OnChainTx {
    let decimals = row["tokenDecimal"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .unwrap_or(18);
    OnChainTx {
        tx_hash: s(row, "hash"),
        index: 0,
        from: s(row, "from"),
        to: s(row, "to"),
        value: scale_decimal(&s(row, "value"), decimals),
        token: Some(s(row, "tokenSymbol")).filter(|t| !t.is_empty()),
        memo: None,
        timestamp: ts(row),
        block_number: n(row, "blockNumber"),
    }
}

/// Number the transfers within each transaction (`0, 1, 2 …` in the order given), so every
/// sub-transfer of a batch has its own stable `(tx_hash, index)`.
pub fn assign_indexes(txs: Vec<OnChainTx>) -> Vec<OnChainTx> {
    let mut next: HashMap<String, u32> = HashMap::new();
    txs.into_iter()
        .map(|mut t| {
            let slot = next.entry(t.tx_hash.to_ascii_lowercase()).or_insert(0);
            t.index = *slot;
            *slot += 1;
            t
        })
        .collect()
}

// ── Real client ───────────────────────────────────────────────────────────────────────────────

/// Etherscan-family client. `base_url` defaults to Etherscan's multichain v2 endpoint, which takes
/// a `chainid` parameter; a chain-specific explorer with the same `module=account` API also works.
pub struct RealTreasuryClient {
    base_url: String,
    chain_id: u64,
    api_key: Option<String>,
    native_decimals: u32,
    http: reqwest::Client,
}

pub const DEFAULT_EXPLORER_URL: &str = "https://api.etherscan.io/v2/api";

impl RealTreasuryClient {
    pub fn new(base_url: impl Into<String>, chain_id: u64, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            chain_id,
            api_key,
            native_decimals: 18,
            http: reqwest::Client::new(),
        }
    }

    pub fn with_native_decimals(mut self, decimals: u32) -> Self {
        self.native_decimals = decimals;
        self
    }

    async fn rows(&self, action: &str, address: &str) -> Result<Vec<Value>, TreasuryClientError> {
        let mut req = self.http.get(&self.base_url).query(&[
            ("chainid", self.chain_id.to_string()),
            ("module", "account".into()),
            ("action", action.into()),
            ("address", address.into()),
            ("startblock", "0".into()),
            ("endblock", "99999999999".into()),
            ("sort", "asc".into()),
        ]);
        if let Some(key) = &self.api_key {
            req = req.query(&[("apikey", key)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(TreasuryClientError::Api(format!("http {}", resp.status())));
        }
        parse_explorer_result(&resp.json::<Value>().await?)
    }
}

#[async_trait]
impl TreasuryClient for RealTreasuryClient {
    async fn list_transactions(
        &self,
        address: &str,
    ) -> Result<Vec<OnChainTx>, TreasuryClientError> {
        let mut all = Vec::new();
        for row in self.rows("txlist", address).await? {
            all.extend(map_native(&row, self.native_decimals));
        }
        for row in self.rows("txlistinternal", address).await? {
            all.extend(map_internal(&row, self.native_decimals));
        }
        for row in self.rows("tokentx", address).await? {
            all.push(map_token(&row));
        }
        // Oldest first, then by hash; the sort is stable so explorer order breaks remaining ties.
        all.sort_by(|a, b| {
            a.block_number
                .cmp(&b.block_number)
                .then_with(|| a.tx_hash.cmp(&b.tx_hash))
        });
        Ok(assign_indexes(all))
    }
}

/// In-process client for unit tests — fixed transfers, no network.
pub struct MockTreasuryClient {
    pub txs: Vec<OnChainTx>,
}

impl MockTreasuryClient {
    pub fn new(txs: Vec<OnChainTx>) -> Self {
        Self { txs }
    }
}

#[async_trait]
impl TreasuryClient for MockTreasuryClient {
    async fn list_transactions(
        &self,
        _address: &str,
    ) -> Result<Vec<OnChainTx>, TreasuryClientError> {
        Ok(self.txs.clone())
    }
}

// ── Observer ──────────────────────────────────────────────────────────────────────────────────

/// Emits one artifact per outgoing transfer of the treasury address.
pub struct TreasuryObserver {
    client: Arc<dyn TreasuryClient>,
    chain_id: u64,
    address: String,
}

impl TreasuryObserver {
    pub fn new(client: Arc<dyn TreasuryClient>, chain_id: u64, address: impl Into<String>) -> Self {
        Self {
            client,
            chain_id,
            address: address.into(),
        }
    }
}

/// `true` for a transfer that moved a non-zero amount **out of** `treasury` — a payment. Incoming
/// funds and zero-value calls (a multisig executing a batch) are not payments.
fn is_payment(tx: &OnChainTx, treasury: &str) -> bool {
    tx.from.eq_ignore_ascii_case(treasury) && tx.value != "0"
}

#[async_trait]
impl Observer for TreasuryObserver {
    fn name(&self) -> &str {
        "treasury"
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let txs = self
            .client
            .list_transactions(&self.address)
            .await
            .map_err(|e| {
                ObserveError::connector(format!("treasury list_transactions failed: {e}"))
            })?;
        // RFC 0032 — the same hazard the governance connector was shown to have live: an
        // Etherscan-family explorer answers a valid-but-wrong address with `status: "0"`,
        // "No transactions found", which `parse_explorer_result` correctly reads as an empty
        // history rather than an error. A wrong `EKOS_TREASURY_ADDRESS` or chain id therefore
        // looks exactly like a treasury that has never paid anyone.
        if txs.is_empty() {
            tracing::warn!(
                address = %self.address,
                chain_id = self.chain_id,
                "explorer returned 0 transactions — if that is unexpected, check the address and \
                 chain id (an unused or wrong address is an empty history, not an error)"
            );
        }
        let mut pkg = ObservationPackage::new(
            "treasury",
            format!("chain:{}:{}", self.chain_id, self.address),
        );
        for tx in txs.iter().filter(|t| is_payment(t, &self.address)) {
            let target = format!("chain:{}:tx:{}:{}", self.chain_id, tx.tx_hash, tx.index);
            let data = serde_json::json!({
                "chain_id": self.chain_id,
                "treasury_address": self.address,
                "tx_hash": tx.tx_hash,
                "index": tx.index,
                "from": tx.from,
                "to": tx.to,
                "value": tx.value,
                "token": tx.token,
                "memo": tx.memo,
                "timestamp": tx.timestamp.to_rfc3339(),
                "block_number": tx.block_number,
            });
            pkg.push(
                ObservationArtifact::new("treasury", &target, data)
                    .with_producer("ekos-plugin-treasury"),
            );
        }
        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TREASURY: &str = "0xSAFE000000000000000000000000000000000001";

    fn tx(
        hash: &str,
        index: u32,
        from: &str,
        to: &str,
        value: &str,
        token: Option<&str>,
    ) -> OnChainTx {
        OnChainTx {
            tx_hash: hash.into(),
            index,
            from: from.into(),
            to: to.into(),
            value: value.into(),
            token: token.map(String::from),
            memo: None,
            timestamp: Utc.timestamp_opt(1_770_000_000, 0).unwrap(),
            block_number: 100,
        }
    }

    #[test]
    fn scale_decimal_handles_padding_trimming_and_huge_values() {
        assert_eq!(scale_decimal("1500000", 6), "1.5");
        assert_eq!(scale_decimal("50000000000", 6), "50000");
        assert_eq!(scale_decimal("5", 6), "0.000005");
        assert_eq!(scale_decimal("0", 18), "0");
        assert_eq!(scale_decimal("", 18), "0");
        // Past f64's exact-integer range — must survive untouched.
        assert_eq!(
            scale_decimal("123456789012345678901234567890", 18),
            "123456789012.34567890123456789"
        );
    }

    #[test]
    fn decode_memo_accepts_text_and_rejects_contract_calls() {
        // "PAY 0xprop1" as hex
        let hex = format!(
            "0x{}",
            "PAY 0xprop1"
                .bytes()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        assert_eq!(decode_memo(&hex).as_deref(), Some("PAY 0xprop1"));
        // An ERC-20 transfer selector + ABI words is not text.
        assert_eq!(
            decode_memo(
                "0xa9059cbb0000000000000000000000001111111111111111111111111111111111111111"
            ),
            None
        );
        assert_eq!(decode_memo("0x"), None);
    }

    #[test]
    fn explorer_envelope_distinguishes_empty_from_error() {
        let ok = json!({"status":"1","message":"OK","result":[{"hash":"0x1"}]});
        assert_eq!(parse_explorer_result(&ok).unwrap().len(), 1);
        let empty = json!({"status":"0","message":"No transactions found","result":[]});
        assert!(parse_explorer_result(&empty).unwrap().is_empty());
        let bad = json!({"status":"0","message":"NOTOK","result":"Invalid API Key"});
        assert!(matches!(
            parse_explorer_result(&bad),
            Err(TreasuryClientError::Api(_))
        ));
    }

    #[test]
    fn maps_the_three_explorer_row_shapes() {
        let native = json!({"hash":"0xa","from":"0xF","to":"0xT","value":"1500000000000000000","input":"0x","timeStamp":"1770000000","blockNumber":"100","isError":"0"});
        let n = map_native(&native, 18).unwrap();
        assert_eq!(
            (n.value.as_str(), n.token.as_deref(), n.block_number),
            ("1.5", None, 100)
        );
        let failed = json!({"hash":"0xb","from":"0xF","to":"0xT","value":"1","input":"0x","timeStamp":"1","blockNumber":"1","isError":"1"});
        assert!(
            map_native(&failed, 18).is_none(),
            "a failed transaction moved no value"
        );
        let token = json!({"hash":"0xc","from":"0xF","to":"0xT","value":"50000000000","tokenSymbol":"USDC","tokenDecimal":"6","timeStamp":"1770000000","blockNumber":"101"});
        let t = map_token(&token);
        assert_eq!(
            (t.value.as_str(), t.token.as_deref()),
            ("50000", Some("USDC"))
        );
    }

    #[test]
    fn batch_sub_transfers_get_distinct_indexes() {
        let batch = assign_indexes(vec![
            tx("0xBATCH", 0, TREASURY, "0xr1", "10", None),
            tx("0xbatch", 0, TREASURY, "0xr2", "20", None), // same hash, different case
            tx("0xother", 0, TREASURY, "0xr3", "30", None),
        ]);
        assert_eq!(
            batch.iter().map(|t| t.index).collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
    }

    #[tokio::test]
    async fn emits_one_artifact_per_outgoing_payment_and_skips_inflows_and_zero_value_calls() {
        let client = Arc::new(MockTreasuryClient::new(vec![
            tx("0x1", 0, TREASURY, "0xr1", "50000", Some("USDC")), // payment
            tx("0x2", 0, "0xdonor", TREASURY, "999", None),        // inflow
            tx("0x3", 0, TREASURY, "0xr2", "0", None),             // zero-value call
            tx("0x4", 0, &TREASURY.to_lowercase(), "0xr3", "1.5", None), // payment, other case
        ]));
        let pkg = TreasuryObserver::new(client, 1, TREASURY)
            .scan(&ScanContext::new("."))
            .await
            .unwrap();
        assert_eq!(pkg.len(), 2);
        let first = &pkg.artifacts[0].content.data;
        assert_eq!(first["tx_hash"], "0x1");
        assert_eq!(first["value"], "50000");
        assert_eq!(first["token"], "USDC");
        assert_eq!(first["chain_id"], 1);
    }

    #[tokio::test]
    async fn each_batch_sub_transfer_is_its_own_artifact() {
        let client = Arc::new(MockTreasuryClient::new(vec![
            tx("0xBATCH", 0, TREASURY, "0xr1", "10", None),
            tx("0xBATCH", 1, TREASURY, "0xr2", "20", None),
        ]));
        let pkg = TreasuryObserver::new(client, 1, TREASURY)
            .scan(&ScanContext::new("."))
            .await
            .unwrap();
        assert_eq!(pkg.len(), 2);
        assert_ne!(pkg.artifacts[0].id, pkg.artifacts[1].id);
    }

    #[tokio::test]
    async fn same_history_same_artifact_ids() {
        let mk = || {
            Arc::new(MockTreasuryClient::new(vec![tx(
                "0x1", 0, TREASURY, "0xr1", "5", None,
            )]))
        };
        let ctx = ScanContext::new(".");
        let a = TreasuryObserver::new(mk(), 1, TREASURY)
            .scan(&ctx)
            .await
            .unwrap();
        let b = TreasuryObserver::new(mk(), 1, TREASURY)
            .scan(&ctx)
            .await
            .unwrap();
        assert_eq!(a.artifacts[0].id, b.artifacts[0].id);
    }

    #[tokio::test]
    async fn empty_history_produces_no_artifacts() {
        let pkg = TreasuryObserver::new(Arc::new(MockTreasuryClient::new(vec![])), 1, TREASURY)
            .scan(&ScanContext::new("."))
            .await
            .unwrap();
        assert!(pkg.is_empty());
    }
}
