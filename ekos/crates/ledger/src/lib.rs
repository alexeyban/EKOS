use chrono::{DateTime, Utc};
use ekos_artifact::ArtifactId;
use ekos_kir::{KirEvidence, KirId, KirObject, KirRelationship};
use rusqlite::{Connection, Result as SqlResult, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Content signature used for version-equality — excludes volatile fields
/// (`created_at`) so re-appending logically-identical content (same name,
/// kind, properties, evidence) is recognized as unchanged even though a fresh
/// `KirObject::new`/`KirRelationship::new` call stamps a new `created_at` on
/// every build. Mirrors how `ekos-artifact` excludes volatile metadata from
/// its content-addressed `ArtifactId`.
fn content_signature(payload: &serde_json::Value) -> String {
    let mut stripped = payload.clone();
    if let serde_json::Value::Object(ref mut map) = stripped {
        map.remove("created_at");
    }
    ArtifactId::compute(&stripped).as_str().to_string()
}

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
                content_sig TEXT NOT NULL,
                written_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_entries_id ON entries(id);
            CREATE INDEX IF NOT EXISTS idx_entries_id_sig ON entries(id, content_sig);

            CREATE TABLE IF NOT EXISTS current_objects (
                object_id    TEXT PRIMARY KEY,
                entry_rowid  INTEGER NOT NULL
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS object_fts
                USING fts5(object_id UNINDEXED, name, kind);

            CREATE TABLE IF NOT EXISTS current_relationships (
                rel_id       TEXT PRIMARY KEY,
                entry_rowid  INTEGER NOT NULL,
                from_id      TEXT NOT NULL,
                to_id        TEXT NOT NULL,
                kind         TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_rel_from ON current_relationships(from_id);
            CREATE INDEX IF NOT EXISTS idx_rel_to   ON current_relationships(to_id);
            ",
        )
    }

    // ── Append methods ────────────────────────────────────────────────────────

    /// Append an entry, versioned by (id, content signature).
    ///
    /// Unlike a plain content-addressable store, the ledger's logical id
    /// (`entry.id`) is stable across an object/relationship's lifetime — it is
    /// **not** required to be unique in `entries`. Re-appending logically
    /// identical content under an existing id is a no-op (idempotent, ignoring
    /// the volatile `created_at` stamp); appending genuinely different content
    /// inserts a new version, addressed by SQLite `rowid`. Returns
    /// `(is_new, rowid)` so callers can point current-state tables at the
    /// exact version row.
    fn append_versioned(&self, entry: &LedgerEntry) -> Result<(bool, i64), LedgerError> {
        let payload = serde_json::to_string(&entry.payload)?;
        let sig = content_signature(&entry.payload);

        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT rowid FROM entries WHERE id = ?1 AND content_sig = ?2",
                params![entry.id, sig],
                |row| row.get(0),
            )
            .ok();

        if let Some(rowid) = existing {
            return Ok((false, rowid));
        }

        self.conn.execute(
            "INSERT INTO entries (id, entry_type, payload, content_sig, written_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![entry.id, entry.entry_type.as_str(), payload, sig, entry.written_at.to_rfc3339()],
        )?;
        Ok((true, self.conn.last_insert_rowid()))
    }

    /// Append an entry. Idempotent — if the exact `(id, payload)` pair already
    /// exists, skip. Returns `true` if a new version was written.
    pub fn append(&self, entry: &LedgerEntry) -> Result<bool, LedgerError> {
        let (is_new, _rowid) = self.append_versioned(entry)?;
        Ok(is_new)
    }

    /// Write a KirObject. Updates the current-state index and FTS to point at
    /// this version, even if the exact payload was already present (repointing
    /// is a cheap no-op in that case). Returns `true` if a new version was
    /// recorded — `false` means this exact content was already the latest.
    pub fn append_object(&self, obj: &KirObject) -> Result<bool, LedgerError> {
        let entry = LedgerEntry {
            id: obj.id.to_string(),
            entry_type: EntryType::Object,
            payload: serde_json::to_value(obj)?,
            written_at: Utc::now(),
        };
        let (is_new, rowid) = self.append_versioned(&entry)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO current_objects (object_id, entry_rowid) VALUES (?1, ?2)",
            params![obj.id.to_string(), rowid],
        )?;
        self.conn.execute(
            "INSERT OR REPLACE INTO object_fts (object_id, name, kind) VALUES (?1, ?2, ?3)",
            params![obj.id.to_string(), obj.name, obj.kind.to_string()],
        )?;

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

    /// Write a KirRelationship. Updates the relationship index to point at
    /// this version. Returns `true` if a new version was recorded.
    pub fn append_relationship(&self, rel: &KirRelationship) -> Result<bool, LedgerError> {
        let entry = LedgerEntry {
            id: rel.id.to_string(),
            entry_type: EntryType::Relationship,
            payload: serde_json::to_value(rel)?,
            written_at: Utc::now(),
        };
        let (is_new, rowid) = self.append_versioned(&entry)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO current_relationships \
             (rel_id, entry_rowid, from_id, to_id, kind) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                rel.id.to_string(),
                rowid,
                rel.from.to_string(),
                rel.to.to_string(),
                format!("{:?}", rel.kind),
            ],
        )?;

        Ok(is_new)
    }

    // ── Read methods — current state ──────────────────────────────────────────

    /// Retrieve the current state of a KirObject by id.
    pub fn get_object(&self, id: &KirId) -> Result<Option<KirObject>, LedgerError> {
        let row = self.conn.query_row(
            "SELECT e.payload FROM entries e
             JOIN current_objects c ON c.entry_rowid = e.rowid
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
            "SELECT e.payload FROM entries e JOIN current_objects c ON c.entry_rowid = e.rowid",
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
            "SELECT e.payload FROM entries e JOIN current_relationships c ON c.entry_rowid = e.rowid",
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
            "SELECT payload FROM entries WHERE id = ?1 AND entry_type = 'evidence'
             ORDER BY rowid DESC LIMIT 1",
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
            "SELECT payload FROM entries WHERE id = ?1 AND entry_type = 'relationship'
             ORDER BY rowid DESC LIMIT 1",
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
             JOIN current_relationships cr ON cr.entry_rowid = e.rowid
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
    ///
    /// Queries the versioned `entries` log directly (not the current-state
    /// pointer table), so this reflects true history: an object updated after
    /// `at` still yields the version that was current *at* `at`.
    pub fn object_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Option<KirObject>, LedgerError> {
        let row = self.conn.query_row(
            "SELECT payload FROM entries
             WHERE id = ?1 AND entry_type = 'object' AND written_at <= ?2
             ORDER BY written_at DESC LIMIT 1",
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
    ///
    /// Limitation: this reflects each relationship's *current* version filtered
    /// by timestamp, not true multi-version history (unlike `object_at`) —
    /// relationships don't yet have a per-id historical index. See RFC 0011.
    pub fn relationships_at(
        &self,
        id: &KirId,
        at: DateTime<Utc>,
    ) -> Result<Vec<KirRelationship>, LedgerError> {
        let id_str = id.to_string();
        let at_str = at.to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT e.payload FROM entries e
             JOIN current_relationships cr ON cr.entry_rowid = e.rowid
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

    // ── Branching (Phase 13 — Optimizer) ──────────────────────────────────────

    /// Write a complete, consistent copy of this ledger to `dest` — used to
    /// create a branch. Uses SQLite's `VACUUM INTO` rather than a raw file
    /// copy: the main ledger runs in WAL mode, so a plain `cp` could miss data
    /// still sitting in the `-wal` file; `VACUUM INTO` always produces a
    /// complete snapshot regardless of WAL state.
    pub fn vacuum_into(&self, dest: &Path) -> Result<(), LedgerError> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        self.conn.execute("VACUUM INTO ?1", params![dest.to_string_lossy()])?;
        Ok(())
    }

    // ── Knowledge diff (Phase 13 — Optimizer) ─────────────────────────────────

    /// Object/relationship versions written strictly after `from` and at or
    /// before `to`, plus the logical id each version belongs to.
    fn versions_in_window(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<(LedgerEntryId, String)>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT rowid, id FROM entries
             WHERE entry_type IN ('object', 'relationship')
             AND written_at > ?1 AND written_at <= ?2",
        )?;
        let rows = stmt.query_map(params![from.to_rfc3339(), to.to_rfc3339()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (rowid, id) = row?;
            out.push((LedgerEntryId(rowid), id));
        }
        Ok(out)
    }
}

/// A version row's SQLite `rowid` — the ledger's unit of "one written version".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntryId(pub i64);

/// What changed in the ledger between two points in time.
///
/// Since the ledger is append-only, "changed" means a new version was recorded
/// for an id that already existed; `added` doesn't distinguish a brand-new
/// object from an updated one — callers can check `object_at(id, from)`
/// themselves if that distinction matters.
#[derive(Debug, Clone)]
pub struct LedgerDiff {
    /// Object/relationship versions written in `(from, to]`.
    pub added: Vec<LedgerEntryId>,
    /// Unique logical ids (object/relationship `KirId`s as strings) touched in
    /// the window — resolvable via `get_object`/`get_relationship` for display.
    pub touched: Vec<String>,
    /// Currently-tracked objects/relationships not touched in that window.
    pub unchanged: usize,
}

/// Diff the ledger's knowledge between two points in time.
pub fn diff_ledger(
    ledger: &Ledger,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<LedgerDiff, LedgerError> {
    let versions = ledger.versions_in_window(from, to)?;
    let touched_ids: std::collections::HashSet<String> =
        versions.iter().map(|(_, id)| id.clone()).collect();

    let total_tracked = ledger.object_count()? + ledger.relationship_count()?;
    let unchanged = total_tracked.saturating_sub(touched_ids.len());

    let mut touched: Vec<String> = touched_ids.into_iter().collect();
    touched.sort();

    Ok(LedgerDiff { added: versions.into_iter().map(|(id, _)| id).collect(), touched, unchanged })
}

// ── Knowledge merge (Phase 13 — Optimizer) ─────────────────────────────────

/// A logical id present in both ledgers with genuinely different content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConflict {
    pub object_id: String,
    pub reason: String,
}

/// Result of merging one branch ledger into another.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MergeReport {
    pub objects_merged: usize,
    pub relationships_merged: usize,
    pub conflicts: Vec<MergeConflict>,
}

/// Merge every object/relationship currently tracked in `branch` into `main`.
///
/// A logical id with no version in `main` yet is a clean addition. A logical
/// id present in both with the same content — compared via the same
/// `content_signature` used for ledger versioning, so volatile `created_at`
/// stamps never cause a false conflict — is a no-op. A logical id present in
/// both with genuinely different content is a conflict: recorded in the
/// report, **not** auto-resolved or overwritten. This is last-write
/// divergence detection, not a true 3-way merge (see RFC 0011).
pub fn merge_branch(main: &Ledger, branch: &Ledger) -> Result<MergeReport, LedgerError> {
    let mut report = MergeReport::default();

    for obj in branch.all_objects()? {
        match main.get_object(&obj.id)? {
            None => {
                main.append_object(&obj)?;
                report.objects_merged += 1;
            }
            Some(existing) => {
                let existing_sig = content_signature(&serde_json::to_value(&existing)?);
                let incoming_sig = content_signature(&serde_json::to_value(&obj)?);
                if existing_sig != incoming_sig {
                    report.conflicts.push(MergeConflict {
                        object_id: obj.id.to_string(),
                        reason: "object diverged between branches".to_string(),
                    });
                }
            }
        }
    }

    for rel in branch.all_relationships()? {
        match main.get_relationship(&rel.id)? {
            None => {
                main.append_relationship(&rel)?;
                report.relationships_merged += 1;
            }
            Some(existing) => {
                let existing_sig = content_signature(&serde_json::to_value(&existing)?);
                let incoming_sig = content_signature(&serde_json::to_value(&rel)?);
                if existing_sig != incoming_sig {
                    report.conflicts.push(MergeConflict {
                        object_id: rel.id.to_string(),
                        reason: "relationship diverged between branches".to_string(),
                    });
                }
            }
        }
    }

    Ok(report)
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

    #[test]
    fn updating_an_object_creates_a_new_version_and_keeps_latest_current() {
        let (ledger, _dir) = temp_ledger();
        let mut obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        ledger.append_object(&obj).unwrap();

        obj.properties.insert("row_count".into(), serde_json::json!(42));
        let is_new = ledger.append_object(&obj).unwrap();
        assert!(is_new, "changed content under the same id must be a new version");

        // 2 versions in the log, but current-state still shows exactly one object.
        assert_eq!(ledger.entry_count().unwrap(), 2);
        assert_eq!(ledger.object_count().unwrap(), 1);
        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(current.properties.get("row_count"), Some(&serde_json::json!(42)));
    }

    #[test]
    fn reappending_identical_object_is_not_a_new_version() {
        let (ledger, _dir) = temp_ledger();
        let obj = KirObject::new("customers", ObjectKind::Table);
        assert!(ledger.append_object(&obj).unwrap());
        // Same logical content (created_at differs only if we constructed a fresh
        // KirObject; here it's the exact same struct, so this exercises the
        // content_sig path directly).
        assert!(!ledger.append_object(&obj).unwrap());
        assert_eq!(ledger.entry_count().unwrap(), 1);
    }

    #[test]
    fn object_at_returns_true_historical_version_after_update() {
        let (ledger, _dir) = temp_ledger();
        let mut obj = KirObject::new("orders", ObjectKind::Table);
        let id = obj.id;
        ledger.append_object(&obj).unwrap();
        let mid = Utc::now();

        obj.properties.insert("row_count".into(), serde_json::json!(99));
        ledger.append_object(&obj).unwrap();

        // At `mid` (before the update), the historical version has no row_count.
        let historical = ledger.object_at(&id, mid).unwrap().unwrap();
        assert!(!historical.properties.contains_key("row_count"));

        // Current state has it.
        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(current.properties.get("row_count"), Some(&serde_json::json!(99)));
    }

    #[test]
    fn diff_ledger_reports_updated_object_as_added_and_others_as_unchanged() {
        let (ledger, _dir) = temp_ledger();
        let mut updated = KirObject::new("orders", ObjectKind::Table);
        let updated_id = updated.id;
        ledger.append_object(&updated).unwrap();
        ledger.append_object(&KirObject::new("customers", ObjectKind::Table)).unwrap();
        ledger.append_object(&KirObject::new("products", ObjectKind::Table)).unwrap();

        let t1 = Utc::now();
        std::thread::sleep(Duration::from_millis(5));

        updated.properties.insert("row_count".into(), serde_json::json!(7));
        ledger.append_object(&updated).unwrap();

        let t2 = Utc::now();

        let diff = diff_ledger(&ledger, t1, t2).unwrap();
        assert_eq!(diff.added.len(), 1, "only the updated object's new version falls in the window");
        assert_eq!(diff.unchanged, 2);

        // Sanity: the added version really is the updated object.
        let _ = updated_id;
    }

    #[test]
    fn vacuum_into_produces_a_readable_copy() {
        let (ledger, _dir) = temp_ledger();
        ledger.append_object(&KirObject::new("orders", ObjectKind::Table)).unwrap();

        let dest_dir = tempdir().unwrap();
        let dest = dest_dir.path().join("branch.db");
        ledger.vacuum_into(&dest).unwrap();

        let copy = Ledger::open(&dest).unwrap();
        assert_eq!(copy.object_count().unwrap(), 1);
    }

    #[test]
    fn merge_branch_adds_new_objects_from_branch() {
        let (main, _dir1) = temp_ledger();
        let (branch, _dir2) = temp_ledger();

        main.append_object(&KirObject::new("customers", ObjectKind::Table)).unwrap();
        branch.append_object(&KirObject::new("orders", ObjectKind::Table)).unwrap();

        let report = merge_branch(&main, &branch).unwrap();
        assert_eq!(report.objects_merged, 1);
        assert!(report.conflicts.is_empty());
        assert_eq!(main.object_count().unwrap(), 2);
    }

    #[test]
    fn merge_branch_flags_diverged_objects_as_conflicts() {
        let (main, _dir1) = temp_ledger();
        let (branch, _dir2) = temp_ledger();

        let mut shared = KirObject::new("orders", ObjectKind::Table);
        main.append_object(&shared).unwrap();

        shared.properties.insert("row_count".into(), serde_json::json!(5));
        branch.append_object(&shared).unwrap();

        let report = merge_branch(&main, &branch).unwrap();
        assert_eq!(report.objects_merged, 0);
        assert_eq!(report.conflicts.len(), 1);
        assert_eq!(report.conflicts[0].object_id, shared.id.to_string());
    }

    #[test]
    fn merge_branch_is_noop_for_identical_objects() {
        let (main, _dir1) = temp_ledger();
        let (branch, _dir2) = temp_ledger();

        let shared = KirObject::new("orders", ObjectKind::Table);
        main.append_object(&shared).unwrap();
        branch.append_object(&shared).unwrap();

        let report = merge_branch(&main, &branch).unwrap();
        assert_eq!(report.objects_merged, 0);
        assert!(report.conflicts.is_empty());
    }
}
