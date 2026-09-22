//! RFC 0032 end to end: the real `TreasuryObserver` and `SnapshotObserver` (with their `Mock*Client`s
//! — no network) run through `build → recover → resolve → compile → commit → treasury scan`, and the
//! resulting `AuthorizedBy` candidates are queried back through the MCP read tools.

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use ekos::extension::{EkosExtension, Extensions};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirId, RelationshipKind};
use ekos_observation_sdk::Observer;
use ekos_plugin_governance::{MockGovernanceClient, Proposal, SnapshotObserver};
use ekos_plugin_treasury::{MockTreasuryClient, OnChainTx, TreasuryObserver};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

const TREASURY: &str = "0x5AFE000000000000000000000000000000000001";
const R1: &str = "0x1111111111111111111111111111111111111111";
const R2: &str = "0x2222222222222222222222222222222222222222";
const R3: &str = "0x3333333333333333333333333333333333333333";
const TX_A: &str = "0xa000000000000000000000000000000000000000000000000000000000000001";
const TX_B: &str = "0xb000000000000000000000000000000000000000000000000000000000000002";
const TX_D: &str = "0xd000000000000000000000000000000000000000000000000000000000000004";

fn at(day: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, day, 12, 0, 0).unwrap()
}

fn tx(hash: &str, from: &str, to: &str, value: &str, day: u32) -> OnChainTx {
    OnChainTx {
        tx_hash: hash.into(),
        index: 0,
        from: from.into(),
        to: to.into(),
        value: value.into(),
        token: Some("USDC".into()),
        memo: None,
        timestamp: at(day),
        block_number: day as u64,
    }
}

fn proposal(id: &str, body: &str, closed_day: u32) -> Proposal {
    Proposal {
        id: id.into(),
        title: format!("Proposal {id}"),
        body: body.into(),
        state: "closed".into(),
        author: "0xauthor".into(),
        choices: vec!["For".into(), "Against".into()],
        scores: vec![900.0, 10.0],
        created: at(closed_day - 5),
        end: at(closed_day),
    }
}

struct Fixture;

#[async_trait(?Send)]
impl EkosExtension for Fixture {
    fn name(&self) -> &'static str {
        "treasury-fixture"
    }

    fn observers(&self, _config: &EkosConfig) -> Vec<Box<dyn Observer>> {
        let payments = vec![
            // A: approved by P1 (50,000 USDC to R1), paid the day after approval.
            tx(TX_A, TREASURY, R1, "50000", 11),
            // B: nothing in governance approves 12,345 USDC to R3.
            tx(TX_B, TREASURY, R3, "12345", 12),
            // D: 8,000 USDC to R2, paid on day 3 — BEFORE P2 was approved on day 20.
            tx(TX_D, TREASURY, R2, "8000", 3),
            // An inflow: never a payment.
            tx(
                "0xf000000000000000000000000000000000000000000000000000000000000005",
                R3,
                TREASURY,
                "1",
                4,
            ),
        ];
        let proposals = vec![
            proposal(
                "0xprop1",
                &format!("Fund marketing: pay 50,000 USDC to {R1}."),
                10,
            ),
            proposal(
                "0xprop2",
                &format!("Fund tooling: pay 8,000 USDC to {R2}."),
                20,
            ),
        ];
        vec![
            Box::new(TreasuryObserver::new(
                Arc::new(MockTreasuryClient::new(payments)),
                1,
                TREASURY,
            )),
            Box::new(SnapshotObserver::new(
                Arc::new(MockGovernanceClient::new(proposals)),
                "dao.eth",
            )),
        ]
    }
}

fn setup(dir: &Path) -> EkosConfig {
    std::fs::create_dir_all(dir.join("app")).unwrap();
    std::fs::write(dir.join("app/main.rs"), b"fn main() {}").unwrap();
    std::fs::write(
        dir.join("ekos.toml"),
        b"[workspace]\nroot = \".\"\n\n\
          [observe]\npaths = [\"app\"]\nignore-patterns = [\".ekos\"]\n\n\
          [llm]\napi-key-env = \"EKOS_TEST_KEY_THAT_DOES_NOT_EXIST\"\n",
    )
    .unwrap();
    EkosConfig::from_file(&dir.join("ekos.toml")).unwrap()
}

fn tool(config: &EkosConfig, dir: &Path, ext: &Extensions, name: &str, args: Value) -> Value {
    let line = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": { "name": name, "arguments": args } })
    .to_string();
    let mut cache = ekos::commands::mcp::StoreCache::new();
    let resp =
        ekos::commands::mcp::handle_message_with(config, dir, &line, &mut cache, ext).unwrap();
    let v: Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["result"]["isError"], false, "{name} failed: {v}");
    serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn payments_are_matched_to_approvals_end_to_end_and_unmatched_ones_have_no_relationship() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let config = setup(dir);
    let ext = Extensions::new(vec![Arc::new(Fixture)]);

    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run_with(&config, dir, &ext)
        .await
        .unwrap();
    ekos::commands::recover::run_with(&config, dir, false, &ext)
        .await
        .unwrap();
    ekos::commands::resolve::run(&config, dir, true).unwrap();
    ekos::commands::compile::run(&config, dir).await.unwrap();
    ekos::commands::commit::run_with(&config, dir, true, &ext)
        .await
        .unwrap();
    ekos::commands::treasury::scan(&config, dir).unwrap();

    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let id_of = |tx: &str| KirId(ekos_recovery::treasury_analyzer::payment_kir_id(1, tx, 0).0);

    // The inflow was never observed as a payment: exactly three payment objects exist.
    let payments = ledger
        .all_objects()
        .unwrap()
        .into_iter()
        .filter(ekos_identity::treasury::is_payment)
        .count();
    assert_eq!(payments, 3);

    let auth = |rels: Vec<ekos_kir::KirRelationship>| {
        rels.into_iter()
            .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == "AuthorizedBy"))
            .collect::<Vec<_>>()
    };

    // A → P1: a strong, unconfirmed, evidenced candidate.
    let a = auth(ledger.relationships_for(&id_of(TX_A)).unwrap());
    assert_eq!(a.len(), 1, "payment A has exactly one approval candidate");
    assert_eq!(a[0].properties["status"], "unconfirmed");
    assert!(a[0].properties["confidence"].as_f64().unwrap() > 0.9);
    assert_eq!(a[0].properties["before_approval"], false);
    assert!(!a[0].evidence.is_empty());

    // B: nothing approves it — "no AuthorizedBy relationship found" is the answer.
    assert!(auth(ledger.relationships_for(&id_of(TX_B)).unwrap()).is_empty());

    // D → P2: recipient and amount match, but it was paid before the approval — surfaced, flagged,
    // and scored far below a properly-ordered match.
    let d = auth(ledger.relationships_for(&id_of(TX_D)).unwrap());
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].properties["before_approval"], true);
    assert!(
        d[0].properties["confidence"].as_f64().unwrap()
            < a[0].properties["confidence"].as_f64().unwrap() * 0.5
    );
    drop(ledger);

    // Queryable through the existing MCP read tools, no new surface.
    let state = tool(
        &config,
        dir,
        &ext,
        "ekos_state",
        json!({ "id": id_of(TX_A).to_string() }),
    );
    let rels = state["relationships"]
        .as_array()
        .expect("ekos_state lists relationships");
    assert!(
        rels.iter()
            .any(|r| r["kind"] == "AuthorizedBy" && r["properties"]["status"] == "unconfirmed"),
        "ekos_state must show the candidate: {state}"
    );

    // Confirm A's candidate through the one write-capable review tool.
    let review = tool(
        &config,
        dir,
        &ext,
        "ekos_identity_review",
        json!({ "relationship_id": a[0].id.to_string(), "decision": "confirmed" }),
    );
    assert_eq!(review["decision"], "confirmed");

    // A re-scan must keep that human decision.
    ekos::commands::treasury::scan(&config, dir).unwrap();
    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let a = auth(ledger.relationships_for(&id_of(TX_A)).unwrap());
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].properties["status"], "confirmed");
}
