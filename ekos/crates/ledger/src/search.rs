//! RFC 0016 Phase 5 — integrated tantivy search.
//!
//! Replaces the FTS5 index (and Phase 4's placeholder scorer) with a real
//! BM25 engine while keeping the semantics RFC 0014 pinned: query terms are
//! ANDed, a trailing `*` prefix-matches a token, and field boosts make a
//! name hit (10×) outrank a kind hit (4×) outrank a content-excerpt hit (1×).
//!
//! The index is **derived and rebuildable** (project invariant): documents
//! are the *current* state of object entities; a `last_tx` marker records
//! how far the index has seen. On open, the ledger replays only the batches
//! past the marker (or rebuilds from scratch if the directory is missing).
//! Appends never pay a tantivy commit — upserts buffer in the writer and
//! commit lazily on the first query after a write (group commit), so build
//! throughput is unaffected and search is read-your-writes.

use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, PhrasePrefixQuery, Query, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, STORED, STRING, Schema, TEXT, Value};
use tantivy::{Index, IndexReader, IndexWriter, TantivyDocument, Term};
use uuid::Uuid;

use crate::LedgerError;
use crate::fact::TxId;

const WRITER_HEAP_BYTES: usize = 32 * 1024 * 1024;

fn terr(e: impl std::fmt::Display) -> LedgerError {
    LedgerError::Corrupt(format!("search index: {e}"))
}

/// The tantivy-backed object search index of a [`crate::FactLedger`].
pub struct SearchIndex {
    writer: IndexWriter,
    reader: IndexReader,
    marker_path: PathBuf,
    dirty: bool,
    f_id: Field,
    f_name: Field,
    f_kind: Field,
    f_content: Field,
}

impl SearchIndex {
    /// Open (or create) the index under `dir`. Returns the index and the
    /// last transaction it has seen (`TxId(0)`-exclusive watermark; `None`
    /// means "nothing indexed / rebuilt from scratch, replay everything").
    pub fn open(dir: &Path) -> Result<(Self, Option<TxId>), LedgerError> {
        let fresh = !dir.exists();
        std::fs::create_dir_all(dir).map_err(LedgerError::Io)?;

        let mut schema_builder = Schema::builder();
        let f_id = schema_builder.add_text_field("id", STRING | STORED);
        let f_name = schema_builder.add_text_field("name", TEXT | STORED);
        let f_kind = schema_builder.add_text_field("kind", TEXT);
        let f_content = schema_builder.add_text_field("content", TEXT);
        let schema = schema_builder.build();

        let mmap_dir = tantivy::directory::MmapDirectory::open(dir).map_err(terr)?;
        let index = Index::open_or_create(mmap_dir, schema).map_err(terr)?;
        let writer = index.writer(WRITER_HEAP_BYTES).map_err(terr)?;
        let reader = index.reader().map_err(terr)?;

        let marker_path = dir.join("last_tx");
        let marker = if fresh {
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
            },
            marker,
        ))
    }

    /// Buffer an upsert of one object's current state. No commit — that
    /// happens lazily on the next [`Self::query`].
    pub fn upsert(&mut self, id: Uuid, name: &str, kind: &str, content: &str) {
        let id_str = id.to_string();
        self.writer
            .delete_term(Term::from_field_text(self.f_id, &id_str));
        let mut doc = TantivyDocument::new();
        doc.add_text(self.f_id, &id_str);
        doc.add_text(self.f_name, name);
        doc.add_text(self.f_kind, kind);
        doc.add_text(self.f_content, content);
        let _ = self.writer.add_document(doc);
        self.dirty = true;
    }

    /// Commit buffered upserts (if any) and record the watermark.
    pub fn commit(&mut self, last_tx: Option<TxId>) -> Result<(), LedgerError> {
        if !self.dirty {
            return Ok(());
        }
        self.writer.commit().map_err(terr)?;
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
