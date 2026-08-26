//! RFC 0016 Phase 5 — integrated tantivy search.
//!
//! Replaces the FTS5 index (and Phase 4's placeholder scorer) with a real
//! BM25 engine while keeping the semantics RFC 0014 pinned: query terms are
//! ANDed, a trailing `*` prefix-matches a token, and field boosts make a
//! name hit (10×) outrank a kind hit (4×) outrank a content-excerpt hit (1×).
//! RFC 0101 adds one more, independent boost: a document under a real
//! `memory/` observed path (`KirObject::is_under_memory_path`) scores an
//! extra 5× on top of whatever it already earned from name/kind/content —
//! applied as an unconditional `Should` clause alongside the per-term `Must`
//! clauses, so it only ever re-ranks documents that already matched the
//! query on their own merits; it can never make a non-matching document
//! appear. RFC 0014 named this exact capability as a Non-goal in 2026-07-17,
//! "revisit with real usage" — the `.claude/skills/memory` workflow has been
//! real, live usage since the same day.
//!
//! The index is **derived and rebuildable** (project invariant): documents
//! are the *current* state of object entities; a `last_tx` marker records
//! how far the index has seen. On open, the ledger replays only the batches
//! past the marker (or rebuilds from scratch if the directory is missing).
//! Appends never pay a tantivy commit — upserts buffer in the writer and
//! commit lazily on the first query after a write (group commit), so build
//! throughput is unaffected and search is read-your-writes.
//!
//! RFC 0103 leans on that same rebuildable invariant for a second purpose: a stale on-disk
//! tantivy schema (a code change like RFC 0101's `memory_path` field addition, opened against an
//! index built before that change) self-heals on a writable open by wiping and rebuilding
//! ([`rebuild_stale_schema`]) rather than failing outright — see [`SearchIndex::open_impl`].

use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, PhrasePrefixQuery, Query, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, STORED, STRING, Schema, TEXT, Value};
use tantivy::{Index, IndexReader, IndexWriter, TantivyDocument, Term};
use uuid::Uuid;

use crate::LedgerError;
use crate::fact::TxId;

const WRITER_HEAP_BYTES: usize = 32 * 1024 * 1024;
/// RFC 0101: meaningfully above a plain content-field hit (1×) — RFC 0014's
/// own motivating example was a common-term content search drowning the one
/// memory note that should rank first among plain project-file content
/// matches — but below a real exact name match (10×), so a memory note's
/// content boost still never outranks something the user's query literally
/// named.
const MEMORY_PATH_BOOST: f32 = 5.0;
/// The literal indexed token [`SearchIndex::upsert`] writes to the
/// `memory_path` field when its `is_memory_path` argument is true (the
/// caller derives that from `KirObject::is_under_memory_path`), and
/// [`SearchIndex::query`] always searches for. Any fixed, non-empty token
/// works — its value has no meaning beyond "present".
const MEMORY_PATH_TOKEN: &str = "1";

fn terr(e: impl std::fmt::Display) -> LedgerError {
    LedgerError::Corrupt(format!("search index: {e}"))
}

/// Wipes `dir`'s on-disk contents so a subsequent `Index::open_or_create` starts genuinely fresh
/// — the self-heal path for a stale on-disk tantivy schema (RFC 0103: e.g. RFC 0101 added a new
/// `memory_path` field with no migration for an already-built index, breaking every pre-existing
/// `FactLedger` workspace's `Index::open_or_create` call). Safe because the search index is a
/// **derived, rebuildable** artifact (this module's own doc comment states this as a project
/// invariant) — `FactLedger::open_with_seal_threshold`'s existing catchup logic already knows how
/// to fully reindex every object when [`SearchIndex::open`] returns a `None` marker (the same
/// path a brand-new workspace's first open already takes), so wiping this directory and returning
/// `None` is not new behavior, just a new way to reach it. `dir` is dedicated entirely to this one
/// search index (nothing else lives there), so a full wipe is unambiguous.
fn rebuild_stale_schema(dir: &Path) -> Result<(), LedgerError> {
    std::fs::remove_dir_all(dir).map_err(LedgerError::Io)?;
    std::fs::create_dir_all(dir).map_err(LedgerError::Io)?;
    Ok(())
}

/// The tantivy-backed object search index of a [`crate::FactLedger`].
///
/// `writer` is `None` for a read-only-opened index (RFC 0097): tantivy's
/// `Index::writer(..)` is what acquires the on-disk `IndexWriter` lockfile,
/// exclusive for the writer's whole lifetime — not just while a commit is in
/// flight. A store meant to stay open and cached across many calls (e.g. an
/// MCP server between `tools/call`s) must never hold that lock, or it blocks
/// any real concurrent writer (`ekos build`/`commit` in a separate process)
/// from ever acquiring it. `reader` alone is always safe to hold indefinitely
/// and share across readers/processes.
pub struct SearchIndex {
    writer: Option<IndexWriter>,
    reader: IndexReader,
    marker_path: PathBuf,
    dirty: bool,
    f_id: Field,
    f_name: Field,
    f_kind: Field,
    f_content: Field,
    f_memory_path: Field,
}

impl SearchIndex {
    /// Open (or create) the index under `dir`, acquiring the writer lock.
    /// Returns the index and the last transaction it has seen (`TxId(0)`-
    /// exclusive watermark; `None` means "nothing indexed / rebuilt from
    /// scratch, replay everything").
    pub fn open(dir: &Path) -> Result<(Self, Option<TxId>), LedgerError> {
        Self::open_impl(dir, true)
    }

    /// Open the index under `dir` for reads only — never calls
    /// `Index::writer(..)`, so it never contends for the writer lock a
    /// concurrent real writer needs. [`Self::upsert`]/[`Self::commit`]
    /// become no-ops on the result (defense in depth — `FactLedger`'s own
    /// `append_inner` already rejects every write before reaching here, see
    /// `LedgerError::ReadOnly`). `dir` must already exist; a read-only open
    /// never creates a fresh index.
    pub fn open_read_only(dir: &Path) -> Result<(Self, Option<TxId>), LedgerError> {
        Self::open_impl(dir, false)
    }

    fn open_impl(dir: &Path, writable: bool) -> Result<(Self, Option<TxId>), LedgerError> {
        let fresh = !dir.exists();
        if fresh && !writable {
            return Err(LedgerError::NotFound(dir.display().to_string()));
        }
        std::fs::create_dir_all(dir).map_err(LedgerError::Io)?;

        let mut schema_builder = Schema::builder();
        let f_id = schema_builder.add_text_field("id", STRING | STORED);
        let f_name = schema_builder.add_text_field("name", TEXT | STORED);
        let f_kind = schema_builder.add_text_field("kind", TEXT);
        let f_content = schema_builder.add_text_field("content", TEXT);
        // RFC 0101: STRING (exact-match, no tokenizer splitting), never
        // STORED — this field only ever needs to be searched, its value is
        // never read back out of a hit.
        let f_memory_path = schema_builder.add_text_field("memory_path", STRING);
        let schema = schema_builder.build();

        let mmap_dir = tantivy::directory::MmapDirectory::open(dir).map_err(terr)?;
        let mut rebuilt = false;
        let index = match Index::open_or_create(mmap_dir, schema.clone()) {
            Ok(index) => index,
            // RFC 0103: a schema field addition (e.g. RFC 0101's `memory_path`) leaves an
            // already-built on-disk index with a stale schema — tantivy validates rather than
            // upgrading. Self-heal on a writable open only: wipe and rebuild (see
            // `rebuild_stale_schema`'s own doc comment for why this is safe), never on a
            // read-only open, which must not mutate the directory (same reasoning
            // `open_read_only` already documents for never acquiring the writer lock).
            Err(tantivy::TantivyError::SchemaError(_)) if writable => {
                rebuild_stale_schema(dir)?;
                rebuilt = true;
                let mmap_dir = tantivy::directory::MmapDirectory::open(dir).map_err(terr)?;
                Index::open_or_create(mmap_dir, schema).map_err(terr)?
            }
            Err(tantivy::TantivyError::SchemaError(msg)) => {
                return Err(LedgerError::Corrupt(format!(
                    "search index schema is stale (a newer EKOS version added a new indexed \
                     field) and a read-only open cannot rebuild it — open writable (e.g. `ekos \
                     build`) once to self-heal, then reopen read-only: {msg}"
                )));
            }
            Err(e) => return Err(terr(e)),
        };
        let writer = if writable {
            Some(index.writer(WRITER_HEAP_BYTES).map_err(terr)?)
        } else {
            None
        };
        let reader = index.reader().map_err(terr)?;

        let marker_path = dir.join("last_tx");
        let marker = if fresh || rebuilt {
            None
        } else {
            std::fs::read_to_string(&marker_path)
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(TxId)
        };
        Ok((
            Self {
                writer,
                reader,
                marker_path,
                dirty: false,
                f_id,
                f_name,
                f_kind,
                f_content,
                f_memory_path,
            },
            marker,
        ))
    }

    /// Buffer an upsert of one object's current state. No commit — that
    /// happens lazily on the next [`Self::query`]. A no-op when opened
    /// read-only (no writer to buffer into). `is_memory_path` (RFC 0101,
    /// from `KirObject::is_under_memory_path`) indexes a real, unconditional
    /// ranking boost — see the module doc comment.
    pub fn upsert(
        &mut self,
        id: Uuid,
        name: &str,
        kind: &str,
        content: &str,
        is_memory_path: bool,
    ) {
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        let id_str = id.to_string();
        writer.delete_term(Term::from_field_text(self.f_id, &id_str));
        let mut doc = TantivyDocument::new();
        doc.add_text(self.f_id, &id_str);
        doc.add_text(self.f_name, name);
        doc.add_text(self.f_kind, kind);
        doc.add_text(self.f_content, content);
        if is_memory_path {
            doc.add_text(self.f_memory_path, MEMORY_PATH_TOKEN);
        }
        let _ = writer.add_document(doc);
        self.dirty = true;
    }

    /// Commit buffered upserts (if any) and record the watermark. A no-op
    /// when opened read-only — `dirty` can never become true there since
    /// [`Self::upsert`] already no-ops, but the writer-less case is also
    /// guarded explicitly rather than relying on that alone.
    pub fn commit(&mut self, last_tx: Option<TxId>) -> Result<(), LedgerError> {
        if !self.dirty {
            return Ok(());
        }
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };
        writer.commit().map_err(terr)?;
        self.reader.reload().map_err(terr)?;
        if let Some(tx) = last_tx {
            std::fs::write(&self.marker_path, tx.0.to_string()).map_err(LedgerError::Io)?;
        }
        self.dirty = false;
        Ok(())
    }

    /// Ranked search: terms ANDed across fields with 10/4/1 boosts;
    /// `term*` prefix-matches. Returns `(entity, name)` pairs, best first.
    pub fn query(&self, query: &str, limit: usize) -> Result<Vec<(Uuid, String)>, LedgerError> {
        let terms: Vec<(String, bool)> = query
            .split(|c: char| !(c.is_alphanumeric() || c == '*'))
            .filter(|t| !t.is_empty())
            .map(|t| match t.strip_suffix('*') {
                Some(stem) => (stem.to_lowercase(), true),
                None => (t.trim_matches('*').to_lowercase(), false),
            })
            .filter(|(t, _)| !t.is_empty())
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        let mut must: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for (term, prefix) in &terms {
            let mut fields: Vec<(Occur, Box<dyn Query>)> = Vec::new();
            for (field, boost) in [
                (self.f_name, 10.0f32),
                (self.f_kind, 4.0),
                (self.f_content, 1.0),
            ] {
                let t = Term::from_field_text(field, term);
                let q: Box<dyn Query> = if *prefix {
                    Box::new(PhrasePrefixQuery::new(vec![t]))
                } else {
                    Box::new(TermQuery::new(t, IndexRecordOption::WithFreqs))
                };
                fields.push((Occur::Should, Box::new(BoostQuery::new(q, boost))));
            }
            must.push((Occur::Must, Box::new(BooleanQuery::new(fields))));
        }
        // RFC 0101: an unconditional Should clause, outside the per-term
        // Must array — it never gates which documents match (a document
        // that fails any Must clause is excluded regardless of this), it
        // only adds extra score to documents that already matched every
        // query term on their own, when those documents also happen to be
        // under a real memory/ path.
        must.push((
            Occur::Should,
            Box::new(BoostQuery::new(
                Box::new(TermQuery::new(
                    Term::from_field_text(self.f_memory_path, MEMORY_PATH_TOKEN),
                    IndexRecordOption::Basic,
                )),
                MEMORY_PATH_BOOST,
            )),
        ));
        let query = BooleanQuery::new(must);

        let searcher = self.reader.searcher();
        let top = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(terr)?;
        let mut out = Vec::with_capacity(top.len());
        for (_score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr).map_err(terr)?;
            let get = |f: Field| {
                doc.get_first(f)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            if let Ok(id) = get(self.f_id).parse::<Uuid>() {
                out.push((id, get(self.f_name)));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an on-disk tantivy index at `dir` with an intentionally *older*-shaped schema
    /// (missing the `memory_path` field `SearchIndex`'s current schema has) — simulates a
    /// pre-RFC-0101 workspace's on-disk search index, without depending on `SearchIndex` itself
    /// (which always builds the *current* schema).
    fn write_stale_schema_index(dir: &Path) {
        let mut schema_builder = Schema::builder();
        schema_builder.add_text_field("id", STRING | STORED);
        schema_builder.add_text_field("name", TEXT | STORED);
        schema_builder.add_text_field("kind", TEXT);
        schema_builder.add_text_field("content", TEXT);
        let schema = schema_builder.build();
        let mmap_dir = tantivy::directory::MmapDirectory::open(dir).unwrap();
        let index = Index::open_or_create(mmap_dir, schema).unwrap();
        // A committed writer is what actually persists `meta.json` to disk — an uncommitted
        // `Index::open_or_create` alone leaves nothing on disk for a later open to trip over.
        let mut writer: IndexWriter = index.writer(WRITER_HEAP_BYTES).unwrap();
        writer.commit().unwrap();
    }

    #[test]
    fn writable_open_self_heals_a_stale_on_disk_schema() {
        let dir = tempfile::tempdir().unwrap();
        write_stale_schema_index(dir.path());

        let (mut index, marker) = SearchIndex::open(dir.path()).unwrap();
        assert_eq!(
            marker, None,
            "a self-healed index must report no watermark, the same contract a genuinely fresh \
             workspace's first open already returns, so the caller's existing full-reindex path \
             (triggered by a None marker) fires with no other code change"
        );

        // The new field must be real and queryable after the heal, not just "opened without
        // erroring" — proves the rebuild used the *current* schema, not a half-migrated one.
        let id = Uuid::new_v4();
        index.upsert(id, "quadratic blowup", "Document", "quadratic blowup", true);
        index.commit(Some(TxId(1))).unwrap();
        let hits = index.query("quadratic", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, id);
    }

    #[test]
    fn read_only_open_refuses_to_self_heal_a_stale_schema_and_leaves_the_directory_untouched() {
        let dir = tempfile::tempdir().unwrap();
        write_stale_schema_index(dir.path());

        let before = std::fs::read(dir.path().join("meta.json")).unwrap();
        let msg = match SearchIndex::open_read_only(dir.path()) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected a read-only open against a stale schema to fail"),
        };
        assert!(
            msg.contains("open writable") && msg.contains("self-heal"),
            "error must name the real fix (open writable once), got: {msg}"
        );
        let after = std::fs::read(dir.path().join("meta.json")).unwrap();
        assert_eq!(
            before, after,
            "a read-only open must never mutate the on-disk index, even to self-heal"
        );

        // The stale schema must still need healing afterward — proves the read-only attempt
        // above didn't silently half-fix it.
        let (_, marker) = SearchIndex::open(dir.path()).unwrap();
        assert_eq!(
            marker, None,
            "still needed a real self-heal after the read-only attempt"
        );
    }

    #[test]
    fn genuine_corruption_is_not_mistaken_for_a_stale_schema() {
        let dir = tempfile::tempdir().unwrap();
        write_stale_schema_index(dir.path());
        // Corrupt `meta.json` itself (not just an outdated schema within it) — `Index::open`
        // fails before it ever gets to compare schemas, so this must surface as a real error,
        // not be silently caught by the schema-mismatch self-heal arm.
        std::fs::write(dir.path().join("meta.json"), b"not valid json").unwrap();

        let result = SearchIndex::open(dir.path());
        assert!(
            result.is_err(),
            "genuine corruption must still surface as an error, not be swallowed as a \
             self-healable schema mismatch"
        );
    }
}
