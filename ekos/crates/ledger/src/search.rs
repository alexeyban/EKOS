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
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TEXT, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::TokenStream;
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

/// One ranked hit, with how much of the query it actually matched (RFC 0139 §3.7).
///
/// `matched_terms`/`total_terms` is the signal a binary relaxed-vs-strict flag could not express.
/// Relaxation deliberately returns documents sharing *some* query words, and a consumer deciding
/// "does the corpus answer this at all?" needs to tell a hit covering 4 of 5 terms (probably the
/// answer) from one covering 1 of 6 (vocabulary noise). Refusing on the binary flag instead was
/// measured to collapse `code` answer correctness from 72.7% to 18.2%, because honest questions
/// also retrieve partial-overlap hits.
#[derive(Debug, Clone)]
pub struct ScoredHit {
    pub id: Uuid,
    pub name: String,
    pub score: f32,
    pub matched_terms: usize,
    pub total_terms: usize,
}

impl ScoredHit {
    /// Fraction of the query's terms this document matched, `0.0..=1.0`.
    pub fn coverage(&self) -> f32 {
        if self.total_terms == 0 {
            return 0.0;
        }
        self.matched_terms as f32 / self.total_terms as f32
    }
}

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
        // Only on a writable open. `create_dir_all` happens to return `Ok` for an existing
        // directory even on a read-only filesystem, but relying on that is a trap — the read-only
        // path should not be calling a mutating API at all.
        if writable {
            std::fs::create_dir_all(dir).map_err(LedgerError::Io)?;
        }

        // F7 (test-runs/run-20260901T160842Z): plain BM25 `TEXT` uses tantivy's `"default"`
        // tokenizer (lowercase + split, no stemming) — a singular mention ("the customer table")
        // never lexically matched a plural indexed name ("Customers"), while the exact plural form
        // did. `"en_stem"` is tantivy's own built-in tokenizer (pre-registered in every
        // `TokenizerManager::default()`, nothing to register by hand) — lowercase + stemming, so
        // "customer"/"customers" index to the same stem. Applied to `name`/`content` (free
        // natural-language text); `kind` stays default (a closed enum-like vocabulary, stemming
        // has no value there); `id`/`memory_path` stay `STRING` (exact-match, untouched).
        let stemmed_stored = TextOptions::default().set_stored().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("en_stem")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let stemmed = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("en_stem")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );

        let mut schema_builder = Schema::builder();
        let f_id = schema_builder.add_text_field("id", STRING | STORED);
        let f_name = schema_builder.add_text_field("name", stemmed_stored);
        let f_kind = schema_builder.add_text_field("kind", TEXT);
        let f_content = schema_builder.add_text_field("content", stemmed);
        // RFC 0101: STRING (exact-match, no tokenizer splitting), never
        // STORED — this field only ever needs to be searched, its value is
        // never read back out of a hit.
        let f_memory_path = schema_builder.add_text_field("memory_path", STRING);
        let schema = schema_builder.build();

        let mmap_dir = tantivy::directory::MmapDirectory::open(dir).map_err(terr)?;
        let mut rebuilt = false;

        // A read-only open must use `Index::open`, never `open_or_create`. The latter takes
        // tantivy's `META_LOCK` and will create the index if absent — both writes. On a genuinely
        // read-only filesystem (a `:ro` container mount, an artifact volume, a snapshot) that
        // fails with `Read-only file system (os error 30)`, which is how this surfaced: every
        // stats endpoint of the web console 500'd against a `:ro`-mounted workspace even though
        // each one only reads counts.
        //
        // The intent was already documented right below — "never on a read-only open, which must
        // not mutate the directory" — but only the *rebuild* path honoured it; the open itself
        // did not.
        let index = if !writable {
            match Index::open(mmap_dir) {
                Ok(opened) => {
                    // `Index::open` takes no expected schema and so, unlike `open_or_create`,
                    // never validates one. Without this check a stale on-disk schema would open
                    // "successfully" read-only and then mis-query — silently — where the writable
                    // path reports it.
                    if opened.schema() != schema {
                        return Err(LedgerError::Corrupt(
                            "search index schema is stale (a newer EKOS version added a new \
                             indexed field) and a read-only open cannot rebuild it — open \
                             writable (e.g. `ekos build`) once to self-heal, then reopen \
                             read-only"
                                .to_string(),
                        ));
                    }
                    opened
                }
                Err(tantivy::TantivyError::SchemaError(msg)) => {
                    return Err(LedgerError::Corrupt(format!(
                        "search index schema is stale (a newer EKOS version added a new indexed \
                         field) and a read-only open cannot rebuild it — open writable (e.g. \
                         `ekos build`) once to self-heal, then reopen read-only: {msg}"
                    )));
                }
                // No index on disk yet. The writable path would create one; a reader must not, so
                // it serves an empty in-RAM index and every non-search read still works. This is
                // the normal state for a backend-served partition whose `search/` has not been
                // synced (RFC 0111 §7's accepted approximation), not an error.
                Err(_) => Index::create_in_ram(schema.clone()),
            }
        } else {
            match Index::open_or_create(mmap_dir, schema.clone()) {
                Ok(index) => index,
                // RFC 0103: a schema field addition (e.g. RFC 0101's `memory_path`) leaves an
                // already-built on-disk index with a stale schema — tantivy validates rather than
                // upgrading. Self-heal by wiping and rebuilding (see `rebuild_stale_schema`'s own
                // doc comment for why this is safe). Only reachable here: the read-only arm above
                // reports the stale schema instead, since rebuilding would mutate the directory.
                Err(tantivy::TantivyError::SchemaError(_)) => {
                    rebuild_stale_schema(dir)?;
                    rebuilt = true;
                    let mmap_dir = tantivy::directory::MmapDirectory::open(dir).map_err(terr)?;
                    Index::open_or_create(mmap_dir, schema.clone()).map_err(terr)?
                }
                Err(e) => return Err(terr(e)),
            }
        };
        let writer = if writable {
            Some(index.writer(WRITER_HEAP_BYTES).map_err(terr)?)
        } else {
            None
        };
        // Opening *any* tantivy reader acquires `META_LOCK` — `IndexReader::open_segment_readers`
        // takes it to stop GC deleting segment files mid-open — and acquiring a lock writes a
        // lockfile. There is no reload policy that avoids it, so a tantivy index on a read-only
        // filesystem cannot be opened at all. That is an upstream constraint, not something this
        // crate can configure away.
        //
        // Rather than fail the whole ledger open, degrade: fall back to an empty in-RAM index so
        // every **non-search** read still works — counts, timelines, `FIND … COUNT GROUP BY`,
        // object state, neighbourhoods. This is the same accepted approximation the backend-served
        // partition path already makes when `search/` has not been synced (RFC 0111 §7). Search
        // itself then returns no hits, so it is logged loudly rather than passing silently.
        let mut index = index;
        let reader = match index.reader() {
            Ok(r) => r,
            Err(e) if !writable => {
                tracing::warn!(
                    dir = %dir.display(),
                    error = %e,
                    "search index unavailable on a read-only filesystem (tantivy needs a lockfile \
                     to open a reader) — continuing without search: counts, timelines and object \
                     reads work, but `query find` will return no hits. Mount the workspace \
                     writable to enable search."
                );
                index = Index::create_in_ram(schema.clone());
                index.reader().map_err(terr)?
            }
            Err(e) => return Err(terr(e)),
        };

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
        Ok(self
            .query_scored(query, limit)?
            .into_iter()
            .map(|(id, name, _)| (id, name))
            .collect())
    }

    /// Like [`SearchIndex::query`], but each hit carries its raw tantivy BM25 score. Used by the
    /// Distributed-mode gateway (RFC 0113 B5) to merge per-shard top-K lists — the scores are
    /// **shard-local** (per-partition term statistics), the accepted query-then-fetch
    /// approximation, not a global ranking.
    pub fn query_scored(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(Uuid, String, f32)>, LedgerError> {
        Ok(self
            .query_scored_marked(query, limit)?
            .into_iter()
            .map(|h| (h.id, h.name, h.score))
            .collect())
    }

    /// [`SearchIndex::query_scored`], plus whether each hit came from the relaxed pass
    /// (RFC 0139 §3.1) rather than matching every query term.
    ///
    /// Callers that reason about *whether the corpus really answers a question* need this
    /// distinction: a relaxed hit means "this document shares some words with your question", which
    /// is a fine retrieval candidate but is not evidence that the thing asked about exists. Without
    /// it, relaxation turns questions about things that do not exist into confident answers built
    /// from whatever shared a word — measured live, fabrications rose 10 → 15 on the RFC 0138 suite
    /// when relaxation shipped un-marked.
    pub fn query_scored_marked(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ScoredHit>, LedgerError> {
        // `(raw, lowercased, is_prefix)` — the original casing is kept because RFC 0139 §3.2's
        // subword expansion needs case transitions, which lowercasing destroys.
        let terms: Vec<(String, String, bool)> = query
            .split(|c: char| !(c.is_alphanumeric() || c == '*'))
            .filter(|t| !t.is_empty())
            .map(|t| match t.strip_suffix('*') {
                Some(stem) => (stem.to_string(), stem.to_lowercase(), true),
                None => {
                    let t = t.trim_matches('*');
                    (t.to_string(), t.to_lowercase(), false)
                }
            })
            .filter(|(_, t, _)| !t.is_empty())
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        // `name`/`content` are indexed with the `"en_stem"` tokenizer (F7 fix, see the schema
        // comment in `open_impl`) — query terms must be stemmed the same way before becoming a
        // `Term`, or a stemmed-at-index-time token never matches an unstemmed-at-query-time one.
        // `kind` stays on the plain lowercased term (unstemmed at index time too).
        let mut stemmer = self
            .reader
            .searcher()
            .index()
            .tokenizers()
            .get("en_stem")
            .ok_or_else(|| terr("en_stem tokenizer not registered"))?;
        let stem = |analyzer: &mut tantivy::tokenizer::TextAnalyzer, term: &str| -> String {
            let mut stream = analyzer.token_stream(term);
            if stream.advance() {
                stream.token().text.clone()
            } else {
                term.to_string()
            }
        };

        // One term's clause: a disjunction over the three fields, boosted name > kind > content.
        // Built here rather than inline because the relaxed pass below needs the identical clause
        // with only its `Occur` changed — if the two ever drifted, a relaxed hit could score on
        // different criteria than a strict one.
        // RFC 0139 §3.2 — a CamelCase query token is one token, but the identifier it names is
        // usually indexed as several. `SimpleTokenizer` splits `build_llm_provider` on `_`, so the
        // index holds `build`/`llm`/`provider`; a question saying "LlmProvider" produces the single
        // token `llmprovider`, and the two never meet. Measured: searching "LlmProvider" returns
        // only objects literally named that, while "build llm provider" ranks
        // `build_llm_provider` first, second and third.
        //
        // Query-time only, deliberately. The index-side half of §3.2 would change document lengths
        // and term frequencies, shifting BM25 for every query and putting RFC 0126's CI gate at
        // risk with no monotonicity argument available. This has neither cost: no schema change,
        // no reindex, and a term can only match *more* documents than before.
        fn subwords(raw: &str) -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            let mut cur = String::new();
            let mut prev: Option<char> = None;
            for c in raw.chars() {
                let boundary = match prev {
                    // camelCase / XMLHttp / letter->digit transitions all start a new subword.
                    Some(p) => {
                        (p.is_lowercase() && c.is_uppercase())
                            || (p.is_alphabetic() && c.is_ascii_digit())
                            || (p.is_ascii_digit() && c.is_alphabetic())
                    }
                    None => false,
                };
                if boundary && !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur.push(c.to_ascii_lowercase());
                prev = Some(c);
            }
            if !cur.is_empty() {
                out.push(cur);
            }
            // Single-character fragments carry no signal and would loosen the clause for nothing.
            out.retain(|w| w.len() > 1);
            out
        }

        let mut term_clause = |term: &str, prefix: bool| -> Box<dyn Query> {
            let stemmed_term = stem(&mut stemmer, term);
            let mut fields: Vec<(Occur, Box<dyn Query>)> = Vec::new();
            for (field, boost, text) in [
                (self.f_name, 10.0f32, &stemmed_term),
                (self.f_kind, 4.0, &term.to_string()),
                (self.f_content, 1.0, &stemmed_term),
            ] {
                let t = Term::from_field_text(field, text);
                let q: Box<dyn Query> = if prefix {
                    Box::new(PhrasePrefixQuery::new(vec![t]))
                } else {
                    Box::new(TermQuery::new(t, IndexRecordOption::WithFreqs))
                };
                fields.push((Occur::Should, Box::new(BoostQuery::new(q, boost))));
            }
            Box::new(BooleanQuery::new(fields))
        };

        // Either the whole token matches, or *every* one of its subwords does. The inner `Must`
        // matters: an `Or` over subwords would let a document containing only "llm" satisfy the
        // term "LlmProvider", which widens far past the intent.
        let mut expanded = |raw: &str, term: &str, prefix: bool| -> Box<dyn Query> {
            let parts = subwords(raw);
            if parts.len() < 2 {
                return term_clause(term, prefix);
            }
            let all_parts: Vec<(Occur, Box<dyn Query>)> = parts
                .iter()
                .map(|w| (Occur::Must, term_clause(w, false)))
                .collect();
            Box::new(BooleanQuery::new(vec![
                (Occur::Should, term_clause(term, prefix)),
                (Occur::Should, Box::new(BooleanQuery::new(all_parts))),
            ]))
        };

        let mut must: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for (raw, term, prefix) in &terms {
            must.push((Occur::Must, expanded(raw, term, *prefix)));
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
        let memory_boost: Box<dyn Query> = Box::new(BoostQuery::new(
            Box::new(TermQuery::new(
                Term::from_field_text(self.f_memory_path, MEMORY_PATH_TOKEN),
                IndexRecordOption::Basic,
            )),
            MEMORY_PATH_BOOST,
        ));
        must.push((Occur::Should, memory_boost));

        let searcher = self.reader.searcher();
        let n_terms = terms.len();
        // A strict hit matched every term by construction — that is what `Occur::Must` means.
        let mut out: Vec<ScoredHit> = self
            .run_query(&searcher, BooleanQuery::new(must), limit)?
            .into_iter()
            .map(|(id, name, score)| ScoredHit {
                id,
                name,
                score,
                matched_terms: n_terms,
                total_terms: n_terms,
            })
            .collect();

        // RFC 0139 §3.1 — progressive relaxation. Every term above is `Occur::Must`, so a
        // natural-language question ("what crate implements the SQL DDL recovery analyzer?")
        // requires *every* content word to co-occur in one document. Measured on the RFC 0138
        // suite, that returned nothing at all for 80 of 89 scenarios, leaving the answer pipeline
        // with only graph-neighbourhood filler to reason from.
        //
        // The relaxed pass is **append-only**: strict hits keep their exact order and scores, and
        // OR-matched hits are appended strictly below them. Recall@k, MRR and nDCG@k over this
        // list are therefore monotonically non-decreasing versus the strict-only behaviour — which
        // is what lets this ship without re-baselining RFC 0126's CI gate. (tantivy 0.22 has no
        // `minimum_number_should_match`, so a coverage-ratio query isn't available here.)
        if out.len() < limit && terms.len() > 1 {
            let mut should: Vec<(Occur, Box<dyn Query>)> = Vec::new();
            for (raw, term, prefix) in &terms {
                should.push((Occur::Should, expanded(raw, term, *prefix)));
            }
            let relaxed = self.run_query(&searcher, BooleanQuery::new(should), limit)?;

            let seen: std::collections::HashSet<Uuid> = out.iter().map(|h| h.id).collect();

            // RFC 0139 §3.7 — how many terms each relaxed hit actually matched. One small BM25
            // lookup per term (~2ms each on this corpus, against a pipeline that spends seconds in
            // the LLM), which buys a graded signal instead of a binary "was relaxed" flag. Ranking
            // is untouched: this only annotates the hits the OR pass already chose.
            let mut coverage: std::collections::HashMap<Uuid, usize> =
                std::collections::HashMap::new();
            for (raw, term, prefix) in &terms {
                let q = BooleanQuery::new(vec![(Occur::Must, expanded(raw, term, *prefix))]);
                for (id, _, _) in self.run_query(&searcher, q, limit.saturating_mul(4))? {
                    *coverage.entry(id).or_insert(0) += 1;
                }
            }

            // Anchor the appended band strictly below the weakest strict hit so the result list
            // stays strictly decreasing (`retrieval::RankedResults` documents that invariant).
            let floor = out.last().map(|h| h.score).unwrap_or(f32::MAX);
            for (i, (id, name, _)) in relaxed.into_iter().enumerate() {
                if out.len() >= limit {
                    break;
                }
                if seen.contains(&id) {
                    continue;
                }
                let score = if floor == f32::MAX {
                    // No strict hits at all — nothing to sit below, so keep a plain descending
                    // band rather than inventing a relationship to a score that doesn't exist.
                    1.0 / (i + 1) as f32
                } else {
                    floor * (0.999 - (i as f32 * 1e-4)).max(0.0)
                };
                out.push(ScoredHit {
                    matched_terms: coverage.get(&id).copied().unwrap_or(1).min(n_terms),
                    total_terms: n_terms,
                    id,
                    name,
                    score,
                });
            }
        }
        Ok(out)
    }

    /// Execute one built query and hydrate `(id, name, score)` triples, best first.
    fn run_query(
        &self,
        searcher: &tantivy::Searcher,
        query: BooleanQuery,
        limit: usize,
    ) -> Result<Vec<(Uuid, String, f32)>, LedgerError> {
        let top = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(terr)?;
        let mut out = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr).map_err(terr)?;
            let get = |f: Field| {
                doc.get_first(f)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            if let Ok(id) = get(self.f_id).parse::<Uuid>() {
                out.push((id, get(self.f_name), score));
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

    /// RFC 0139 §3.1 — the relaxation pass.
    mod relaxation {
        use super::*;

        fn index_with(objects: &[(&str, &str)]) -> (tempfile::TempDir, SearchIndex) {
            let dir = tempfile::tempdir().unwrap();
            let (mut index, _) = SearchIndex::open(dir.path()).unwrap();
            for (name, content) in objects {
                index.upsert(Uuid::new_v4(), name, "Doc", content, false);
            }
            index.commit(Some(crate::fact::TxId(1))).unwrap();
            (dir, index)
        }

        #[test]
        fn a_multi_term_question_no_longer_returns_nothing() {
            // The measured pathology: every term was `Occur::Must`, so a natural-language
            // question found nothing unless one document contained *every* content word.
            let (_d, index) =
                index_with(&[("sql_analyzer", "parses SQL DDL into structural form")]);
            let hits = index
                .query_scored("what crate implements the sql ddl recovery analyzer", 10)
                .unwrap();
            assert!(
                !hits.is_empty(),
                "a question whose terms don't all co-occur must still retrieve the \
                 relevant document; got nothing"
            );
            assert_eq!(hits[0].1, "sql_analyzer");
        }

        #[test]
        fn strict_hits_keep_their_rank_and_outrank_every_relaxed_hit() {
            // The safety property the RFC 0126 gate rests on: relaxation is append-only, so a
            // document matching all terms can never be displaced by one matching only some.
            let (_d, index) = index_with(&[
                ("alpha", "widget gadget"),   // matches both terms
                ("beta", "widget only here"), // matches one
                ("gamma", "gadget only here"),
            ]);
            let hits = index.query_scored("widget gadget", 10).unwrap();
            assert_eq!(
                hits[0].1, "alpha",
                "the document matching every term must rank first"
            );
            assert!(hits.len() > 1, "relaxed hits should still be appended");
            for w in hits.windows(2) {
                assert!(
                    w[0].2 >= w[1].2,
                    "scores must stay non-increasing so RankedResults' invariant holds: {hits:?}"
                );
            }
        }

        #[test]
        fn a_single_term_query_is_untouched_by_relaxation() {
            // Nothing to relax with one term — this guards against the relaxed pass firing and
            // widening a query that was already precise.
            let (_d, index) = index_with(&[("alpha", "widget"), ("beta", "unrelated")]);
            let hits = index.query_scored("widget", 10).unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].1, "alpha");
        }

        /// RFC 0139 §3.7 — coverage is the graded signal a binary relaxed/strict flag could not
        /// give. These two cases are the ones that matter: a hit sharing most of the query is
        /// usable evidence, one sharing a single common word is not.
        #[test]
        fn coverage_separates_a_near_miss_from_vocabulary_noise() {
            let (_d, index) = index_with(&[
                ("alpha", "widget gadget sprocket"),  // 3 of 4 terms
                ("beta", "widget unrelated content"), // 1 of 4
            ]);
            let hits = index
                .query_scored_marked("widget gadget sprocket flange", 10)
                .unwrap();
            let by_name = |n: &str| hits.iter().find(|h| h.name == n).unwrap().clone();

            let near = by_name("alpha");
            assert_eq!((near.matched_terms, near.total_terms), (3, 4));
            assert!(
                near.coverage() >= crate::WEAK_COVERAGE,
                "a 3-of-4 match must count as real evidence, got {}",
                near.coverage()
            );

            let noise = by_name("beta");
            assert_eq!(noise.matched_terms, 1);
            assert!(
                noise.coverage() < crate::WEAK_COVERAGE,
                "a 1-of-4 match must not count as evidence, got {}",
                noise.coverage()
            );
        }

        /// RFC 0139 §3.2 — the measured failure: a question saying "LlmProvider" produced the
        /// single token `llmprovider`, while the identifier it names is indexed as
        /// `build`/`llm`/`provider`. The two never met, so the function ranked nowhere while
        /// objects literally named `LlmProvider` took the top slots.
        #[test]
        fn a_camelcase_query_token_finds_the_underscored_identifier() {
            let (_d, index) = index_with(&[
                ("build_llm_provider", "constructs the provider"),
                ("LlmProvider", "the provider trait"),
            ]);
            let names: Vec<String> = index
                .query_scored("LlmProvider", 10)
                .unwrap()
                .into_iter()
                .map(|(_, n, _)| n)
                .collect();
            assert!(
                names.iter().any(|n| n == "build_llm_provider"),
                "subword expansion should reach the underscored identifier: {names:?}"
            );
        }

        #[test]
        fn subword_expansion_requires_every_part_not_just_one() {
            // An `Or` over subwords would let a document containing only "llm" satisfy
            // "LlmProvider". The inner `Must` is what keeps the expansion honest.
            let (_d, index) = index_with(&[("llm_only", "llm and nothing else here")]);
            let names: Vec<String> = index
                .query_scored("LlmProvider", 10)
                .unwrap()
                .into_iter()
                .map(|(_, n, _)| n)
                .collect();
            assert!(
                !names.iter().any(|n| n == "llm_only"),
                "a document with only one subword must not match: {names:?}"
            );
        }

        #[test]
        fn a_strict_hit_always_reports_full_coverage() {
            let (_d, index) = index_with(&[("alpha", "widget gadget")]);
            let hits = index.query_scored_marked("widget gadget", 10).unwrap();
            assert_eq!(hits[0].matched_terms, hits[0].total_terms);
            assert_eq!(hits[0].coverage(), 1.0);
        }

        #[test]
        fn relaxation_does_not_duplicate_a_document_already_matched_strictly() {
            let (_d, index) = index_with(&[("alpha", "widget gadget")]);
            let hits = index.query_scored("widget gadget", 10).unwrap();
            assert_eq!(
                hits.len(),
                1,
                "the strict hit must not reappear as a relaxed one: {hits:?}"
            );
        }
    }
}
