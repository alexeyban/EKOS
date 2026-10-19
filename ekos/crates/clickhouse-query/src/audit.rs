//! Auditability (RFC 0056 Stage 2) — every live query is appended to the ledger as an
//! Evidence/Event pair via `&dyn KnowledgeStore` directly, the same access level
//! `commit.rs`/`crates/simulation` already have. The row data itself is never ledgered, only the
//! fact that a query ran: SQL text, question, row count, and a content-hash of the result —
//! avoids turning live analytical output into permanent ledger bloat while still keeping the
//! "every conclusion traceable to evidence" spirit for the one path that touches a live system.
//!
//! This is the one ledger write a non-review MCP tool makes (`ekos_clickhouse_query`, issue #11):
//! querying a live system is an observation, so it is recorded as one. The event id is derived
//! from `(database, sql, result hash)`, so the same query with the same answer is one event whose
//! ledger history lists each run, not a new row per call.

use ekos_kir::{EventKind, KirEvent, KirEvidence, KirId, SourceLocation};
use ekos_ledger::{KnowledgeStore, LedgerError};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Deterministic id for "the ClickHouse database" as an event subject — stable across queries
/// against the same configured database, same construction `clickhouse_analyzer`'s
/// `table_kir_id` uses for tables.
fn database_kir_id(database: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("clickhouse-query:{database}").as_bytes(),
    ))
}

/// The audit event's id: the same query with the same answer is the same event.
fn query_event_id(database: &str, sql: &str, result_hash: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("clickhouse-query-event:{database}:{result_hash}:{sql}").as_bytes(),
    ))
}

fn content_hash(rows: &[serde_json::Value]) -> String {
    let mut hasher = Sha256::new();
    for row in rows {
        hasher.update(row.to_string().as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Appends one Evidence (the SQL text) + one `Custom("ClickHouseQueryExecuted")` Event
/// (question, row count, result content-hash) to `store`. Returns the event's own id.
pub fn record_query_event(
    store: &dyn KnowledgeStore,
    database: &str,
    question: &str,
    sql: &str,
    rows: &[serde_json::Value],
) -> Result<KirId, LedgerError> {
    let evidence = KirEvidence::new(SourceLocation::file(format!("clickhouse:{database}")), sql);
    let evidence_id = evidence.id;
    store.append_evidence(&evidence)?;

    let result_hash = content_hash(rows);
    let payload = serde_json::json!({
        "database": database,
        "question": question,
        "sql": sql,
        "row_count": rows.len(),
        "result_hash": result_hash,
    });
    let event = KirEvent {
        id: query_event_id(database, sql, &result_hash),
        kind: EventKind::Custom("ClickHouseQueryExecuted".to_string()),
        subject: database_kir_id(database),
        payload,
        evidence: vec![evidence_id],
        occurred_at: chrono::Utc::now(),
    };
    let event_id = event.id;
    store.append_event(&event)?;

    Ok(event_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_ledger::Ledger;
    use tempfile::TempDir;

    fn temp_ledger() -> (Ledger, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ledger.db");
        (Ledger::open(&path).unwrap(), dir)
    }

    #[test]
    fn records_evidence_and_event_but_not_row_data() {
        let (ledger, _dir) = temp_ledger();
        let rows = vec![serde_json::json!({"id": 1}), serde_json::json!({"id": 2})];

        let event_id = record_query_event(
            &ledger,
            "analytics",
            "how many orders?",
            "SELECT * FROM orders",
            &rows,
        )
        .unwrap();

        let event = ledger.get_event(&event_id).unwrap().unwrap();
        assert_eq!(
            event.kind,
            EventKind::Custom("ClickHouseQueryExecuted".to_string())
        );
        assert_eq!(event.payload["row_count"], 2);
        assert_eq!(event.payload["sql"], "SELECT * FROM orders");
        assert!(
            event.payload.get("rows").is_none(),
            "row data must not be ledgered"
        );

        assert_eq!(event.evidence.len(), 1);
        let evidence = ledger.get_evidence(&event.evidence[0]).unwrap().unwrap();
        assert_eq!(evidence.fragment, "SELECT * FROM orders");
    }

    #[test]
    fn same_database_gets_same_event_subject_across_queries() {
        let (ledger, _dir) = temp_ledger();
        let id1 = record_query_event(&ledger, "analytics", "q1", "SELECT 1", &[]).unwrap();
        let id2 = record_query_event(&ledger, "analytics", "q2", "SELECT 2", &[]).unwrap();

        let e1 = ledger.get_event(&id1).unwrap().unwrap();
        let e2 = ledger.get_event(&id2).unwrap().unwrap();
        assert_eq!(e1.subject, e2.subject);
    }

    /// Issue #11: repeating a query with the same answer does not add a new event row.
    #[test]
    fn the_same_query_and_answer_is_one_event() {
        let (ledger, _dir) = temp_ledger();
        let rows = vec![serde_json::json!({"n": 1})];
        let a = record_query_event(&ledger, "analytics", "q", "SELECT 1", &rows).unwrap();
        let b = record_query_event(&ledger, "analytics", "q again", "SELECT 1", &rows).unwrap();
        assert_eq!(a, b);
        let other = vec![serde_json::json!({"n": 2})];
        let c = record_query_event(&ledger, "analytics", "q", "SELECT 1", &other).unwrap();
        assert_ne!(a, c, "a different answer is a different observation");
    }

    /// RFC 0056 amendment: a repeat is not lost. It is a new version of the same event id (the
    /// time and the question differ), on both backends; `get_event` shows the latest.
    #[test]
    fn a_repeated_query_is_a_new_version_not_a_lost_row() {
        let rows = vec![serde_json::json!({"n": 1})];
        let sqlite = temp_ledger();
        let facts_dir = TempDir::new().unwrap();
        let facts = ekos_ledger::FactLedger::open(facts_dir.path()).unwrap();
        for store in [&sqlite.0 as &dyn KnowledgeStore, &facts] {
            let before = store.entry_count().unwrap();
            let a = record_query_event(store, "analytics", "q", "SELECT 1", &rows).unwrap();
            let after_first = store.entry_count().unwrap();
            let b = record_query_event(store, "analytics", "q, again", "SELECT 1", &rows).unwrap();
            assert_eq!(a, b);
            assert!(
                store.entry_count().unwrap() > after_first,
                "the repeat was recorded"
            );
            assert!(after_first > before);
            assert_eq!(
                store.get_event(&a).unwrap().unwrap().payload["question"],
                "q, again"
            );
        }
    }
}
