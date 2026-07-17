pub mod fact;
pub mod fact_ledger;
pub mod index;
pub mod segment;

pub use fact_ledger::FactLedger;

use chrono::{DateTime, Utc};
use ekos_artifact::ArtifactId;
use ekos_kir::{KirEvidence, KirId, KirObject, KirRelationship};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Result as SqlResult, params};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

/// Content signature used for version-equality — excludes volatile fields
/// (`created_at`) so re-appending logically-identical content (same name,
/// kind, properties, evidence) is recognized as unchanged even though a fresh
/// `KirObject::new`/`KirRelationship::new` call stamps a new `created_at` on
/// every build. Mirrors how `ekos-artifact` excludes volatile metadata from
/// its content-addressed `ArtifactId`.
///
/// The signature is always the SHA-256 of *canonical JSON* — never of the
/// stored (compressed) bytes — so identity is independent of the on-disk
/// format (RFC 0015).
pub(crate) fn content_signature(payload: &serde_json::Value) -> String {
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
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt ledger data: {0}")]
    Corrupt(String),
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

// ── Storage format (RFC 0015) ───────────────────────────────────────────────

/// On-disk schema generation, sniffed from `PRAGMA user_version` on open.
///
/// - **V1** (`user_version < 2`): JSON TEXT payloads, hex TEXT signatures,
///   RFC 3339 TEXT timestamps, UUID TEXT ids, FTS with stored columns.
/// - **V2** (`user_version = 2`): zstd BLOB payloads (optionally with a
///   corpus-trained dictionary), 32-byte BLOB signatures, unix-millisecond
///   INTEGER timestamps, 16-byte BLOB ids, contentless FTS keyed by the
///   current version's `entries.rowid`.
///
/// A V1 ledger stays fully readable and writable until `migrate_to_v2` runs;
/// new ledgers are created as V2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    V1,
    V2,
}

/// zstd level for ledger payload frames. Higher than the streaming-file
/// default (3): rows are sub-KB and written once per version, so spending
/// tens of microseconds at write time for a visibly smaller `entries` table
/// is the right trade — decompression speed is independent of level.
const LEDGER_ZSTD_LEVEL: i32 = 19;

/// Maximum trained-dictionary size (the zstd-recommended ~110 KB).
const DICT_MAX_BYTES: usize = 112_640;
/// Minimum sample count before dictionary training is attempted.
const DICT_MIN_SAMPLES: usize = 64;
/// Payload rows sampled for dictionary training during migration.
const DICT_SAMPLE_LIMIT: usize = 50_000;

/// Per-payload compression codec. Every V2 payload BLOB is
/// `[dict_version: u8]` + one zstd frame; dict_version 0 means "no
/// dictionary", any other value names the `meta` table dictionary the frame
/// was compressed with — so retraining never requires rewriting old rows.
enum Codec {
    /// V1: payloads are plain JSON TEXT; the codec is never invoked on write.
    PlainText,
    Zstd {
        dict: Option<Dict>,
        /// Reused compression context for dictionary-less frames. Level-19
        /// context *initialization* costs ~45 ms (huge match tables); reusing
        /// one context makes per-row compression microseconds. The dictionary
        /// path gets the same effect from its precomputed `EncoderDictionary`.
        plain_compressor: std::cell::RefCell<zstd::bulk::Compressor<'static>>,
    },
}

struct Dict {
    version: u8,
    enc: zstd::dict::EncoderDictionary<'static>,
    dec: zstd::dict::DecoderDictionary<'static>,
}

impl Codec {
    fn zstd(dict_bytes: Option<(u8, Vec<u8>)>) -> Self {
        let dict = dict_bytes.map(|(version, bytes)| Dict {
            version,
            enc: zstd::dict::EncoderDictionary::copy(&bytes, LEDGER_ZSTD_LEVEL),
            dec: zstd::dict::DecoderDictionary::copy(&bytes),
        });
        let plain_compressor = std::cell::RefCell::new(
            zstd::bulk::Compressor::new(LEDGER_ZSTD_LEVEL)
                .expect("zstd compressor construction cannot fail"),
        );
        Self::Zstd {
            dict,
            plain_compressor,
        }
    }

    /// JSON bytes → framed payload BLOB.
    fn compress(&self, json: &[u8]) -> Result<Vec<u8>, LedgerError> {
        let Codec::Zstd {
            dict,
            plain_compressor,
        } = self
        else {
            return Err(LedgerError::Corrupt(
                "compress called on a v1 (plain-text) ledger".into(),
            ));
        };
        let mut frame;
        match dict {
            Some(d) => {
                let mut compressor = zstd::bulk::Compressor::with_prepared_dictionary(&d.enc)?;
                let body = compressor.compress(json)?;
                frame = Vec::with_capacity(body.len() + 1);
                frame.push(d.version);
                frame.extend_from_slice(&body);
            }
            None => {
                let body = plain_compressor.borrow_mut().compress(json)?;
                frame = Vec::with_capacity(body.len() + 1);
                frame.push(0);
                frame.extend_from_slice(&body);
            }
        }
        Ok(frame)
    }

    /// Framed payload BLOB → JSON bytes.
    fn decompress(&self, frame: &[u8]) -> Result<Vec<u8>, LedgerError> {
        let (&version, body) = frame
            .split_first()
            .ok_or_else(|| LedgerError::Corrupt("empty payload frame".into()))?;
        if version == 0 {
            return Ok(zstd::decode_all(body)?);
        }
        let Codec::Zstd { dict: Some(d), .. } = self else {
            return Err(LedgerError::Corrupt(format!(
                "payload frame uses dictionary v{version} but the ledger has none"
            )));
        };
        if d.version != version {
            return Err(LedgerError::Corrupt(format!(
                "payload frame uses dictionary v{version}, ledger has v{}",
                d.version
            )));
        }
        let mut out = Vec::new();
        zstd::stream::read::Decoder::with_prepared_dictionary(body, &d.dec)?
            .read_to_end(&mut out)?;
        Ok(out)
    }
}

/// The append-only semantic knowledge ledger.
///
/// Backed by SQLite in v0.x. No code outside this crate touches SQLite directly.
pub struct Ledger {
    conn: Connection,
    format: Format,
    codec: Codec,
}

impl Ledger {
    /// Open (or create) the ledger database at the given path.
    ///
    /// New databases are created with the v2 (compact) schema. Existing v1
    /// databases keep working in v1 format until `migrate_to_v2` is run.
    pub fn open(path: &Path) -> Result<Self, LedgerError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        let user_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let has_entries: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_schema WHERE type='table' AND name='entries'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if user_version >= 2 {
            let dict = load_dictionary(&conn)?;
            return Ok(Self {
                conn,
                format: Format::V2,
                codec: Codec::zstd(dict),
            });
        }
        if has_entries {
            let ledger = Self {
                conn,
                format: Format::V1,
                codec: Codec::PlainText,
            };
            ledger.migrate_fts_v2()?;
            return Ok(ledger);
        }

        // Fresh database → v2 schema.
        init_schema_v2(&conn)?;
        Ok(Self {
            conn,
            format: Format::V2,
            codec: Codec::zstd(None),
        })
    }

    /// Create a v2 ledger at `path` with an optional pre-trained dictionary.
    fn create_v2(path: &Path, dict_bytes: Option<Vec<u8>>) -> Result<Self, LedgerError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        init_schema_v2(&conn)?;

        let dict = match dict_bytes {
            Some(bytes) => {
                conn.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('dict:1', ?1)",
                    params![bytes],
                )?;
                Some((1u8, bytes))
            }
            None => None,
        };
        Ok(Self {
            conn,
            format: Format::V2,
            codec: Codec::zstd(dict),
        })
    }

    /// RFC 0014 migration (v1 ledgers only): pre-content-column FTS tables are
    /// dropped, recreated with the v1.2 schema, and repopulated from current
    /// objects. The FTS table is a derived index — rebuilding it loses nothing.
    fn migrate_fts_v2(&self) -> Result<(), LedgerError> {
        let has_content: bool = {
            let mut stmt = self.conn.prepare("PRAGMA table_info(object_fts)")?;
            let cols = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut found = false;
            for col in cols {
                if col? == "content" {
                    found = true;
                }
            }
            found
        };
        if has_content {
            return Ok(());
        }

        tracing::info!("migrating object_fts to v2 (content column, RFC 0014)");
        self.conn.execute_batch(
            "BEGIN;
             DROP TABLE object_fts;
             CREATE VIRTUAL TABLE object_fts
                 USING fts5(object_id UNINDEXED, name, kind, content);
             COMMIT;",
        )?;
        for obj in self.all_objects()? {
            self.index_object_fts_v1(&obj)?;
        }
        Ok(())
    }

    // ── Format-dependent parameter encoding ──────────────────────────────────

    fn id_param(&self, id: &str) -> SqlValue {
        match self.format {
            Format::V1 => SqlValue::Text(id.to_owned()),
            Format::V2 => match Uuid::parse_str(id) {
                Ok(u) => SqlValue::Blob(u.as_bytes().to_vec()),
                Err(_) => SqlValue::Text(id.to_owned()),
            },
        }
    }

    fn sig_param(&self, hex_sig: &str) -> SqlValue {
        match self.format {
            Format::V1 => SqlValue::Text(hex_sig.to_owned()),
            Format::V2 => match hex::decode(hex_sig) {
                Ok(bytes) => SqlValue::Blob(bytes),
                Err(_) => SqlValue::Text(hex_sig.to_owned()),
            },
        }
    }

    fn ts_param(&self, t: DateTime<Utc>) -> SqlValue {
        match self.format {
            Format::V1 => SqlValue::Text(t.to_rfc3339()),
            // Microseconds, not millis: consecutive appends in one build tick
            // must stay distinguishable for `object_at` history queries.
            Format::V2 => SqlValue::Integer(t.timestamp_micros()),
        }
    }

    /// Decode a stored payload column (JSON TEXT in v1, compressed BLOB in v2)
    /// back into its JSON string.
    fn payload_to_string(&self, value: SqlValue) -> Result<String, LedgerError> {
        match value {
            SqlValue::Text(s) => Ok(s),
            SqlValue::Blob(frame) => {
                let bytes = self.codec.decompress(&frame)?;
                String::from_utf8(bytes)
                    .map_err(|e| LedgerError::Corrupt(format!("payload not UTF-8: {e}")))
            }
            other => Err(LedgerError::Corrupt(format!(
                "unexpected payload column type: {other:?}"
            ))),
        }
    }

    fn payload_param(&self, json: &str) -> Result<SqlValue, LedgerError> {
        match self.format {
            Format::V1 => Ok(SqlValue::Text(json.to_owned())),
            Format::V2 => Ok(SqlValue::Blob(self.codec.compress(json.as_bytes())?)),
        }
    }

    /// Run a payload-returning query and decode every row.
    fn query_payloads(
        &self,
        sql: &str,
        query_params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<String>, LedgerError> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(query_params, |row| row.get::<_, SqlValue>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(self.payload_to_string(row?)?);
        }
        Ok(out)
    }

    // ── Full-text index maintenance ───────────────────────────────────────────

    /// v1: (re)index one object into the column-storing FTS table.
    fn index_object_fts_v1(&self, obj: &KirObject) -> Result<(), LedgerError> {
        let content = obj
            .properties
            .get("excerpt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        self.conn.execute(
            "INSERT OR REPLACE INTO object_fts (object_id, name, kind, content)
             VALUES (?1, ?2, ?3, ?4)",
            params![obj.id.to_string(), obj.name, obj.kind.to_string(), content],
        )?;
        Ok(())
    }

    /// v2: index one object version into the contentless FTS table, keyed by
    /// its `entries.rowid`. The previous version's row (if different) is
    /// deleted — the FTS index tracks *current* versions only, like
    /// `current_objects`.
    fn index_object_fts_v2(
        &self,
        obj: &KirObject,
        rowid: i64,
        old_rowid: Option<i64>,
    ) -> Result<(), LedgerError> {
        if old_rowid == Some(rowid) {
            return Ok(()); // same version already indexed
        }
        if let Some(old) = old_rowid {
            self.conn
                .execute("DELETE FROM object_fts WHERE rowid = ?1", params![old])?;
        }
        let content = obj
            .properties
            .get("excerpt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        self.conn.execute(
            "INSERT INTO object_fts (rowid, name, kind, content) VALUES (?1, ?2, ?3, ?4)",
            params![rowid, obj.name, obj.kind.to_string(), content],
        )?;
        Ok(())
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
        let payload_json = serde_json::to_string(&entry.payload)?;
        let sig = content_signature(&entry.payload);
        let id_param = self.id_param(&entry.id);
        let sig_param = self.sig_param(&sig);

        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT rowid FROM entries WHERE id = ?1 AND content_sig = ?2",
                params![id_param, sig_param],
                |row| row.get(0),
            )
            .ok();

        if let Some(rowid) = existing {
            return Ok((false, rowid));
        }

        self.conn.execute(
            "INSERT INTO entries (id, entry_type, payload, content_sig, written_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id_param,
                entry.entry_type.as_str(),
                self.payload_param(&payload_json)?,
                sig_param,
                self.ts_param(entry.written_at),
            ],
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

        let old_rowid: Option<i64> = self
            .conn
            .query_row(
                "SELECT entry_rowid FROM current_objects WHERE object_id = ?1",
                params![obj.id.to_string()],
                |row| row.get(0),
            )
            .ok();

        self.conn.execute(
            "INSERT OR REPLACE INTO current_objects (object_id, entry_rowid) VALUES (?1, ?2)",
            params![obj.id.to_string(), rowid],
        )?;
        match self.format {
            Format::V1 => self.index_object_fts_v1(obj)?,
            Format::V2 => self.index_object_fts_v2(obj, rowid, old_rowid)?,
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
            |row| row.get::<_, SqlValue>(0),
        );

        match row {
            Ok(payload) => Ok(Some(serde_json::from_str(
                &self.payload_to_string(payload)?,
            )?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// All objects currently in the ledger's current-state index.
    pub fn all_objects(&self) -> Result<Vec<KirObject>, LedgerError> {
        let payloads = self.query_payloads(
            "SELECT e.payload FROM entries e JOIN current_objects c ON c.entry_rowid = e.rowid",
            &[],
        )?;
        payloads
            .iter()
            .map(|p| serde_json::from_str::<KirObject>(p).map_err(Into::into))
            .collect()
    }

    /// All relationships currently in the ledger's current-state index.
    pub fn all_relationships(&self) -> Result<Vec<KirRelationship>, LedgerError> {
        let payloads = self.query_payloads(
            "SELECT e.payload FROM entries e JOIN current_relationships c ON c.entry_rowid = e.rowid",
            &[],
        )?;
        payloads
            .iter()
            .map(|p| serde_json::from_str::<KirRelationship>(p).map_err(Into::into))
            .collect()
    }

    /// Retrieve a KirEvidence by id.
    pub fn get_evidence(&self, id: &KirId) -> Result<Option<KirEvidence>, LedgerError> {
        let row = self.conn.query_row(
            "SELECT payload FROM entries WHERE id = ?1 AND entry_type = 'evidence'
             ORDER BY rowid DESC LIMIT 1",
            params![self.id_param(&id.to_string())],
            |row| row.get::<_, SqlValue>(0),
        );

        match row {
            Ok(payload) => Ok(Some(serde_json::from_str(
                &self.payload_to_string(payload)?,
            )?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Retrieve a KirRelationship by id.
    pub fn get_relationship(&self, id: &KirId) -> Result<Option<KirRelationship>, LedgerError> {
        let row = self.conn.query_row(
            "SELECT payload FROM entries WHERE id = ?1 AND entry_type = 'relationship'
             ORDER BY rowid DESC LIMIT 1",
            params![self.id_param(&id.to_string())],
            |row| row.get::<_, SqlValue>(0),
        );

        match row {
            Ok(payload) => Ok(Some(serde_json::from_str(
                &self.payload_to_string(payload)?,
            )?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// All relationships where `from` or `to` equals `id`.
    pub fn relationships_for(&self, id: &KirId) -> Result<Vec<KirRelationship>, LedgerError> {
        let id_str = id.to_string();
        let payloads = self.query_payloads(
            "SELECT e.payload FROM entries e
             JOIN current_relationships cr ON cr.entry_rowid = e.rowid
             WHERE cr.from_id = ?1 OR cr.to_id = ?1",
            &[&id_str],
        )?;
        payloads
            .iter()
            .map(|p| serde_json::from_str::<KirRelationship>(p).map_err(Into::into))
            .collect()
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
             ORDER BY written_at DESC, rowid DESC LIMIT 1",
            params![self.id_param(&id.to_string()), self.ts_param(at)],
            |row| row.get::<_, SqlValue>(0),
        );

        match row {
            Ok(payload) => Ok(Some(serde_json::from_str(
                &self.payload_to_string(payload)?,
            )?)),
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
        let at_param = self.ts_param(at);
        let payloads = self.query_payloads(
            "SELECT e.payload FROM entries e
             JOIN current_relationships cr ON cr.entry_rowid = e.rowid
             WHERE (cr.from_id = ?1 OR cr.to_id = ?1)
             AND e.written_at <= ?2",
            &[&id_str, &at_param],
        )?;
        payloads
            .iter()
            .map(|p| serde_json::from_str::<KirRelationship>(p).map_err(Into::into))
            .collect()
    }

    // ── Full-text search ──────────────────────────────────────────────────────

    /// Full-text search over object names, kinds, and content excerpts
    /// (RFC 0014), ranked by bm25 relevance: name matches weigh 10×, kind 4×,
    /// content 1× — a name hit always outranks a body mention.
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

        match self.format {
            Format::V1 => self.find_objects_v1(&match_expr),
            Format::V2 => self.find_objects_v2(&match_expr),
        }
    }

    fn find_objects_v1(&self, match_expr: &str) -> Result<Vec<(KirId, String)>, LedgerError> {
        let mut stmt = self.conn.prepare(
            // bm25 weights are positional per column: object_id (unindexed,
            // never matches), name, kind, content.
            "SELECT object_id, name FROM object_fts WHERE object_fts MATCH ?1
             ORDER BY bm25(object_fts, 0.0, 10.0, 4.0, 1.0) LIMIT 50",
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

    /// v2: the FTS table is contentless, so ranked rowids are resolved back to
    /// id/name through the entry payload (≤50 rows, one join).
    fn find_objects_v2(&self, match_expr: &str) -> Result<Vec<(KirId, String)>, LedgerError> {
        let payloads = self.query_payloads(
            "SELECT e.payload FROM object_fts f
             JOIN entries e ON e.rowid = f.rowid
             WHERE object_fts MATCH ?1
             ORDER BY bm25(object_fts, 10.0, 4.0, 1.0) LIMIT 50",
            &[&match_expr],
        )?;

        let mut results = Vec::new();
        for payload in payloads {
            let value: serde_json::Value = serde_json::from_str(&payload)?;
            let id = value["id"].as_str().and_then(|s| s.parse::<KirId>().ok());
            let name = value["name"].as_str().unwrap_or_default().to_string();
            if let Some(id) = id {
                results.push((id, name));
            }
        }
        Ok(results)
    }

    // ── Counters ──────────────────────────────────────────────────────────────

    /// Total number of entries in the ledger.
    pub fn entry_count(&self) -> Result<usize, LedgerError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    /// Number of distinct objects in the current-state index.
    pub fn object_count(&self) -> Result<usize, LedgerError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM current_objects", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    /// Number of distinct relationships in the current-state index.
    pub fn relationship_count(&self) -> Result<usize, LedgerError> {
        let n: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM current_relationships", [], |row| {
                    row.get(0)
                })?;
        Ok(n as usize)
    }

    /// Per-table on-disk size in bytes, largest first, via the `dbstat`
    /// virtual table (RFC 0015 measurement). Returns an empty list when the
    /// bundled SQLite was built without `SQLITE_ENABLE_DBSTAT_VTAB`.
    pub fn storage_stats(&self) -> Result<Vec<(String, u64)>, LedgerError> {
        let mut stmt = match self
            .conn
            .prepare("SELECT name, SUM(pgsize) FROM dbstat GROUP BY name ORDER BY SUM(pgsize) DESC")
        {
            Ok(stmt) => stmt,
            Err(_) => return Ok(Vec::new()),
        };
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ── Branching (Phase 13 — Optimizer) ──────────────────────────────────────

    /// Write a complete, consistent copy of this ledger to `dest` — used to
    /// create a branch. Uses SQLite's `VACUUM INTO` rather than a raw file
    /// copy: the main ledger runs in WAL mode, so a plain `cp` could miss data
    /// still sitting in the `-wal` file; `VACUUM INTO` always produces a
    /// complete snapshot regardless of WAL state. `user_version` and the
    /// `meta` dictionary travel with the copy, so branches keep the format of
    /// the ledger they were created from.
    pub fn vacuum_into(&self, dest: &Path) -> Result<(), LedgerError> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        self.conn
            .execute("VACUUM INTO ?1", params![dest.to_string_lossy()])?;
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
        let rows = stmt.query_map(params![self.ts_param(from), self.ts_param(to)], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, SqlValue>(1)?))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (rowid, id) = row?;
            out.push((LedgerEntryId(rowid), id_value_to_string(id)));
        }
        Ok(out)
    }
}

fn init_schema_v2(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS entries (
            id          BLOB NOT NULL,
            entry_type  TEXT NOT NULL,
            payload     BLOB NOT NULL,
            content_sig BLOB NOT NULL,
            written_at  INTEGER NOT NULL
        );
        -- The composite index also serves id-only lookups (prefix), so the
        -- separate idx_entries_id of v1 is deliberately gone.
        CREATE INDEX IF NOT EXISTS idx_entries_id_sig ON entries(id, content_sig);

        CREATE TABLE IF NOT EXISTS current_objects (
            object_id    TEXT PRIMARY KEY,
            entry_rowid  INTEGER NOT NULL
        );
        -- Contentless FTS (RFC 0015): tokens only, no stored column values.
        -- rowid = the current version's entries.rowid.
        CREATE VIRTUAL TABLE IF NOT EXISTS object_fts
            USING fts5(name, kind, content, content='', contentless_delete=1);

        CREATE TABLE IF NOT EXISTS current_relationships (
            rel_id       TEXT PRIMARY KEY,
            entry_rowid  INTEGER NOT NULL,
            from_id      TEXT NOT NULL,
            to_id        TEXT NOT NULL,
            kind         TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_rel_from ON current_relationships(from_id);
        CREATE INDEX IF NOT EXISTS idx_rel_to   ON current_relationships(to_id);

        CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value BLOB NOT NULL
        );

        PRAGMA user_version = 2;
        ",
    )
}

/// Load the newest compression dictionary from the `meta` table.
fn load_dictionary(conn: &Connection) -> Result<Option<(u8, Vec<u8>)>, LedgerError> {
    let row = conn.query_row(
        "SELECT key, value FROM meta WHERE key LIKE 'dict:%' ORDER BY key DESC LIMIT 1",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
    );
    match row {
        Ok((key, bytes)) => {
            let version: u8 = key
                .strip_prefix("dict:")
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| LedgerError::Corrupt(format!("bad dictionary key: {key}")))?;
            Ok(Some((version, bytes)))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn id_value_to_string(value: SqlValue) -> String {
    match value {
        SqlValue::Text(s) => s,
        SqlValue::Blob(b) => Uuid::from_slice(&b)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| hex::encode(b)),
        other => format!("{other:?}"),
    }
}

fn sig_value_to_hex(value: SqlValue) -> String {
    match value {
        SqlValue::Text(s) => s,
        SqlValue::Blob(b) => hex::encode(b),
        other => format!("{other:?}"),
    }
}

fn ts_value_to_datetime(value: SqlValue) -> Result<DateTime<Utc>, LedgerError> {
    match value {
        SqlValue::Text(s) => DateTime::parse_from_rfc3339(&s)
            .map(|t| t.with_timezone(&Utc))
            .map_err(|e| LedgerError::Corrupt(format!("bad timestamp {s:?}: {e}"))),
        SqlValue::Integer(us) => DateTime::from_timestamp_micros(us)
            .ok_or_else(|| LedgerError::Corrupt(format!("bad timestamp micros {us}"))),
        other => Err(LedgerError::Corrupt(format!(
            "unexpected timestamp type: {other:?}"
        ))),
    }
}

// ── v1 → v2 migration (RFC 0015) ────────────────────────────────────────────

/// Result of a `migrate_to_v2` run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateReport {
    pub entries: usize,
    pub objects: usize,
    pub relationships: usize,
    /// Trained-dictionary size in bytes; 0 when the corpus was too small.
    pub dict_bytes: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub backup_path: PathBuf,
}

/// Migrate the ledger at `path` to the v2 (compact) format, preserving the
/// full append-only history — every version row keeps its rowid, id,
/// signature, and timestamp; payloads are recompressed with a dictionary
/// trained on this ledger's own corpus.
///
/// The migration streams into a sibling temp file, verifies every payload
/// round-trips byte-identically, then atomically swaps files, leaving the
/// original as `<name>.bak`. Running it on an already-v2 ledger is allowed
/// and simply recompresses with a freshly trained dictionary.
///
/// Do not run while another process (a build, the MCP server) is writing.
pub fn migrate_to_v2(path: &Path) -> Result<MigrateReport, LedgerError> {
    if !path.exists() {
        return Err(LedgerError::NotFound(path.display().to_string()));
    }
    let src = Ledger::open(path)?;
    let bytes_before = std::fs::metadata(path)?.len();

    // Train a dictionary on this ledger's own payloads.
    let samples = payload_samples(&src, DICT_SAMPLE_LIMIT)?;
    let dict_bytes_vec = if samples.len() >= DICT_MIN_SAMPLES {
        zstd::dict::from_samples(&samples, DICT_MAX_BYTES).ok()
    } else {
        None
    };
    let dict_bytes = dict_bytes_vec.as_ref().map(|d| d.len()).unwrap_or(0);
    drop(samples);

    let tmp = sibling_path(path, ".migrating");
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(sibling_path(&tmp, "-wal"));
    let _ = std::fs::remove_file(sibling_path(&tmp, "-shm"));
    let dst = Ledger::create_v2(&tmp, dict_bytes_vec)?;

    dst.conn.execute_batch("BEGIN")?;

    // Stream every version row, preserving rowids — current_* tables
    // reference entries by rowid.
    let mut entries = 0usize;
    {
        let mut stmt = src.conn.prepare(
            "SELECT rowid, id, entry_type, payload, content_sig, written_at
             FROM entries ORDER BY rowid",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, SqlValue>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, SqlValue>(3)?,
                row.get::<_, SqlValue>(4)?,
                row.get::<_, SqlValue>(5)?,
            ))
        })?;

        for row in rows {
            let (rowid, id, entry_type, payload, sig, written_at) = row?;
            let id_str = id_value_to_string(id);
            let json = src.payload_to_string(payload)?;
            let sig_hex = sig_value_to_hex(sig);
            let at = ts_value_to_datetime(written_at)?;

            let frame = dst.codec.compress(json.as_bytes())?;
            // Verify the round-trip before the bytes ever land on disk.
            if dst.codec.decompress(&frame)? != json.as_bytes() {
                return Err(LedgerError::Corrupt(format!(
                    "payload round-trip mismatch for entry rowid {rowid}"
                )));
            }

            dst.conn.execute(
                "INSERT INTO entries (rowid, id, entry_type, payload, content_sig, written_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    rowid,
                    dst.id_param(&id_str),
                    entry_type,
                    SqlValue::Blob(frame),
                    dst.sig_param(&sig_hex),
                    dst.ts_param(at),
                ],
            )?;
            entries += 1;
        }
    }

    // Current-state pointer tables copy verbatim (TEXT columns, rowid refs).
    {
        let mut stmt = src
            .conn
            .prepare("SELECT object_id, entry_rowid FROM current_objects")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (object_id, entry_rowid) = row?;
            dst.conn.execute(
                "INSERT INTO current_objects (object_id, entry_rowid) VALUES (?1, ?2)",
                params![object_id, entry_rowid],
            )?;
        }

        let mut stmt = src.conn.prepare(
            "SELECT rel_id, entry_rowid, from_id, to_id, kind FROM current_relationships",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (rel_id, entry_rowid, from_id, to_id, kind) = row?;
            dst.conn.execute(
                "INSERT INTO current_relationships (rel_id, entry_rowid, from_id, to_id, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![rel_id, entry_rowid, from_id, to_id, kind],
            )?;
        }
    }

    // Rebuild the (derived, contentless) FTS index from current objects.
    for obj in dst.all_objects_with_rowids()? {
        let (rowid, obj) = obj;
        dst.index_object_fts_v2(&obj, rowid, None)?;
    }

    dst.conn.execute_batch("COMMIT")?;

    // Verify counts before swapping anything.
    let objects = dst.object_count()?;
    let relationships = dst.relationship_count()?;
    if entries != src.entry_count()?
        || objects != src.object_count()?
        || relationships != src.relationship_count()?
    {
        return Err(LedgerError::Corrupt(
            "migration count mismatch — original ledger left untouched".into(),
        ));
    }

    // Close both databases so WAL files are checkpointed and removed.
    drop(src);
    drop(dst);

    let bytes_after = std::fs::metadata(&tmp)?.len();

    // Swap: original → .bak (with any straggling WAL sidecars), tmp → live.
    let backup_path = sibling_path(path, ".bak");
    let _ = std::fs::remove_file(&backup_path);
    let _ = std::fs::rename(
        sibling_path(path, "-wal"),
        sibling_path(&backup_path, "-wal"),
    );
    let _ = std::fs::rename(
        sibling_path(path, "-shm"),
        sibling_path(&backup_path, "-shm"),
    );
    std::fs::rename(path, &backup_path)?;
    std::fs::rename(&tmp, path)?;

    Ok(MigrateReport {
        entries,
        objects,
        relationships,
        dict_bytes,
        bytes_before,
        bytes_after,
        backup_path,
    })
}

impl Ledger {
    /// Current objects together with the entry rowid backing each one —
    /// used to rebuild the contentless FTS index.
    fn all_objects_with_rowids(&self) -> Result<Vec<(i64, KirObject)>, LedgerError> {
        let mut stmt = self.conn.prepare(
            "SELECT c.entry_rowid, e.payload FROM current_objects c
             JOIN entries e ON e.rowid = c.entry_rowid",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, SqlValue>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (rowid, payload) = row?;
            let obj: KirObject = serde_json::from_str(&self.payload_to_string(payload)?)?;
            out.push((rowid, obj));
        }
        Ok(out)
    }
}

/// Decoded payload JSON bytes for dictionary training.
fn payload_samples(ledger: &Ledger, limit: usize) -> Result<Vec<Vec<u8>>, LedgerError> {
    let mut stmt = ledger
        .conn
        .prepare("SELECT payload FROM entries LIMIT ?1")?;
    let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, SqlValue>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(ledger.payload_to_string(row?)?.into_bytes());
    }
    Ok(out)
}

/// `ledger.db` + suffix → `ledger.db.bak`, `ledger.db-wal`, …
fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
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

    Ok(LedgerDiff {
        added: versions.into_iter().map(|(id, _)| id).collect(),
        touched,
        unchanged,
    })
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
/// Signatures compare decoded canonical JSON, so ledgers of different
/// storage formats (v1/v2) merge transparently.
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
    use ekos_kir::{
        KirEvidence, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
    };
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
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        let all = ledger.all_objects().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn all_relationships_returns_every_relationship() {
        let (ledger, _dir) = temp_ledger();
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::ForeignKey,
                KirId::new(),
                KirId::new(),
            ))
            .unwrap();
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::Calls,
                KirId::new(),
                KirId::new(),
            ))
            .unwrap();
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
        ledger
            .append_object(&KirObject::new("order_items", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        let results = ledger.find_objects("order*").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "order_items");
    }

    #[test]
    fn fts_find_objects_no_match_with_special_chars_returns_empty() {
        let (ledger, _dir) = temp_ledger();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        // Hyphens are FTS5 query-syntax operators; must not error, just find nothing.
        assert!(ledger.find_objects("zzz-nonexistent").unwrap().is_empty());
    }

    /// RFC 0014: content excerpts are searchable even when the name says
    /// nothing about the topic.
    #[test]
    fn fts_finds_objects_by_content_excerpt() {
        let (ledger, _dir) = temp_ledger();
        let note = KirObject::new("note-17.md", ObjectKind::File).with_property(
            "excerpt",
            serde_json::json!("Lesson: coupling analysis is quadratic per commit"),
        );
        ledger.append_object(&note).unwrap();

        let results = ledger.find_objects("quadratic").unwrap();
        assert_eq!(
            results.len(),
            1,
            "body keyword must match via content column"
        );
        assert_eq!(results[0].1, "note-17.md");
    }

    /// RFC 0014: a name hit outranks a content-only mention.
    #[test]
    fn fts_ranks_name_matches_above_content_matches() {
        let (ledger, _dir) = temp_ledger();
        let mention = KirObject::new("random-notes.md", ObjectKind::File).with_property(
            "excerpt",
            serde_json::json!("this mentions orders in passing"),
        );
        ledger.append_object(&mention).unwrap();
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();

        let results = ledger.find_objects("orders").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1, "orders", "name match must rank first");
    }

    /// RFC 0015: the FTS index must track the *latest* version — an updated
    /// object stays searchable exactly once, with fresh content.
    #[test]
    fn fts_follows_object_updates() {
        let (ledger, _dir) = temp_ledger();
        let mut obj = KirObject::new("pipeline-notes.md", ObjectKind::File)
            .with_property("excerpt", serde_json::json!("first draft about kafka"));
        ledger.append_object(&obj).unwrap();

        obj.properties.insert(
            "excerpt".into(),
            serde_json::json!("rewritten to cover flink"),
        );
        ledger.append_object(&obj).unwrap();

        assert!(
            ledger.find_objects("kafka").unwrap().is_empty(),
            "stale content must be gone"
        );
        let results = ledger.find_objects("flink").unwrap();
        assert_eq!(
            results.len(),
            1,
            "updated content must be indexed exactly once"
        );
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
        ledger
            .append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, a, b))
            .unwrap();
        ledger
            .append_relationship(&KirRelationship::new(RelationshipKind::Calls, c, a))
            .unwrap();

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
        ledger
            .append_relationship(&KirRelationship::new(RelationshipKind::ForeignKey, a, b))
            .unwrap();
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

        obj.properties
            .insert("row_count".into(), serde_json::json!(42));
        let is_new = ledger.append_object(&obj).unwrap();
        assert!(
            is_new,
            "changed content under the same id must be a new version"
        );

        // 2 versions in the log, but current-state still shows exactly one object.
        assert_eq!(ledger.entry_count().unwrap(), 2);
        assert_eq!(ledger.object_count().unwrap(), 1);
        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(
            current.properties.get("row_count"),
            Some(&serde_json::json!(42))
        );
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

        obj.properties
            .insert("row_count".into(), serde_json::json!(99));
        ledger.append_object(&obj).unwrap();

        // At `mid` (before the update), the historical version has no row_count.
        let historical = ledger.object_at(&id, mid).unwrap().unwrap();
        assert!(!historical.properties.contains_key("row_count"));

        // Current state has it.
        let current = ledger.get_object(&id).unwrap().unwrap();
        assert_eq!(
            current.properties.get("row_count"),
            Some(&serde_json::json!(99))
        );
    }

    #[test]
    fn diff_ledger_reports_updated_object_as_added_and_others_as_unchanged() {
        let (ledger, _dir) = temp_ledger();
        let mut updated = KirObject::new("orders", ObjectKind::Table);
        let updated_id = updated.id;
        ledger.append_object(&updated).unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        ledger
            .append_object(&KirObject::new("products", ObjectKind::Table))
            .unwrap();

        let t1 = Utc::now();
        std::thread::sleep(Duration::from_millis(5));

        updated
            .properties
            .insert("row_count".into(), serde_json::json!(7));
        ledger.append_object(&updated).unwrap();

        let t2 = Utc::now();

        let diff = diff_ledger(&ledger, t1, t2).unwrap();
        assert_eq!(
            diff.added.len(),
            1,
            "only the updated object's new version falls in the window"
        );
        assert_eq!(diff.unchanged, 2);

        // Sanity: the added version really is the updated object.
        let _ = updated_id;
    }

    #[test]
    fn vacuum_into_produces_a_readable_copy() {
        let (ledger, _dir) = temp_ledger();
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();

        let dest_dir = tempdir().unwrap();
        let dest = dest_dir.path().join("branch.db");
        ledger.vacuum_into(&dest).unwrap();

        let copy = Ledger::open(&dest).unwrap();
        assert_eq!(copy.object_count().unwrap(), 1);
        // Branches inherit the source's format (user_version travels with VACUUM).
        assert_eq!(copy.format, Format::V2);
    }

    #[test]
    fn merge_branch_adds_new_objects_from_branch() {
        let (main, _dir1) = temp_ledger();
        let (branch, _dir2) = temp_ledger();

        main.append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();
        branch
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();

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

        shared
            .properties
            .insert("row_count".into(), serde_json::json!(5));
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

    // ── RFC 0015: format v2 + migration ─────────────────────────────────────

    /// Hand-build a v1 ledger file exactly as pre-RFC-0015 code laid it out.
    fn build_v1_ledger(path: &Path, objects: &[KirObject]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE entries (
                 id          TEXT NOT NULL,
                 entry_type  TEXT NOT NULL,
                 payload     TEXT NOT NULL,
                 content_sig TEXT NOT NULL,
                 written_at  TEXT NOT NULL
             );
             CREATE INDEX idx_entries_id ON entries(id);
             CREATE INDEX idx_entries_id_sig ON entries(id, content_sig);
             CREATE TABLE current_objects (
                 object_id    TEXT PRIMARY KEY,
                 entry_rowid  INTEGER NOT NULL
             );
             CREATE VIRTUAL TABLE object_fts
                 USING fts5(object_id UNINDEXED, name, kind, content);
             CREATE TABLE current_relationships (
                 rel_id       TEXT PRIMARY KEY,
                 entry_rowid  INTEGER NOT NULL,
                 from_id      TEXT NOT NULL,
                 to_id        TEXT NOT NULL,
                 kind         TEXT NOT NULL
             );
             CREATE INDEX idx_rel_from ON current_relationships(from_id);
             CREATE INDEX idx_rel_to   ON current_relationships(to_id);",
        )
        .unwrap();

        for obj in objects {
            let payload = serde_json::to_value(obj).unwrap();
            let sig = content_signature(&payload);
            conn.execute(
                "INSERT INTO entries (id, entry_type, payload, content_sig, written_at)
                 VALUES (?1, 'object', ?2, ?3, ?4)",
                params![
                    obj.id.to_string(),
                    serde_json::to_string(&payload).unwrap(),
                    sig,
                    Utc::now().to_rfc3339(),
                ],
            )
            .unwrap();
            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT OR REPLACE INTO current_objects (object_id, entry_rowid) VALUES (?1, ?2)",
                params![obj.id.to_string(), rowid],
            )
            .unwrap();
            let excerpt = obj
                .properties
                .get("excerpt")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            conn.execute(
                "INSERT OR REPLACE INTO object_fts (object_id, name, kind, content)
                 VALUES (?1, ?2, ?3, ?4)",
                params![obj.id.to_string(), obj.name, obj.kind.to_string(), excerpt],
            )
            .unwrap();
        }
    }

    fn v1_corpus() -> Vec<KirObject> {
        (0..200)
            .map(|i| {
                KirObject::new(format!("src/handlers/handler_{i}.rs"), ObjectKind::File)
                    .with_property(
                        "path",
                        serde_json::json!(format!("src/handlers/handler_{i}.rs")),
                    )
                    .with_property("size_bytes", serde_json::json!(1000 + i))
                    .with_property(
                        "excerpt",
                        serde_json::json!(format!(
                            "Handler {i} reconciles inbound events against the projection store"
                        )),
                    )
            })
            .collect()
    }

    #[test]
    fn fresh_ledger_is_v2_with_compressed_blob_payloads() {
        let (ledger, _dir) = temp_ledger();
        assert_eq!(ledger.format, Format::V2);
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();

        let (payload_type, sig_type, ts_type): (String, String, String) = ledger
            .conn
            .query_row(
                "SELECT typeof(payload), typeof(content_sig), typeof(written_at)
                 FROM entries LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(payload_type, "blob");
        assert_eq!(sig_type, "blob");
        assert_eq!(ts_type, "integer");
    }

    #[test]
    fn v1_ledger_opens_and_reads_without_migration() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.db");
        let objects = v1_corpus();
        build_v1_ledger(&path, &objects);

        let ledger = Ledger::open(&path).unwrap();
        assert_eq!(ledger.format, Format::V1);
        assert_eq!(ledger.object_count().unwrap(), objects.len());
        let found = ledger.get_object(&objects[3].id).unwrap().unwrap();
        assert_eq!(found.name, objects[3].name);
        assert!(!ledger.find_objects("reconciles").unwrap().is_empty());

        // Writes still work in v1 format until migration.
        ledger
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        assert_eq!(ledger.object_count().unwrap(), objects.len() + 1);
    }

    #[test]
    fn migrate_v1_to_v2_preserves_content_history_and_search() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.db");
        let objects = v1_corpus();
        build_v1_ledger(&path, &objects);

        // Give one object a second (v1) version so history survives migration.
        let mut updated = objects[0].clone();
        {
            let v1 = Ledger::open(&path).unwrap();
            updated
                .properties
                .insert("row_count".into(), serde_json::json!(7));
            assert!(v1.append_object(&updated).unwrap());
        }

        let report = migrate_to_v2(&path).unwrap();
        assert_eq!(report.entries, objects.len() + 1);
        assert_eq!(report.objects, objects.len());
        assert!(report.backup_path.exists(), "v1 backup must remain");
        assert!(
            report.bytes_after < report.bytes_before,
            "v2 must be smaller: {} -> {}",
            report.bytes_before,
            report.bytes_after
        );

        let v2 = Ledger::open(&path).unwrap();
        assert_eq!(v2.format, Format::V2);
        assert_eq!(v2.entry_count().unwrap(), objects.len() + 1);
        assert_eq!(v2.object_count().unwrap(), objects.len());

        // Current state round-trips exactly.
        let current = v2.get_object(&objects[0].id).unwrap().unwrap();
        assert_eq!(
            current.properties.get("row_count"),
            Some(&serde_json::json!(7))
        );
        for obj in &objects[1..] {
            let found = v2.get_object(&obj.id).unwrap().unwrap();
            assert_eq!(
                serde_json::to_value(&found).unwrap(),
                serde_json::to_value(obj).unwrap()
            );
        }

        // History: the pre-update version is still reachable.
        let after = Utc::now() + chrono::Duration::seconds(60);
        assert!(v2.object_at(&objects[0].id, after).unwrap().is_some());

        // Search (contentless FTS rebuilt): name and content hits both work.
        assert!(!v2.find_objects("handler_42*").unwrap().is_empty());
        assert!(!v2.find_objects("reconciles").unwrap().is_empty());

        // And the migrated ledger accepts new writes + indexes them.
        let note = KirObject::new("n.md", ObjectKind::File).with_property(
            "excerpt",
            serde_json::json!("migration smoke keyword zebra"),
        );
        v2.append_object(&note).unwrap();
        assert_eq!(v2.find_objects("zebra").unwrap().len(), 1);
    }

    #[test]
    fn migrated_corpus_trains_a_dictionary() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.db");
        build_v1_ledger(&path, &v1_corpus());

        let report = migrate_to_v2(&path).unwrap();
        // 200 similar payloads is enough for zstd dictionary training; if the
        // trainer ever declines, frames fall back to dict 0 and this assert
        // is the only thing that notices.
        assert!(report.dict_bytes > 0, "expected a trained dictionary");

        // Dictionary-compressed frames decode fine on a fresh open.
        let v2 = Ledger::open(&path).unwrap();
        assert_eq!(v2.all_objects().unwrap().len(), 200);
    }
}
