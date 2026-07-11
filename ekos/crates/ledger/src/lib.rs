use chrono::{DateTime, Utc};
use ekos_kir::{KirEvidence, KirId, KirObject, KirRelationship};
use rusqlite::{Connection, Result as SqlResult, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ledger not initialized at {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryType {
    Object,
    Evidence,
    Relationship,
    Event,
}

impl EntryType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Object => "object",
            Self::Evidence => "evidence",
            Self::Relationship => "relationship",
            Self::Event => "event",
        }
    }
}

/// One row in the ledger's append-only entry log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub id: String,
    pub entry_type: EntryType,
    pub payload: serde_json::Value,
    pub written_at: DateTime<Utc>,
}

/// The append-only semantic knowledge ledger.
///
/// Backed by SQLite in v0.x. No code outside this crate touches SQLite directly.
pub struct Ledger {
    conn: Connection,
}

impl Ledger {
    /// Open (or create) the ledger database at the given path.
    pub fn open(path: &Path) -> Result<Self, LedgerError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        let ledger = Self { conn };
        ledger.init_schema()?;
        Ok(ledger)
    }

    fn init_schema(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS entries (
                id          TEXT NOT NULL,
                entry_type  TEXT NOT NULL,
                payload     TEXT NOT NULL,
                written_at  TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_entries_id ON entries(id);

            CREATE TABLE IF NOT EXISTS current_objects (
                object_id   TEXT PRIMARY KEY,
                entry_id    TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS object_fts
                USING fts5(object_id UNINDEXED, name, kind);

            CREATE TABLE IF NOT EXISTS current_relationships (
                rel_id      TEXT PRIMARY KEY,
                from_id     TEXT NOT NULL,
                to_id       TEXT NOT NULL,
                kind        TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_rel_from ON current_relationships(from_id);
            CREATE INDEX IF NOT EXISTS idx_rel_to   ON current_relationships(to_id);
            ",
        )
    }

    // ── Append methods ────────────────────────────────────────────────────────

    /// Append an entry. Idempotent — if `entry.id` already exists, skip.
    pub fn append(&self, entry: &LedgerEntry) -> Result<bool, LedgerError> {
        let exists: bool = self.conn.query_row(
            "SELECT 1 FROM entries WHERE id = ?1",
            params![entry.id],
            |_| Ok(true),
        ).unwrap_or(false);

        if exists {
            return Ok(false);
        }

        let payload = serde_json::to_string(&entry.payload)?;
        self.conn.execute(
            "INSERT INTO entries (id, entry_type, payload, written_at) VALUES (?1, ?2, ?3, ?4)",
            params![entry.id, entry.entry_type.as_str(), payload, entry.written_at.to_rfc3339()],
        )?;
        Ok(true)
    }

    /// Write a KirObject. Updates the current-state index and FTS.
    /// Returns `true` if written, `false` if already present (idempotent).
    pub fn append_object(&self, obj: &KirObject) -> Result<bool, LedgerError> {
        let entry = LedgerEntry {
            id: obj.id.to_string(),
            entry_type: EntryType::Object,
            payload: serde_json::to_value(obj)?,
            written_at: Utc::now(),
        };
        let is_new = self.append(&entry)?;

        if is_new {
            self.conn.execute(
                "INSERT OR REPLACE INTO current_objects (object_id, entry_id) VALUES (?1, ?2)",
                params![obj.id.to_string(), entry.id],
            )?;
            self.conn.execute(
                "INSERT OR REPLACE INTO object_fts (object_id, name, kind) VALUES (?1, ?2, ?3)",
                params![obj.id.to_string(), obj.name, obj.kind.to_string()],
            )?;
        }

        Ok(is_new)
    }

    /// Write a KirEvidence. Idempotent.
    pub fn append_evidence(&self, ev: &KirEvidence) -> Result<(), LedgerError> {
        let entry = LedgerEntry {
            id: ev.id.to_string(),
            entry_type: EntryType::Evidence,
            payload: serde_json::to_value(ev)?,
            written_at: Utc::now(),
        };
        self.append(&entry)?;
        Ok(())
    }

    /// Write a KirRelationship. Updates the relationship index.
    /// Returns `true` if written, `false` if already present (idempotent).
    pub fn append_relationship(&self, rel: &KirRelationship) -> Result<bool, LedgerError> {
        let entry = LedgerEntry {
            id: rel.id.to_string(),
            entry_type: EntryType::Relationship,
            payload: serde_json::to_value(rel)?,
            written_at: Utc::now(),
        };
        let is_new = self.append(&entry)?;

        if is_new {
            self.conn.execute(
                "INSERT OR REPLACE INTO current_relationships \
                 (rel_id, from_id, to_id, kind) VALUES (?1, ?2, ?3, ?4)",
                params![
                    rel.id.to_string(),
                    rel.from.to_string(),
                    rel.to.to_string(),
                    format!("{:?}", rel.kind),
                ],
            )?;
        }

        Ok(is_new)
    }

    // ── Read methods — current state ──────────────────────────────────────────

    /// Retrieve the current state of a KirObject by id.
    pub fn get_object(&self, id: &KirId) -> Result<Option<KirObject>, LedgerError> {
        let row = self.conn.query_row(
            "SELECT e.payload FROM entries e
             JOIN current_objects c ON c.entry_id = e.id
             WHERE c.object_id = ?1",
            params![id.to_string()],
            |row| row.get::<_, String>(0),
        );

        match row {
            Ok(payload) => Ok(Some(serde_json::from_str(&payload)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// All objects currently in the ledger's current-state index.
    pub fn all_objects(&self) -> Result<Vec<KirObject>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT e.payload FROM entries e JOIN current_objects c ON c.entry_id = e.id",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

        let mut objects = Vec::new();
        for row in rows {
            objects.push(serde_json::from_str::<KirObject>(&row?)?);
        }
        Ok(objects)
    }

    /// All relationships currently in the ledger's current-state index.
    pub fn all_relationships(&self) -> Result<Vec<KirRelationship>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT e.payload FROM entries e JOIN current_relationships c ON c.rel_id = e.id",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

        let mut rels = Vec::new();
        for row in rows {
            rels.push(serde_json::from_str::<KirRelationship>(&row?)?);
        }
        Ok(rels)
    }

    /// Retrieve a KirEvidence by id.
    pub fn get_evidence(&self, id: &KirId) -> Result<Option<KirEvidence>, LedgerError> {
        let row = self.conn.query_row(
            "SELECT payload FROM entries WHERE id = ?1 AND entry_type = 'evidence'",
            params![id.to_string()],
            |row| row.get::<_, String>(0),
        );

        match row {
            Ok(payload) => Ok(Some(serde_json::from_str(&payload)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Retrieve a KirRelationship by id.
    pub fn get_relationship(&self, id: &KirId) -> Result<Option<KirRelationship>, LedgerError> {
        let row = self.conn.query_row(
            "SELECT payload FROM entries WHERE id = ?1 AND entry_type = 'relationship'",
            params![id.to_string()],
            |row| row.get::<_, String>(0),
        );

        match row {
            Ok(payload) => Ok(Some(serde_json::from_str(&payload)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// All relationships where `from` or `to` equals `id`.
    pub fn relationships_for(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        let id_str = id.to_string();
        let mut stmt = self.conn.prepare(
            "SELECT e.payload FROM entries e
             JOIN current_relationships cr ON cr.rel_id = e.id
             WHERE cr.from_id = ?1 OR cr.to_id = ?1",
        )?;
        let rows = stmt.query_map(params![id_str], |row| row.get::<_, String>(0))?;

        let mut rels = Vec::new();
        for row in rows {
            let payload = row?;
            rels.push(serde_json::from_str::<KirRelationship>(&payload)?);
        }
        Ok(rels)
    }

    // ── Read methods — historical state ───────────────────────────────────────

    /// Retrieve the object as it was at or before `at`. Returns `None` if it had
    /// not yet been committed by that point in time.
    pub fn object_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Option<KirObject>, LedgerError> {
        let row = self.conn.query_row(
            "SELECT e.payload FROM entries e
             JOIN current_objects c ON c.entry_id = e.id
             WHERE c.object_id = ?1 AND e.written_at <= ?2",
            params![id.to_string(), at.to_rfc3339()],
            |row| row.get::<_, String>(0),
        );

        match row {
            Ok(payload) => Ok(Some(serde_json::from_str(&payload)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// All relationships involving `id` that were committed at or before `at`.
    pub fn relationships_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, LedgerError> {
        let id_str = id.to_string();
        let at_str = at.to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT e.payload FROM entries e
             JOIN current_relationships cr ON cr.rel_id = e.id
             WHERE (cr.from_id = ?1 OR cr.to_id = ?1)
             AND e.written_at <= ?2",
        )?;
        let rows = stmt.query_map(params![id_str, at_str], |row| row.get::<_, String>(0))?;

        let mut rels = Vec::new();
        for row in rows {
            let payload = row?;
            rels.push(serde_json::from_str::<KirRelationship>(&payload)?);
        }
        Ok(rels)
    }

    // ── Full-text search ──────────────────────────────────────────────────────

    /// Full-text search over object names and kinds.
    ///
    /// The query is matched as-is when it looks like a simple FTS5 term (e.g. a
    /// prefix query like `order*`), but any query containing characters FTS5
    /// treats as query-syntax operators (`-`, `:`, `"`, etc.) is escaped into a
    /// literal phrase so callers can search for arbitrary text without hitting
    /// FTS5 syntax errors.
    pub fn find_objects(&self, query: &str) -> Result<Vec<(KirId, String)>, LedgerError> {
        let is_simple_term = query
            .chars()
            .all(|c| c.is_alphanumeric() || c == '*' || c == ' ');
        let match_expr = if is_simple_term {
            query.to_string()
        } else {
            format!("\"{}\"", query.replace('"', "\"\""))
        };

        let mut stmt = self.conn.prepare(
            "SELECT object_id, name FROM object_fts WHERE object_fts MATCH ?1 LIMIT 20",
        )?;
        let rows = stmt.query_map(params![match_expr], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (id_str, name) = row?;
            if let Ok(id) = id_str.parse::<KirId>() {
                results.push((id, name));
            }
        }
        Ok(results)
    }

    // ── Counters ──────────────────────────────────────────────────────────────

    /// Total number of entries in the ledger.
    pub fn entry_count(&self) -> Result<usize, LedgerError> {
        let n: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    /// Number of distinct objects in the current-state index.
    pub fn object_count(&self) -> Result<usize, LedgerError> {
        let n: i64 =
            self.conn.query_row("SELECT COUNT(*) FROM current_objects", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    /// Number of distinct relationships in the current-state index.
    pub fn relationship_count(&self) -> Result<usize, LedgerError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM current_relationships", [], |row| row.get(0))?;
        Ok(n as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation};
    use std::time::Duration;
    use tempfile::tempdir;

    fn temp_ledger() -> (Ledger, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.db");
        (Ledger::open(&path).unwrap(), dir)
    }

    #[test]
    fn append_and_retrieve_object() {
        let (ledger, _dir) = temp_ledger();
        let obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        ledger.append_object(&obj).unwrap();
        let found = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(found.name, "orders");
    }

    #[test]
    fn all_objects_returns_every_object() {
        let (ledger, _dir) = temp_ledger();
        ledger.append_object(&KirObject::new("orders", ObjectKind::Table)).unwrap();
        ledger.append_object(&KirObject::new("customers", ObjectKind::Table)).unwrap();
        let all = ledger.all_objects().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn all_relationships_returns_every_relationship() {
        let (ledger, _dir) = temp_ledger();
        ledger.append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, KirId::new(), KirId::new())).unwrap();
        ledger.append_relationship(&KirRelationship::new(RelationshipKind::Calls, KirId::new(), KirId::new())).unwrap();
        let all = ledger.all_relationships().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn append_is_idempotent() {
        let (ledger, _dir) = temp_ledger();
        let obj = KirObject::new("customers", ObjectKind::Table);
        ledger.append_object(&obj).unwrap();
        ledger.append_object(&obj).unwrap();
        assert_eq!(ledger.entry_count().unwrap(), 1);
    }

    #[test]
    fn get_unknown_object_returns_none() {
        let (ledger, _dir) = temp_ledger();
        assert!(ledger.get_object(&KirId::new()).unwrap().is_none());
    }

    #[test]
    fn append_and_retrieve_evidence() {
        let (ledger, _dir) = temp_ledger();
        let ev = KirEvidence::new(SourceLocation::at("schema.sql", 10), "CREATE TABLE orders");
        let id = ev.id;
        ledger.append_evidence(&ev).unwrap();
        let found = ledger.get_evidence(&id).unwrap().unwrap();
        assert_eq!(found.fragment, "CREATE TABLE orders");
    }

    #[test]
    fn fts_find_objects() {
        let (ledger, _dir) = temp_ledger();
        ledger.append_object(&KirObject::new("order_items", ObjectKind::Table)).unwrap();
        ledger.append_object(&KirObject::new("customers", ObjectKind::Table)).unwrap();
        let results = ledger.find_objects("order*").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "order_items");
    }

    #[test]
    fn fts_find_objects_no_match_with_special_chars_returns_empty() {
        let (ledger, _dir) = temp_ledger();
        ledger.append_object(&KirObject::new("customers", ObjectKind::Table)).unwrap();
        // Hyphens are FTS5 query-syntax operators; must not error, just find nothing.
        assert!(ledger.find_objects("zzz-nonexistent").unwrap().is_empty());
    }

    #[test]
    fn append_and_retrieve_relationship() {
        let (ledger, _dir) = temp_ledger();
        let a = KirId::new();
        let b = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, a, b);
        let rel_id = rel.id;
        ledger.append_relationship(&rel).unwrap();
        let found = ledger.get_relationship(&rel_id).unwrap().unwrap();
        assert_eq!(found.from, a);
        assert_eq!(found.to, b);
    }

    #[test]
    fn relationship_is_idempotent() {
        let (ledger, _dir) = temp_ledger();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, KirId::new(), KirId::new());
        assert!(ledger.append_relationship(&rel).unwrap());
        assert!(!ledger.append_relationship(&rel).unwrap());
        assert_eq!(ledger.relationship_count().unwrap(), 1);
    }

    #[test]
    fn relationships_for_returns_both_directions() {
        let (ledger, _dir) = temp_ledger();
        let a = KirId::new();
        let b = KirId::new();
        let c = KirId::new();
        // a→b and c→a
        ledger.append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, a, b)).unwrap();
        ledger.append_relationship(&KirRelationship::new(RelationshipKind::Calls, c, a)).unwrap();

        let rels = ledger.relationships_for(&a).unwrap();
        assert_eq!(rels.len(), 2, "a participates in both relationships");
    }

    #[test]
    fn object_at_before_written_returns_none() {
        let (ledger, _dir) = temp_ledger();
        let obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        let before = Utc::now() - chrono::Duration::seconds(60);
        ledger.append_object(&obj).unwrap();
        // Query before the object was written
        assert!(ledger.object_at(&id, before).unwrap().is_none());
    }

    #[test]
    fn object_at_after_written_returns_object() {
        let (ledger, _dir) = temp_ledger();
        let obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        ledger.append_object(&obj).unwrap();
        let after = Utc::now() + chrono::Duration::seconds(60);
        assert!(ledger.object_at(&id, after).unwrap().is_some());
    }

    #[test]
    fn relationships_at_filters_by_time() {
        let (ledger, _dir) = temp_ledger();
        let a = KirId::new();
        let b = KirId::new();
        let before = Utc::now() - chrono::Duration::seconds(60);
        ledger.append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, a, b)).unwrap();
        // Query before write: nothing
        assert!(ledger.relationships_at(&a, before).unwrap().is_empty());
        // Query after write: one
        let after = Utc::now() + chrono::Duration::seconds(60);
        assert_eq!(ledger.relationships_at(&a, after).unwrap().len(), 1);
    }
}
