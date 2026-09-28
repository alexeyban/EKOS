//! RFC 0016 Phase 3 — derived index runs in three sort orders.
//!
//! Segments are the truth (Phase 2); indexes are **derived and rebuildable**.
//! Each sealed segment's facts sort into per-segment *runs* — immutable files
//! of key-ordered entries — in three covering sort orders:
//!
//! | Sort | Key | Answers |
//! |------|-----|---------|
//! | EAVT | (entity, attr, pos, tx) | an entity's facts → object reconstruction, `object_at` |
//! | AEVT | (attr, entity, pos, tx) | all entities carrying an attribute → `WHERE kind = …` |
//! | AVET | (attr, value, entity, pos, tx) | entities by attribute *value* — **ref values only** (graph hops); scalar lookups ride AEVT or tantivy |
//!
//! Reads issue **prefix ranged scans**: the key prefix (an entity, an
//! attribute, an attribute+value) selects a contiguous key range, blocks
//! outside it are pruned via the run's block directory, and results from
//! multiple runs merge in key order — an LSM read path. `merge_runs`
//! compacts a sort order's runs into one (build-time, never a background
//! daemon — determinism rule).
//!
//! Keys are **order-preserving byte strings**: fixed-width big-endian fields,
//! and the one variable-length field (the AVET value key) is
//! `0x00`-escape-terminated so tuple ordering survives concatenation.
//! AVET value keys are typed-tag + canonical bytes: exact-match and prefix
//! scans are supported for every value type; *numeric range* scans are a
//! documented non-goal of v1 (numbers order by their lexical form).
//!
//! Run files are blocks (zstd level 19) plus a block directory (first/last
//! key per block) and a fixed footer. Format v2 (RFC 0016 §7): blocks store
//! **explicit prefix-delta-encoded keys** and compact binary entries, and
//! only EAVT bodies carry values — reconstruction needs them; AEVT/AVET
//! reads consume the entity id (the AVET value lives in the key), so their
//! bodies are slim and their hydrated scan results carry `FactValue::Null`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::fact::{AttrId, Fact, FactOp, FactValue, TxId};
use crate::segment::{Batch, SegmentError};

/// Entries per zstd block inside a run file.
const BLOCK_ENTRIES: usize = 512;
const RUN_MAGIC: u32 = 0x454B_4953; // "EKIS" — format v2: explicit prefix-delta keys, slim projections
/// Run-block compression. Was 19 on the reasoning that runs are "written once — spend effort
/// there", but a merge rewrites every run inline in whichever `commit` crosses `MERGE_RUNS_AT`.
/// Measured on EKOS's own 8 EAVT runs (RFC 0168, 2026-09-28): level 19 → 150 s / 36 MB,
/// level 9 → 4.5 s / 37 MB, level 3 → 1.4 s / 38 MB, point reads unchanged. Level 9 buys a 33×
/// faster write for 3% more disk.
const RUN_ZSTD_LEVEL: i32 = 9;

/// The three covering sort orders (RFC 0016 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SortOrder {
    Eavt,
    Aevt,
    Avet,
}

impl SortOrder {
    pub const ALL: [SortOrder; 3] = [SortOrder::Eavt, SortOrder::Aevt, SortOrder::Avet];

    fn prefix(self) -> &'static str {
        match self {
            SortOrder::Eavt => "eavt",
            SortOrder::Aevt => "aevt",
            SortOrder::Avet => "avet",
        }
    }
}

/// One indexed fact occurrence — the full covering entry, so no read ever
/// needs to go back to the segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub entity: Uuid,
    pub attr: AttrId,
    pub pos: Option<u32>,
    pub tx: TxId,
    pub op: FactOp,
    pub value: FactValue,
}

impl IndexEntry {
    pub fn from_fact(fact: &Fact, tx: TxId, op: FactOp) -> Self {
        Self {
            entity: fact.entity,
            attr: fact.attr,
            pos: fact.pos,
            tx,
            op,
            value: fact.value.clone(),
        }
    }
}

/// Flatten committed batches into index entries.
pub fn entries_from_batches<'a>(batches: impl IntoIterator<Item = &'a Batch>) -> Vec<IndexEntry> {
    batches
        .into_iter()
        .flat_map(|b| {
            b.ops
                .iter()
                .map(|(op, f)| IndexEntry::from_fact(f, b.tx, *op))
        })
        .collect()
}

// ── Order-preserving key encoding ───────────────────────────────────────────

/// Escape-terminate a variable-length byte field so lexicographic order of
/// the concatenated key equals tuple order: `0x00` → `0x00 0xFF`, terminated
/// by `0x00 0x00`.
fn push_escaped(out: &mut Vec<u8>, bytes: &[u8]) {
    for &b in bytes {
        out.push(b);
        if b == 0 {
            out.push(0xFF);
        }
    }
    out.push(0x00);
    out.push(0x00);
}

/// Typed, canonical value bytes for AVET keys. Equality/prefix scans only —
/// numbers compare by lexical form, not magnitude (v1 non-goal).
fn value_order_key(value: &FactValue) -> Vec<u8> {
    let mut out = Vec::new();
    match value {
        FactValue::Null => out.push(0),
        FactValue::Bool(false) => out.push(1),
        FactValue::Bool(true) => out.push(2),
        FactValue::Number(n) => {
            out.push(3);
            out.extend_from_slice(n.to_string().as_bytes());
        }
        FactValue::String(s) => {
            out.push(4);
            out.extend_from_slice(s.as_bytes());
        }
        FactValue::Ref(u) => {
            out.push(5);
            out.extend_from_slice(u.as_bytes());
        }
        FactValue::Composite(v) => {
            out.push(6);
            out.extend_from_slice(
                serde_json::to_vec(v)
                    .expect("composite serializes")
                    .as_slice(),
            );
        }
    }
    out
}

fn push_pos(out: &mut Vec<u8>, pos: Option<u32>) {
    match pos {
        None => out.extend_from_slice(&[0, 0, 0, 0, 0]),
        Some(p) => {
            out.push(1);
            out.extend_from_slice(&p.to_be_bytes());
        }
    }
}

/// The sort key of an entry under a given order.
pub fn encode_key(order: SortOrder, e: &IndexEntry) -> Vec<u8> {
    let mut k = Vec::with_capacity(48);
    match order {
        SortOrder::Eavt => {
            k.extend_from_slice(e.entity.as_bytes());
            k.extend_from_slice(&e.attr.0.to_be_bytes());
        }
        SortOrder::Aevt => {
            k.extend_from_slice(&e.attr.0.to_be_bytes());
            k.extend_from_slice(e.entity.as_bytes());
        }
        SortOrder::Avet => {
            k.extend_from_slice(&e.attr.0.to_be_bytes());
            push_escaped(&mut k, &value_order_key(&e.value));
            k.extend_from_slice(e.entity.as_bytes());
        }
    }
    push_pos(&mut k, e.pos);
    k.extend_from_slice(&e.tx.0.to_be_bytes());
    k
}

/// Scan prefixes: each selects a contiguous key range under its sort order.
#[derive(Debug, Clone)]
pub enum ScanPrefix {
    /// EAVT: everything about one entity (optionally one attribute).
    Entity { entity: Uuid, attr: Option<AttrId> },
    /// AEVT: every entity carrying an attribute.
    Attr { attr: AttrId },
    /// AVET: every entity whose attribute has exactly this value.
    AttrValue { attr: AttrId, value: FactValue },
}

impl ScanPrefix {
    fn order(&self) -> SortOrder {
        match self {
            ScanPrefix::Entity { .. } => SortOrder::Eavt,
            ScanPrefix::Attr { .. } => SortOrder::Aevt,
            ScanPrefix::AttrValue { .. } => SortOrder::Avet,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut k = Vec::new();
        match self {
            ScanPrefix::Entity { entity, attr } => {
                k.extend_from_slice(entity.as_bytes());
                if let Some(a) = attr {
                    k.extend_from_slice(&a.0.to_be_bytes());
                }
            }
            ScanPrefix::Attr { attr } => k.extend_from_slice(&attr.0.to_be_bytes()),
            ScanPrefix::AttrValue { attr, value } => {
                k.extend_from_slice(&attr.0.to_be_bytes());
                push_escaped(&mut k, &value_order_key(value));
            }
        }
        k
    }
}

fn in_prefix(key: &[u8], prefix: &[u8]) -> bool {
    key.len() >= prefix.len() && &key[..prefix.len()] == prefix
}

// ── Run files ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct BlockMeta {
    offset: u64,
    len: u32,
    count: u32,
    /// Hex of the block's first/last key (keys are recomputed from entries
    /// on read; the directory only exists for pruning).
    first_key: String,
    last_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RunDirectory {
    order: SortOrder,
    entry_count: u64,
    blocks: Vec<BlockMeta>,
    /// RFC 0168: the entities this (EAVT) run holds, so an entity scan skips a run that cannot
    /// match without reading a block. `None` for runs written before RFC 0168 and for the other
    /// orders — such a run is always probed, exactly as before. Older binaries ignore the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entity_filter: Option<EntityFilter>,
}

// ── RFC 0168: per-run entity filter ─────────────────────────────────────────

/// Bits per distinct entity. At `FILTER_PROBES` = 7 this is ~1% false positives; a false positive
/// only costs the block read every probe paid before this filter existed.
const FILTER_BITS_PER_ENTITY: usize = 10;
const FILTER_PROBES: u64 = 7;

/// A Bloom filter over entity ids, in-tree and deterministic (RFC 0168 open question 1): the same
/// entities always produce the same bits, so a rewritten run is byte-identical.
///
/// Entity ids are UUIDs — v4 random or v5 name-hashed — so their bytes are already close to
/// uniform. They are still passed through a SplitMix64 finalizer, because a UUID's version and
/// variant nibbles are fixed and would otherwise bias two of the probe positions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct EntityFilter {
    /// Hex of the little-endian bit words: compact in the zstd'd JSON directory, no new codec.
    bits: String,
    #[serde(skip)]
    words: Vec<u64>,
}

fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl EntityFilter {
    fn build<'a>(entities: impl Iterator<Item = &'a Uuid>, distinct_hint: usize) -> Self {
        let bits = (distinct_hint.max(1) * FILTER_BITS_PER_ENTITY).max(64);
        let mut words = vec![0u64; bits.div_ceil(64)];
        for e in entities {
            for bit in Self::probes(e, words.len() as u64 * 64) {
                words[(bit / 64) as usize] |= 1 << (bit % 64);
            }
        }
        let mut bytes = Vec::with_capacity(words.len() * 8);
        for w in &words {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        Self {
            bits: hex::encode(bytes),
            words,
        }
    }

    /// Kirsch–Mitzenmacher double hashing: probe `i` is `h1 + i·h2`.
    fn probes(entity: &Uuid, nbits: u64) -> impl Iterator<Item = u64> {
        let b = entity.as_bytes();
        let h1 = mix64(u64::from_le_bytes(b[0..8].try_into().expect("8 bytes")));
        let h2 = mix64(u64::from_le_bytes(b[8..16].try_into().expect("8 bytes"))) | 1;
        (0..FILTER_PROBES).map(move |i| h1.wrapping_add(i.wrapping_mul(h2)) % nbits)
    }

    /// Rebuild `words` after deserialization. A malformed field disables filtering for the run
    /// (always probe) rather than failing the open: the filter is an optimization, never truth.
    fn hydrate(mut self) -> Option<Self> {
        let bytes = hex::decode(&self.bits).ok()?;
        if bytes.is_empty() || bytes.len() % 8 != 0 {
            return None;
        }
        self.words = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| u64::from_le_bytes(*c))
            .collect();
        Some(self)
    }

    fn may_contain(&self, entity: &Uuid) -> bool {
        let nbits = self.words.len() as u64 * 64;
        Self::probes(entity, nbits)
            .all(|bit| self.words[(bit / 64) as usize] & (1 << (bit % 64)) != 0)
    }
}

/// Whether a sort order stores values in run bodies. Only EAVT is covering —
/// reconstruction needs values. AEVT/AVET reads consume the *entity* (the
/// AVET value lives in the key), so their bodies are slim (RFC 0016 §7);
/// their hydrated scan results carry `FactValue::Null`.
fn stores_values(order: SortOrder) -> bool {
    matches!(order, SortOrder::Eavt)
}

/// A raw run record: the (explicit) sort key plus the entry, whose value may
/// be dropped for slim orders.
type RawRecord = (Vec<u8>, IndexEntry);

fn project(order: SortOrder, mut e: IndexEntry) -> Option<RawRecord> {
    // AVET exists for value → entity lookups, and the only value-shaped
    // lookup any read path issues is the graph hop (`from`/`to` = Ref).
    // Indexing scalar/composite values would put 600-char excerpts *inside
    // keys* (RFC 0016 §7 measurement: ~20 MB of AVET on the live estate for
    // zero reads). Scalar value queries go through AEVT + filter or tantivy.
    if matches!(order, SortOrder::Avet) && !matches!(e.value, FactValue::Ref(_)) {
        return None;
    }
    let key = encode_key(order, &e);
    if !stores_values(order) {
        e.value = FactValue::Null;
    }
    Some((key, e))
}

/// Write one immutable run file from entries (sorted and projected
/// internally per the order's storage rule).
pub fn write_run(
    path: &Path,
    order: SortOrder,
    entries: Vec<IndexEntry>,
) -> Result<(), SegmentError> {
    let mut raws: Vec<RawRecord> = entries
        .into_iter()
        .filter_map(|e| project(order, e))
        .collect();
    raws.sort_by(|a, b| a.0.cmp(&b.0));
    write_run_raw(path, order, &raws)
}

fn write_run_raw(path: &Path, order: SortOrder, raws: &[RawRecord]) -> Result<(), SegmentError> {
    write_run_raw_with(path, order, raws, true)
}

/// `with_filter: false` writes a run exactly as a pre-RFC 0168 binary did — tests use it to prove
/// mixed old/new run sets still scan correctly.
fn write_run_raw_with(
    path: &Path,
    order: SortOrder,
    raws: &[RawRecord],
    with_filter: bool,
) -> Result<(), SegmentError> {
    let mut file = File::create(path)?;
    let mut blocks = Vec::new();
    let mut offset = 0u64;
    for chunk in raws.chunks(BLOCK_ENTRIES) {
        let body = zstd::encode_all(&encode_block(order, chunk)?[..], RUN_ZSTD_LEVEL)?;
        file.write_all(&body)?;
        blocks.push(BlockMeta {
            offset,
            len: body.len() as u32,
            count: chunk.len() as u32,
            first_key: hex::encode(&chunk[0].0),
            last_key: hex::encode(&chunk.last().unwrap().0),
        });
        offset += body.len() as u64;
    }

    // EAVT keys sort by entity first, so distinct entities are the runs of equal ids.
    let entity_filter =
        (with_filter && matches!(order, SortOrder::Eavt) && !raws.is_empty()).then(|| {
            let mut distinct: Vec<&Uuid> = raws.iter().map(|(_, e)| &e.entity).collect();
            distinct.dedup();
            EntityFilter::build(distinct.iter().copied(), distinct.len())
        });
    let dir = RunDirectory {
        order,
        entry_count: raws.len() as u64,
        blocks,
        entity_filter,
    };
    let dir_json = serde_json::to_vec(&dir)?;
    let dir_body = zstd::encode_all(&dir_json[..], ekos_common::compress::ZSTD_LEVEL)?;
    file.write_all(&dir_body)?;
    file.write_all(&offset.to_le_bytes())?;
    file.write_all(&(dir_body.len() as u32).to_le_bytes())?;
    file.write_all(&RUN_MAGIC.to_le_bytes())?;
    file.sync_all()?;
    Ok(())
}

// ── Binary block codec: prefix-delta keys + compact entries ────────────────

fn encode_block(order: SortOrder, chunk: &[RawRecord]) -> Result<Vec<u8>, SegmentError> {
    let mut out = Vec::new();
    out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
    let mut prev: &[u8] = &[];
    for (key, e) in chunk {
        let shared = prev
            .iter()
            .zip(key.iter())
            .take_while(|(a, b)| a == b)
            .count()
            .min(u16::MAX as usize);
        let suffix = &key[shared..];
        out.extend_from_slice(&(shared as u16).to_le_bytes());
        out.extend_from_slice(&(suffix.len() as u16).to_le_bytes());
        out.extend_from_slice(suffix);
        prev = key;

        out.extend_from_slice(e.entity.as_bytes());
        out.extend_from_slice(&e.attr.0.to_le_bytes());
        match e.pos {
            None => out.push(0),
            Some(p) => {
                out.push(1);
                out.extend_from_slice(&p.to_le_bytes());
            }
        }
        out.extend_from_slice(&e.tx.0.to_le_bytes());
        out.push(matches!(e.op, FactOp::Retract) as u8);
        if stores_values(order) {
            let v = serde_json::to_vec(&e.value)?;
            out.extend_from_slice(&(v.len() as u32).to_le_bytes());
            out.extend_from_slice(&v);
        }
    }
    Ok(out)
}

/// What [`decode_block_where`] does with one record, decided from its key alone.
enum Keep {
    Yes,
    No,
    /// Keys are sorted: nothing after this one can match either.
    Stop,
}

/// Decode a block, materializing only the records `keep` accepts.
///
/// RFC 0168: a point scan used to parse the JSON value of all 512 records in a block to return a
/// handful. Every key must still be rebuilt, because each one is delta-encoded against the last,
/// but a skipped record's value bytes are stepped over, never parsed.
fn decode_block_where(
    order: SortOrder,
    bytes: &[u8],
    keep: impl Fn(&[u8]) -> Keep,
) -> Result<Vec<RawRecord>, SegmentError> {
    fn corrupt(m: &str) -> SegmentError {
        SegmentError::Corrupt(format!("run block: {m}"))
    }
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], SegmentError> {
        let end = at.checked_add(n).ok_or_else(|| corrupt("overflow"))?;
        let s = bytes.get(at..end).ok_or_else(|| corrupt("truncated"))?;
        at = end;
        Ok(s)
    };
    let count = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
    let mut out = Vec::new();
    let mut key: Vec<u8> = Vec::new();
    for _ in 0..count {
        let shared = u16::from_le_bytes(take(2)?.try_into().unwrap()) as usize;
        let suffix_len = u16::from_le_bytes(take(2)?.try_into().unwrap()) as usize;
        if shared > key.len() {
            return Err(corrupt("bad shared prefix"));
        }
        key.truncate(shared);
        key.extend_from_slice(take(suffix_len)?);

        let wanted = match keep(&key) {
            Keep::Stop => break,
            Keep::Yes => true,
            Keep::No => false,
        };
        if !wanted {
            // Step over the fixed-width fields, the optional pos and the value.
            take(16 + 4)?;
            if take(1)?[0] != 0 {
                take(4)?;
            }
            take(8 + 1)?;
            if stores_values(order) {
                let vlen = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
                take(vlen)?;
            }
            continue;
        }

        let entity = Uuid::from_slice(take(16)?).map_err(|_| corrupt("bad uuid"))?;
        let attr = AttrId(u32::from_le_bytes(take(4)?.try_into().unwrap()));
        let pos = match take(1)?[0] {
            0 => None,
            _ => Some(u32::from_le_bytes(take(4)?.try_into().unwrap())),
        };
        let tx = TxId(u64::from_le_bytes(take(8)?.try_into().unwrap()));
        let op = if take(1)?[0] == 1 {
            FactOp::Retract
        } else {
            FactOp::Assert
        };
        let value = if stores_values(order) {
            let vlen = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
            serde_json::from_slice(take(vlen)?)?
        } else {
            FactValue::Null
        };
        out.push((
            key.clone(),
            IndexEntry {
                entity,
                attr,
                pos,
                tx,
                op,
                value,
            },
        ));
    }
    Ok(out)
}

/// An open run file: block directory in memory, blocks read on demand.
pub struct IndexRun {
    path: PathBuf,
    dir: RunDirectory,
    /// Blocks read so far — lets a test prove the filter *skips* reads, not just that results match.
    #[cfg(test)]
    blocks_read: std::sync::atomic::AtomicUsize,
}

impl IndexRun {
    pub fn open(path: &Path) -> Result<Self, SegmentError> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        if len < 16 {
            return Err(SegmentError::Corrupt(format!(
                "run file {} too short",
                path.display()
            )));
        }
        let mut footer = [0u8; 16];
        file.seek(SeekFrom::Start(len - 16))?;
        file.read_exact(&mut footer)?;
        let dir_offset = u64::from_le_bytes(footer[0..8].try_into().unwrap());
        let dir_len = u32::from_le_bytes(footer[8..12].try_into().unwrap());
        let magic = u32::from_le_bytes(footer[12..16].try_into().unwrap());
        if magic != RUN_MAGIC || dir_offset + dir_len as u64 + 16 != len {
            return Err(SegmentError::Corrupt(format!(
                "run file {} has a bad footer",
                path.display()
            )));
        }

        let mut body = vec![0u8; dir_len as usize];
        file.seek(SeekFrom::Start(dir_offset))?;
        file.read_exact(&mut body)?;
        let dir_json = zstd::decode_all(&body[..])?;
        let mut dir: RunDirectory = serde_json::from_slice(&dir_json)?;
        dir.entity_filter = dir.entity_filter.take().and_then(EntityFilter::hydrate);
        Ok(Self {
            path: path.to_path_buf(),
            dir,
            #[cfg(test)]
            blocks_read: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub fn order(&self) -> SortOrder {
        self.dir.order
    }

    pub fn entry_count(&self) -> u64 {
        self.dir.entry_count
    }

    fn read_block_raw(&self, meta: &BlockMeta) -> Result<Vec<RawRecord>, SegmentError> {
        self.read_block_where(meta, |_| Keep::Yes)
    }

    fn read_block_where(
        &self,
        meta: &BlockMeta,
        keep: impl Fn(&[u8]) -> Keep,
    ) -> Result<Vec<RawRecord>, SegmentError> {
        #[cfg(test)]
        self.blocks_read
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut file = File::open(&self.path)?;
        let mut body = vec![0u8; meta.len as usize];
        file.seek(SeekFrom::Start(meta.offset))?;
        file.read_exact(&mut body)?;
        let bytes = zstd::decode_all(&body[..])?;
        decode_block_where(self.dir.order, &bytes, keep)
    }

    /// All entries whose (stored) key starts with `prefix`, in key order.
    /// Blocks whose key span cannot intersect the prefix range are never
    /// read. Slim orders hydrate with `FactValue::Null` (see
    /// [`stores_values`]).
    fn scan(&self, prefix: &[u8]) -> Result<Vec<IndexEntry>, SegmentError> {
        // RFC 0168: an EAVT prefix starts with the entity. If the run's filter rules the entity
        // out, no block can match — skip the read. Every run used to pay one block decode here
        // (ids are random, so some block always straddles the id).
        if let (SortOrder::Eavt, Some(filter), Some(entity)) = (
            self.dir.order,
            &self.dir.entity_filter,
            prefix.get(..16).and_then(|b| Uuid::from_slice(b).ok()),
        ) && !filter.may_contain(&entity)
        {
            return Ok(Vec::new());
        }
        // Hex encoding is order-preserving, so string comparison over the
        // directory's hex keys equals byte-key comparison.
        let hex_prefix = hex::encode(prefix);
        let mut out = Vec::new();
        // Blocks are in key order: jump to the first that does not end before the prefix
        // (RFC 0168 §4) instead of walking the directory from the start.
        let first = self
            .dir
            .blocks
            .partition_point(|meta| meta.last_key.as_str() < hex_prefix.as_str());
        for meta in &self.dir.blocks[first..] {
            // Any key > prefix that doesn't start with it is ≥ the prefix's
            // successor — this block (and all later ones) starts past the range.
            if meta.first_key.as_str() > hex_prefix.as_str()
                && !meta.first_key.starts_with(&hex_prefix)
            {
                break;
            }
            let keep = |key: &[u8]| {
                if in_prefix(key, prefix) {
                    Keep::Yes
                } else if key > prefix {
                    Keep::Stop // sorted: past the prefix range for good
                } else {
                    Keep::No
                }
            };
            out.extend(
                self.read_block_where(meta, keep)?
                    .into_iter()
                    .map(|(_, e)| e),
            );
        }
        Ok(out)
    }

    /// Every raw record in the run, in key order (merge input).
    fn all_raw(&self) -> Result<Vec<RawRecord>, SegmentError> {
        let mut out = Vec::with_capacity(self.dir.entry_count as usize);
        for meta in &self.dir.blocks {
            out.extend(self.read_block_raw(meta)?);
        }
        Ok(out)
    }

    /// Every entry in the run, in key order.
    pub fn all(&self) -> Result<Vec<IndexEntry>, SegmentError> {
        Ok(self.all_raw()?.into_iter().map(|(_, e)| e).collect())
    }
}

// ── The index set: runs per sort order + merge ──────────────────────────────

/// All index runs of a store, grouped by sort order. Lives in
/// `<root>/indexes/`; entirely derived — deleting the directory and calling
/// [`FactIndexes::build_from_batches`] loses nothing.
pub struct FactIndexes {
    dir: PathBuf,
    runs: HashMap<SortOrder, Vec<IndexRun>>,
}

impl FactIndexes {
    /// Open all run files present under `dir` (created if missing).
    /// Returns the index set plus a `clean` flag: `false` means one or more
    /// run files were unreadable (e.g. an older format after an upgrade) and
    /// were deleted — the caller must rebuild from segment truth (runs are
    /// derived, so nothing is lost).
    pub fn open(dir: impl Into<PathBuf>) -> Result<(Self, bool), SegmentError> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let mut clean = true;
        let mut runs: HashMap<SortOrder, Vec<IndexRun>> = HashMap::new();
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("run"))
            .collect();
        paths.sort();
        for path in paths {
            match IndexRun::open(&path) {
                Ok(run) => runs.entry(run.order()).or_default().push(run),
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        "unreadable index run ({e}); deleting — derived data, will rebuild"
                    );
                    let _ = std::fs::remove_file(&path);
                    clean = false;
                }
            }
        }
        Ok((Self { dir, runs }, clean))
    }

    /// Index one batch group (typically: one sealed segment) as a new run in
    /// every sort order. `name` keys the run files (e.g. the segment seq).
    pub fn add_runs(&mut self, name: &str, entries: &[IndexEntry]) -> Result<(), SegmentError> {
        for order in SortOrder::ALL {
            let path = self.dir.join(format!("{}-{name}.run", order.prefix()));
            write_run(&path, order, entries.to_vec())?;
            self.runs
                .entry(order)
                .or_default()
                .push(IndexRun::open(&path)?);
        }
        Ok(())
    }

    /// Rebuild everything from segment batches (the recovery path that makes
    /// the whole directory disposable).
    pub fn build_from_batches(&mut self, batches: &[Batch]) -> Result<(), SegmentError> {
        for run in self.runs.values().flatten() {
            let _ = std::fs::remove_file(&run.path);
        }
        self.runs.clear();
        self.add_runs("rebuild", &entries_from_batches(batches))
    }

    /// Prefix scan, k-way merged across the order's runs in key order.
    pub fn scan(&self, prefix: &ScanPrefix) -> Result<Vec<IndexEntry>, SegmentError> {
        let order = prefix.order();
        let bytes = prefix.bytes();
        let mut out = Vec::new();
        for run in self.runs.get(&order).map(Vec::as_slice).unwrap_or(&[]) {
            out.extend(run.scan(&bytes)?);
        }
        out.sort_by_key(|e| encode_key(order, e));
        Ok(out)
    }

    /// LSM compaction: collapse all runs of `order` into one. Build-time
    /// maintenance, deterministic, no background threads.
    pub fn merge_runs(&mut self, order: SortOrder) -> Result<(), SegmentError> {
        let runs = self.runs.remove(&order).unwrap_or_default();
        if runs.len() <= 1 {
            self.runs.insert(order, runs);
            return Ok(());
        }
        let mut raws = Vec::new();
        for run in &runs {
            raws.extend(run.all_raw()?);
        }
        raws.sort_by(|a, b| a.0.cmp(&b.0));
        let merged_path = self.dir.join(format!("{}-merged.run", order.prefix()));
        let tmp = self.dir.join(format!("{}-merged.run.tmp", order.prefix()));
        write_run_raw(&tmp, order, &raws)?;
        for run in &runs {
            let _ = std::fs::remove_file(&run.path);
        }
        std::fs::rename(&tmp, &merged_path)?;
        self.runs.insert(order, vec![IndexRun::open(&merged_path)?]);
        Ok(())
    }

    /// The open runs of one sort order (bulk sequential reads).
    pub fn runs_of(&self, order: SortOrder) -> &[IndexRun] {
        self.runs.get(&order).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Total runs currently open for a sort order.
    pub fn run_count(&self, order: SortOrder) -> usize {
        self.runs.get(&order).map(Vec::len).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact::{AttributeRegistry, decompose};
    use crate::segment::SegmentStore;
    use ekos_kir::{KirObject, ObjectKind};
    use tempfile::tempdir;

    fn store_with_objects(
        dir: &Path,
        names: &[&str],
    ) -> (SegmentStore, AttributeRegistry, Vec<Uuid>) {
        let mut store = SegmentStore::open(dir).unwrap();
        let mut reg = AttributeRegistry::new();
        let mut ids = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let obj = KirObject::new(*name, ObjectKind::Table)
                .with_property("size_bytes", serde_json::json!(i));
            ids.push(obj.id.0);
            let facts = decompose(obj.id.0, &serde_json::to_value(&obj).unwrap(), &mut reg)
                .unwrap()
                .into_iter()
                .map(|f| (FactOp::Assert, f))
                .collect();
            store.append(facts, i as i64).unwrap();
        }
        (store, reg, ids)
    }

    // ── RFC 0168: entity filter ─────────────────────────────────────────────

    /// `n` entities × 3 attributes, as EAVT entries.
    fn synthetic(seed: u128, n: u128) -> (Vec<Uuid>, Vec<IndexEntry>) {
        let ids: Vec<Uuid> = (0..n)
            .map(|i| {
                Uuid::from_u128(mix64((seed * 1_000_003 + i) as u64) as u128 * 0x9E37_79B9 + i)
            })
            .collect();
        let entries = ids
            .iter()
            .flat_map(|id| {
                (0..3u32).map(move |a| IndexEntry {
                    entity: *id,
                    attr: AttrId(a),
                    pos: None,
                    tx: TxId(1),
                    op: FactOp::Assert,
                    value: FactValue::String(format!("{id}-{a}")),
                })
            })
            .collect();
        (ids, entries)
    }

    fn run_from(path: &Path, entries: Vec<IndexEntry>, with_filter: bool) -> IndexRun {
        let mut raws: Vec<RawRecord> = entries
            .into_iter()
            .filter_map(|e| project(SortOrder::Eavt, e))
            .collect();
        raws.sort_by(|a, b| a.0.cmp(&b.0));
        write_run_raw_with(path, SortOrder::Eavt, &raws, with_filter).unwrap();
        IndexRun::open(path).unwrap()
    }

    fn entity_prefix(id: &Uuid) -> Vec<u8> {
        ScanPrefix::Entity {
            entity: *id,
            attr: None,
        }
        .bytes()
    }

    #[test]
    fn filter_has_no_false_negatives_and_a_low_false_positive_rate() {
        let (present, _) = synthetic(1, 20_000);
        let (absent, _) = synthetic(2, 20_000);
        let f = EntityFilter::build(present.iter(), present.len());
        assert!(
            present.iter().all(|id| f.may_contain(id)),
            "a false negative loses data"
        );
        let fp = absent.iter().filter(|id| f.may_contain(id)).count();
        assert!(fp < 400, "{fp} of 20000 absent ids passed (>2%)");
    }

    #[test]
    fn filter_round_trips_through_the_run_directory_deterministically() {
        let dir = tempdir().unwrap();
        let (ids, entries) = synthetic(3, 1_000);
        let a = run_from(&dir.path().join("a.run"), entries.clone(), true);
        let b = run_from(&dir.path().join("b.run"), entries, true);
        let fa = a
            .dir
            .entity_filter
            .as_ref()
            .expect("EAVT run carries a filter");
        assert_eq!(
            fa.bits,
            b.dir.entity_filter.as_ref().unwrap().bits,
            "same input, same bits"
        );
        assert!(
            ids.iter().all(|id| fa.may_contain(id)),
            "hydrated filter matches"
        );
    }

    /// Filtered and unfiltered runs return identical results, for present and absent entities,
    /// and the filtered set reads far fewer blocks.
    #[test]
    fn filtered_scans_equal_unfiltered_and_skip_block_reads() {
        let dir = tempdir().unwrap();
        let mut filtered = Vec::new();
        let mut plain = Vec::new();
        let mut all_ids = Vec::new();
        for r in 0..8u128 {
            let (ids, entries) = synthetic(10 + r, 400);
            all_ids.extend(ids);
            filtered.push(run_from(
                &dir.path().join(format!("f{r}.run")),
                entries.clone(),
                true,
            ));
            plain.push(run_from(
                &dir.path().join(format!("p{r}.run")),
                entries,
                false,
            ));
        }
        let (absent, _) = synthetic(99, 200);
        let scan = |runs: &[IndexRun], id: &Uuid| -> Vec<IndexEntry> {
            runs.iter()
                .flat_map(|r| r.scan(&entity_prefix(id)).unwrap())
                .collect()
        };
        for id in all_ids.iter().chain(absent.iter()) {
            assert_eq!(scan(&filtered, id), scan(&plain, id), "{id}");
        }
        let reads = |runs: &[IndexRun]| -> usize {
            runs.iter()
                .map(|r| r.blocks_read.load(std::sync::atomic::Ordering::SeqCst))
                .sum()
        };
        let (f, p) = (reads(&filtered), reads(&plain));
        assert!(f * 4 < p, "filtered read {f} blocks, unfiltered {p}");
    }

    /// Pre-RFC runs (no filter) and new runs mixed in one index set still scan correctly.
    #[test]
    fn a_mix_of_pre_rfc_and_filtered_runs_scans_correctly() {
        let dir = tempdir().unwrap();
        let (old_ids, old_entries) = synthetic(20, 300);
        let (new_ids, new_entries) = synthetic(21, 300);
        let old = run_from(&dir.path().join("old.run"), old_entries, false);
        let new = run_from(&dir.path().join("new.run"), new_entries, true);
        assert!(old.dir.entity_filter.is_none() && new.dir.entity_filter.is_some());
        for id in old_ids.iter().chain(new_ids.iter()) {
            let hits: usize = [&old, &new]
                .iter()
                .map(|r| r.scan(&entity_prefix(id)).unwrap().len())
                .sum();
            assert_eq!(hits, 3, "{id}");
        }
    }

    /// The test of the test: a filter missing one entity must make the equivalence check fail.
    #[test]
    fn a_planted_false_negative_is_caught_by_the_equivalence_check() {
        let dir = tempdir().unwrap();
        let (ids, entries) = synthetic(30, 500);
        let plain = run_from(&dir.path().join("p.run"), entries.clone(), false);
        let mut broken = run_from(&dir.path().join("b.run"), entries, true);
        // Plant against the first id the broken filter really excludes — skipping any that
        // collide into a false positive, so the test can never pass without testing anything.
        let victim = ids
            .iter()
            .copied()
            .find(|v| {
                let others: Vec<Uuid> = ids.iter().copied().filter(|id| id != v).collect();
                !EntityFilter::build(others.iter(), others.len()).may_contain(v)
            })
            .expect("some id is not a false positive");
        let others: Vec<Uuid> = ids.iter().copied().filter(|id| *id != victim).collect();
        broken.dir.entity_filter = Some(EntityFilter::build(others.iter(), others.len()));
        let lost = broken.scan(&entity_prefix(&victim)).unwrap();
        let truth = plain.scan(&entity_prefix(&victim)).unwrap();
        assert_ne!(lost, truth, "equivalence must detect the missing entity");
    }

    #[test]
    fn a_malformed_filter_disables_filtering_instead_of_failing() {
        let bad = EntityFilter {
            bits: "zz".into(),
            words: vec![],
        };
        assert!(bad.hydrate().is_none());
    }

    #[test]
    fn eavt_scan_returns_one_entitys_facts() {
        let dir = tempdir().unwrap();
        let (store, _reg, ids) = store_with_objects(dir.path(), &["orders", "customers"]);

        let mut idx = FactIndexes::open(dir.path().join("indexes")).unwrap().0;
        idx.add_runs("000000", &entries_from_batches(&store.batches().unwrap()))
            .unwrap();

        let facts = idx
            .scan(&ScanPrefix::Entity {
                entity: ids[0],
                attr: None,
            })
            .unwrap();
        assert!(!facts.is_empty());
        assert!(
            facts.iter().all(|e| e.entity == ids[0]),
            "only the asked entity"
        );
        // name, kind, id, created_at, properties.size_bytes at least.
        assert!(facts.len() >= 5);
    }

    #[test]
    fn avet_scan_finds_entity_by_ref_value() {
        let dir = tempdir().unwrap();
        let (store, mut reg, ids) = store_with_objects(dir.path(), &["orders", "customers"]);

        // A relationship provides the ref-valued facts AVET exists for.
        let rel = ekos_kir::KirRelationship::new(
            ekos_kir::RelationshipKind::ForeignKey,
            ekos_kir::KirId(ids[0]),
            ekos_kir::KirId(ids[1]),
        );
        let mut store = store;
        let facts = decompose(rel.id.0, &serde_json::to_value(&rel).unwrap(), &mut reg)
            .unwrap()
            .into_iter()
            .map(|f| (FactOp::Assert, f))
            .collect();
        store.append(facts, 99).unwrap();

        let mut idx = FactIndexes::open(dir.path().join("indexes")).unwrap().0;
        idx.add_runs("000000", &entries_from_batches(&store.batches().unwrap()))
            .unwrap();

        // Graph hop: who points at ids[1]?
        let to_attr = reg.intern("to");
        let hits = idx
            .scan(&ScanPrefix::AttrValue {
                attr: to_attr,
                value: FactValue::Ref(ids[1]),
            })
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entity, rel.id.0);

        // Scalar values are deliberately NOT in AVET (RFC 0016 §7):
        // name lookups go through AEVT/tantivy.
        let name_attr = reg.intern("name");
        let none = idx
            .scan(&ScanPrefix::AttrValue {
                attr: name_attr,
                value: FactValue::String("orders".into()),
            })
            .unwrap();
        assert!(none.is_empty(), "scalar values are not AVET-indexed");
    }

    #[test]
    fn aevt_scan_lists_every_entity_with_attribute() {
        let dir = tempdir().unwrap();
        let (store, mut reg, ids) = store_with_objects(dir.path(), &["a", "b", "c"]);

        let mut idx = FactIndexes::open(dir.path().join("indexes")).unwrap().0;
        idx.add_runs("000000", &entries_from_batches(&store.batches().unwrap()))
            .unwrap();

        let kind_attr = reg.intern("kind");
        let hits = idx.scan(&ScanPrefix::Attr { attr: kind_attr }).unwrap();
        let mut entities: Vec<Uuid> = hits.iter().map(|e| e.entity).collect();
        entities.dedup();
        assert_eq!(entities.len(), 3);
        for id in ids {
            assert!(hits.iter().any(|e| e.entity == id));
        }
    }

    #[test]
    fn scans_merge_across_runs_and_survive_compaction() {
        let dir = tempdir().unwrap();
        let (mut store, mut reg, ids) = store_with_objects(dir.path(), &["orders"]);

        let mut idx = FactIndexes::open(dir.path().join("indexes")).unwrap().0;
        idx.add_runs("000000", &entries_from_batches(&store.batches().unwrap()))
            .unwrap();

        // A second run: retract + assert of size_bytes (a later version).
        let size_attr = reg.intern("properties.size_bytes");
        let tx = store
            .append(
                vec![
                    (
                        FactOp::Retract,
                        Fact {
                            entity: ids[0],
                            attr: size_attr,
                            pos: None,
                            value: FactValue::Number(0.into()),
                        },
                    ),
                    (
                        FactOp::Assert,
                        Fact {
                            entity: ids[0],
                            attr: size_attr,
                            pos: None,
                            value: FactValue::Number(99.into()),
                        },
                    ),
                ],
                7,
            )
            .unwrap();
        let all = store.batches().unwrap();
        idx.add_runs("000001", &entries_from_batches(&all[all.len() - 1..]))
            .unwrap();
        assert_eq!(idx.run_count(SortOrder::Eavt), 2);

        let check = |idx: &FactIndexes| {
            let facts = idx
                .scan(&ScanPrefix::Entity {
                    entity: ids[0],
                    attr: Some(size_attr),
                })
                .unwrap();
            // v0 assert, v1 retract, v1 assert — in tx order.
            assert_eq!(facts.len(), 3);
            assert_eq!(facts[0].tx, TxId(0));
            assert_eq!(facts[1].tx, tx);
            assert_eq!(facts[2].tx, tx);
            assert!(
                matches!(facts[1].op, FactOp::Retract) || matches!(facts[2].op, FactOp::Retract)
            );
        };
        check(&idx);

        // Compact to one run per order: identical results.
        for order in SortOrder::ALL {
            idx.merge_runs(order).unwrap();
            assert_eq!(idx.run_count(order), 1);
        }
        check(&idx);

        // And reopening from disk sees the merged runs.
        let idx = FactIndexes::open(dir.path().join("indexes")).unwrap().0;
        check(&idx);
    }

    #[test]
    fn indexes_rebuild_from_segments() {
        let dir = tempdir().unwrap();
        let (store, _reg, ids) = store_with_objects(dir.path(), &["orders", "customers"]);
        let batches = store.batches().unwrap();

        let mut idx = FactIndexes::open(dir.path().join("indexes")).unwrap().0;
        idx.add_runs("000000", &entries_from_batches(&batches))
            .unwrap();
        let before = idx
            .scan(&ScanPrefix::Entity {
                entity: ids[1],
                attr: None,
            })
            .unwrap();

        // Blow the directory away and rebuild — nothing lost (derived data).
        let mut idx = FactIndexes::open(dir.path().join("indexes")).unwrap().0;
        idx.build_from_batches(&batches).unwrap();
        let after = idx
            .scan(&ScanPrefix::Entity {
                entity: ids[1],
                attr: None,
            })
            .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn value_keys_with_embedded_zeros_and_prefixes_stay_ordered() {
        let attr = AttrId(1);
        let mk = |v: FactValue, n: u128| IndexEntry {
            entity: Uuid::from_u128(n),
            attr,
            pos: None,
            tx: TxId(0),
            op: FactOp::Assert,
            value: v,
        };
        // The escaped-terminator encoding must keep tuple order for
        // variable-length value keys: "ab" < "ab\0" < "abc".
        let a = encode_key(SortOrder::Avet, &mk(FactValue::String("ab".into()), 1));
        let b = encode_key(SortOrder::Avet, &mk(FactValue::String("ab\0".into()), 2));
        let c = encode_key(SortOrder::Avet, &mk(FactValue::String("abc".into()), 3));
        assert!(a < b, "terminator must sort before escaped zero");
        assert!(b < c, "escaped zero must sort before larger byte");

        // Runs index only ref values; an exact ref lookup hits exactly one.
        let dir = tempdir().unwrap();
        let path = dir.path().join("avet-t.run");
        let target = Uuid::from_u128(77);
        write_run(
            &path,
            SortOrder::Avet,
            vec![
                mk(FactValue::Ref(target), 1),
                mk(FactValue::Ref(Uuid::from_u128(78)), 2),
                mk(FactValue::String("not indexed".into()), 3),
            ],
        )
        .unwrap();
        let run = IndexRun::open(&path).unwrap();
        assert_eq!(run.entry_count(), 2, "scalar entry projected out");
        let hits = run
            .scan(
                &ScanPrefix::AttrValue {
                    attr,
                    value: FactValue::Ref(target),
                }
                .bytes(),
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entity, Uuid::from_u128(1));
    }

    #[test]
    fn block_pruning_still_finds_entries_across_blocks() {
        // More entries than one block: prefix scans must cross block bounds.
        let attr = AttrId(0);
        let entries: Vec<IndexEntry> = (0..(BLOCK_ENTRIES * 3))
            .map(|i| IndexEntry {
                entity: Uuid::from_u128(i as u128),
                attr,
                pos: None,
                tx: TxId(i as u64),
                op: FactOp::Assert,
                value: FactValue::Number((i as u64).into()),
            })
            .collect();
        let dir = tempdir().unwrap();
        let path = dir.path().join("aevt-big.run");
        write_run(&path, SortOrder::Aevt, entries.clone()).unwrap();
        let run = IndexRun::open(&path).unwrap();
        assert!(run.dir.blocks.len() >= 3);

        let hits = run.scan(&ScanPrefix::Attr { attr }.bytes()).unwrap();
        assert_eq!(hits.len(), entries.len(), "all blocks contribute");
        // A single-entity EAVT-style prefix on this AEVT run's key space:
        let one = run
            .scan(&{
                let mut k = attr.0.to_be_bytes().to_vec();
                k.extend_from_slice(Uuid::from_u128(700).as_bytes());
                k
            })
            .unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].entity, Uuid::from_u128(700));
    }
}
