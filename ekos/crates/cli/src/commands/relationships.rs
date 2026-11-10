//! RFC 0252 §3.3/§3.4: `ekos relationships discover|list` — references found in the data, beside
//! the ones code joins.
//!
//! `discover` computes, per live database, a MinHash signature of every comparable column
//! (`ekos_observation_sdk::overlap`, computed by the database: only salted digest minimums leave
//! it), proposes pairs by LSH banding, keeps a pair whose estimated containment of the child's
//! values in a unique parent's reaches the threshold, adds every pair code joins, and verifies each
//! exactly with RFC 0158's inclusion check. Each survivor is a keyed `FkCandidate` with `seed:
//! code | data | both` and the RFC 0158 verdict. Across databases a pair cannot be joined, so
//! overlapping key columns become a `value_overlap` signal on a `SameAs` candidate (RFC 0029),
//! never a merge.
//!
//! The salt is the project's (`<state>/profile/salt`, created once, never in the ledger).

use super::sql_check::{ProbeWorker, Target};
use anyhow::{Result, bail};
use ekos_common::redaction::{RedactionConfig, redact};
use ekos_compiler_core::{DatabaseConfig, DatabaseKind, EkosConfig};
use ekos_kir::{
    KirEvidence, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
};
use ekos_ledger::KnowledgeStore;
use ekos_migrate_dq::infer::InclusionResult;
use ekos_observation_sdk::overlap::{self, Signature};
use ekos_observation_sdk::profile::Flavor;
use ekos_sqlgen::probe::Probe;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The keyed kind an inferred reference is written as.
pub const KIND: &str = "FkCandidate";

pub struct DiscoverOpts<'a> {
    pub database: Option<&'a str>,
    pub cross_database: bool,
    pub min_containment: f64,
    pub min_parent_distinct: i64,
    pub max_rows: i64,
    pub allow_writable_role: bool,
    pub json: bool,
}

/// One column's signature, with where it lives.
#[derive(Debug, Clone)]
struct Col {
    database: String,
    owner: String,
    table: String,
    column: String,
    table_id: KirId,
    unique: bool,
    /// Whole number (as opposed to text): only the same kind is compared.
    numeric: bool,
    sig: Signature,
}

impl Col {
    fn label(&self) -> String {
        format!("{}.{}.{}", self.owner, self.table, self.column)
    }
}

/// A pair to verify: (child, parent) indices into the column list, how it was seeded, the estimate.
#[derive(Debug, Clone)]
struct Pair {
    child: usize,
    parent: usize,
    data: bool,
    code_sources: Vec<String>,
    containment: Option<f64>,
}

fn flavor(kind: DatabaseKind) -> Flavor {
    match kind {
        DatabaseKind::Oracle => Flavor::Oracle,
        DatabaseKind::Postgres => Flavor::Postgres,
        DatabaseKind::Clickhouse => Flavor::ClickHouse,
        DatabaseKind::Sqlserver => Flavor::SqlServer,
    }
}

fn s<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties.get(k).and_then(Value::as_str).unwrap_or("")
}

fn kid(key: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()))
}

pub fn current_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config
        .ekos_dir(cwd)
        .join("profile")
        .join("relationships.json")
}

/// The ids the latest `discover` wrote; `None` before the first run.
pub fn current_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    let v: Value = serde_json::from_slice(&std::fs::read(current_path(config, cwd)).ok()?).ok()?;
    Some(
        v["ids"]
            .as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
    )
}

/// The project's salt: 32 letters and digits, created on first use, readable by the owner only.
fn salt(config: &EkosConfig, cwd: &Path) -> Result<String> {
    let path = config.ekos_dir(cwd).join("profile").join("salt");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim().to_string();
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Ok(s);
        }
    }
    std::fs::create_dir_all(path.parent().expect("a state directory"))?;
    let s: String = Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .chain(Uuid::new_v4().simple().to_string().chars())
        .take(32)
        .collect();
    std::fs::write(&path, &s)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(s)
}

fn columns(t: &KirObject) -> &[Value] {
    t.properties
        .get("columns")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn known_rows(t: &KirObject) -> Option<i64> {
    t.properties
        .get("profile")
        .and_then(|p| p["row_count"].as_i64())
        .or_else(|| t.properties.get("num_rows").and_then(Value::as_i64))
}

/// Single-column keys of a table: declared primary key / unique, or the grain a profile found.
fn unique_columns(
    t: &KirObject,
    grain: &HashMap<(String, String, String), Vec<String>>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for c in columns(t) {
        if (c["primary_key"] == true || c["unique"] == true)
            && let Some(n) = c["name"].as_str()
        {
            out.insert(n.to_string());
        }
    }
    if let Some(Value::Array(pk)) = t.properties.get("primary_key")
        && pk.len() == 1
        && let Some(n) = pk[0].as_str()
    {
        out.insert(n.to_string());
    }
    if let Some(k) = grain.get(&(
        s(t, "database").into(),
        s(t, "owner").into(),
        t.name.clone(),
    )) && k.len() == 1
    {
        out.insert(k[0].clone());
    }
    out
}

/// Single-column keys a profile run measured, by (database, owner, table).
fn measured_keys(
    config: &EkosConfig,
    cwd: &Path,
) -> HashMap<(String, String, String), Vec<String>> {
    super::profile::load(config, cwd)
        .map(|sig| {
            sig.grain
                .into_iter()
                .filter_map(|g| {
                    let k = g.search?.keys.into_iter().find(|k| k.len() == 1)?;
                    Some(((g.database, g.owner, g.table), k))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Declared foreign keys as (child table, child column, parent table, parent column), lowercase,
/// from each `ForeignKey`'s `fk_desc` (`from.a, b → to.x, y`).
fn declared(
    objects: &[KirObject],
    relationships: &[KirRelationship],
) -> BTreeSet<(String, String, String, String)> {
    let names: HashMap<KirId, String> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .map(|o| (o.id, o.name.to_lowercase()))
        .collect();
    let mut out = BTreeSet::new();
    for r in relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::ForeignKey)
    {
        let (Some(from), Some(to)) = (names.get(&r.from), names.get(&r.to)) else {
            continue;
        };
        let desc = r
            .properties
            .get("fk_desc")
            .and_then(Value::as_str)
            .unwrap_or("");
        let Some((l, rr)) = desc.split_once('→') else {
            continue;
        };
        let cols = |side: &str| -> Vec<String> {
            side.split_once('.')
                .map(|(_, c)| c.split(',').map(|x| x.trim().to_lowercase()).collect())
                .unwrap_or_default()
        };
        for (c, p) in cols(l.trim()).into_iter().zip(cols(rr.trim())) {
            out.insert((from.clone(), c, to.clone(), p));
        }
    }
    out
}

/// Signatures of every comparable column of `db`'s live tables.
fn sign_database(
    db: &DatabaseConfig,
    probe: &dyn Probe,
    tables: &[&KirObject],
    grain: &HashMap<(String, String, String), Vec<String>>,
    salt: &str,
    max_rows: i64,
    redaction: &RedactionConfig,
    errors: &mut Vec<Value>,
) -> Vec<Col> {
    let f = flavor(db.kind);
    let mut out = Vec::new();
    for t in tables {
        if max_rows > 0 && known_rows(t).is_some_and(|n| n > max_rows) {
            continue;
        }
        let keys = unique_columns(t, grain);
        for name in overlap::signature_columns(f, columns(t)) {
            let ty = columns(t)
                .iter()
                .find(|c| c["name"].as_str() == Some(name.as_str()))
                .and_then(|c| c["data_type"].as_str())
                .unwrap_or("");
            let numeric = ekos_observation_sdk::profile::family(f, ty)
                == ekos_observation_sdk::profile::Family::Number;
            let row = overlap::signature_sql(f, s(t, "owner"), &t.name, &name, salt, numeric)
                .and_then(|q| probe.preview(&q, 1))
                .and_then(|p| {
                    p.rows
                        .first()
                        .and_then(|r| overlap::parse_signature(r))
                        .ok_or_else(|| "a signature query returned no usable row".to_string())
                });
            match row {
                Ok(sig) if sig.distinct > 0 => out.push(Col {
                    database: db.name.clone(),
                    owner: s(t, "owner").into(),
                    table: t.name.clone(),
                    column: name.clone(),
                    table_id: t.id,
                    unique: keys.contains(&name),
                    numeric,
                    sig,
                }),
                Ok(_) => {}
                Err(e) => errors.push(json!({
                    "database": db.name, "table": t.name, "column": name, "error": redact(&e, redaction),
                })),
            }
        }
    }
    out
}

/// Pairs within one database: data-seeded (LSH, then containment) and code-seeded (joins).
fn propose(
    cols: &[Col],
    code: &BTreeMap<(String, String, String, String), Vec<String>>,
    declared: &BTreeSet<(String, String, String, String)>,
    min_containment: f64,
    min_parent_distinct: i64,
) -> Vec<Pair> {
    let mut by_band: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, c) in cols.iter().enumerate() {
        for b in overlap::bands(&c.sig) {
            by_band.entry(b).or_default().push(i);
        }
    }
    let key = |c: &Col| (c.table.to_lowercase(), c.column.to_lowercase());
    let mut pairs: BTreeMap<(usize, usize), Pair> = BTreeMap::new();
    for (p, parent) in cols.iter().enumerate() {
        if !parent.unique || parent.sig.distinct < min_parent_distinct {
            continue;
        }
        let near: BTreeSet<usize> = overlap::bands(&parent.sig)
            .iter()
            .filter_map(|b| by_band.get(b))
            .flatten()
            .copied()
            .collect();
        for c in near {
            let child = &cols[c];
            if child.table_id == parent.table_id
                || child.sig.distinct > parent.sig.distinct
                || child.numeric != parent.numeric
            {
                continue;
            }
            // Names break ties the data cannot: a key column pointing at another key, or any
            // column inside a dense id sequence (every small whole number is "contained" in one).
            let affinity = overlap::name_affinity(&child.column, &parent.table, &parent.column);
            if (child.unique || parent.sig.dense()) && !affinity {
                continue;
            }
            let (ct, cc) = key(child);
            let (pt, pc) = key(parent);
            if declared.contains(&(ct, cc, pt, pc)) {
                continue;
            }
            let est = overlap::containment(&child.sig, &parent.sig);
            if est >= min_containment {
                pairs.insert(
                    (c, p),
                    Pair {
                        child: c,
                        parent: p,
                        data: true,
                        code_sources: vec![],
                        containment: Some(est),
                    },
                );
            }
        }
    }
    // Code joins between two signed columns, in either direction: the unique side is the parent.
    let idx: HashMap<(String, String), Vec<usize>> =
        cols.iter()
            .enumerate()
            .fold(HashMap::new(), |mut m, (i, c)| {
                m.entry(key(c)).or_default().push(i);
                m
            });
    for ((lt, lc, rt, rc), sources) in code {
        let (Some(ls), Some(rs)) = (
            idx.get(&(lt.clone(), lc.clone())),
            idx.get(&(rt.clone(), rc.clone())),
        ) else {
            continue;
        };
        for &l in ls {
            for &r in rs {
                let (c, p) = if cols[r].unique || !cols[l].unique {
                    (l, r)
                } else {
                    (r, l)
                };
                if cols[c].table_id == cols[p].table_id {
                    continue;
                }
                let (ct, cc) = key(&cols[c]);
                let (pt, pc) = key(&cols[p]);
                if declared.contains(&(ct, cc, pt, pc)) {
                    continue;
                }
                let est = overlap::containment(&cols[c].sig, &cols[p].sig);
                let e = pairs.entry((c, p)).or_insert(Pair {
                    child: c,
                    parent: p,
                    data: false,
                    code_sources: vec![],
                    containment: Some(est),
                });
                e.code_sources.extend(sources.iter().cloned());
                e.code_sources.sort();
                e.code_sources.dedup();
            }
        }
    }
    pairs.into_values().collect()
}

/// The `FkCandidate` for one verified pair.
fn candidate_object(
    cols: &[Col],
    pair: &Pair,
    result: Option<&InclusionResult>,
    measured_at: &str,
) -> (KirObject, KirEvidence) {
    let (c, p) = (&cols[pair.child], &cols[pair.parent]);
    let key = format!("fk-candidate:{}:{}->{}", c.database, c.label(), p.label());
    let seed = match (pair.data, !pair.code_sources.is_empty()) {
        (true, true) => "both",
        (true, false) => "data",
        _ => "code",
    };
    let verdict = result
        .map(|r| r.verdict().as_str())
        .unwrap_or("not_measured");
    let mut o = KirObject::new(
        format!("{}.{} → {}.{}", c.table, c.column, p.table, p.column),
        ObjectKind::Custom(KIND.into()),
    );
    o.id = kid(&key);
    for (k, v) in [
        ("status", json!("hypothesis")),
        ("database", json!(c.database)),
        ("child_owner", json!(c.owner)),
        ("child_table", json!(c.table)),
        ("child_column", json!(c.column)),
        ("parent_owner", json!(p.owner)),
        ("parent_table", json!(p.table)),
        ("parent_column", json!(p.column)),
        ("seed", json!(seed)),
        ("verdict", json!(verdict)),
        ("orphans", json!(result.map(|r| r.orphans))),
        ("population", json!(result.map(|r| r.population))),
        (
            "inclusion_rate",
            json!(result.and_then(InclusionResult::inclusion_rate)),
        ),
        (
            "containment_estimate",
            json!(pair.containment.map(|x| (x * 1000.0).round() / 1000.0)),
        ),
        ("parent_unique", json!(p.unique)),
        (
            "name_affinity",
            json!(overlap::name_affinity(&c.column, &p.table, &p.column)),
        ),
        ("code_sources", json!(pair.code_sources)),
        ("measured_at", json!(measured_at)),
    ] {
        o.properties.insert(k.into(), v);
    }
    let mut ev = KirEvidence::new(
        SourceLocation {
            path: format!("profile://{}/{}", c.database, c.label()),
            line: None,
            column: None,
        },
        format!(
            "{} values checked against {} ({} seed; RFC 0252 overlap + RFC 0158 inclusion)",
            c.label(),
            p.label(),
            seed
        ),
    );
    ev.id = kid(&format!("{key}:evidence"));
    o.evidence = vec![ev.id];
    (o, ev)
}

/// Rank: both seeds first, then a clean verdict, then the estimate.
fn rank(o: &KirObject) -> (u8, u8, i64) {
    let seed = match s(o, "seed") {
        "both" => 0,
        "data" => 1,
        _ => 2,
    };
    let verdict = match s(o, "verdict") {
        "clean_relationship" => 0,
        "relationship_with_orphans" => 1,
        "unclear" => 2,
        _ => 3,
    };
    let est = o
        .properties
        .get("containment_estimate")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    (seed, verdict, -(est * 1000.0) as i64)
}

/// Code joins, lowercased, with their sources.
fn code_joins(
    ledger: &dyn KnowledgeStore,
) -> Result<BTreeMap<(String, String, String, String), Vec<String>>> {
    let mut out: BTreeMap<_, Vec<String>> = BTreeMap::new();
    for j in super::migrate::harvest_join_observations(ledger)? {
        let tail = |t: &str| t.rsplit('.').next().unwrap_or(t).to_lowercase();
        out.entry((
            tail(&j.left_table),
            j.left_column.to_lowercase(),
            tail(&j.right_table),
            j.right_column.to_lowercase(),
        ))
        .or_default()
        .push(j.source);
    }
    Ok(out)
}

/// `ekos relationships discover`.
pub async fn discover(config: &EkosConfig, cwd: &Path, opts: DiscoverOpts<'_>) -> Result<()> {
    let dbs: Vec<&DatabaseConfig> = config
        .databases
        .iter()
        .filter(|d| opts.database.is_none_or(|n| n == d.name))
        .collect();
    if dbs.is_empty() {
        bail!("no [[databases]] to read (ekos database list)");
    }
    let ledger = super::store::open_store(config, cwd)?;
    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;
    let declared = declared(&objects, &relationships);
    let code = code_joins(&*ledger)?;
    let grain = measured_keys(config, cwd);
    let salt = salt(config, cwd)?;
    let redaction = config.redaction_config();
    let measured_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let (mut all_cols, mut written, mut errors, mut report) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for db in dbs {
        let tables: Vec<&KirObject> = {
            let mut v: Vec<&KirObject> = objects
                .iter()
                .filter(|o| {
                    o.kind == ObjectKind::Table
                        && s(o, "origin") == "live"
                        && s(o, "database") == db.name
                })
                .collect();
            v.sort_by(|a, b| (s(a, "owner"), &a.name).cmp(&(s(b, "owner"), &b.name)));
            v.dedup_by(|a, b| a.id == b.id);
            v
        };
        let r = tokio::task::block_in_place(|| {
            let (worker, status) =
                ProbeWorker::start(&Target::Database(db.clone()), opts.allow_writable_role);
            let Some(worker) = worker else {
                let why = match status {
                    Some(Err(e)) => redact(&e, &redaction),
                    _ => "not connected".into(),
                };
                return Err(why);
            };
            let cols = sign_database(
                db,
                &worker,
                &tables,
                &grain,
                &salt,
                opts.max_rows,
                &redaction,
                &mut errors,
            );
            let pairs = propose(
                &cols,
                &code,
                &declared,
                opts.min_containment,
                opts.min_parent_distinct,
            );
            let f = flavor(db.kind);
            let mut out = Vec::new();
            for pair in &pairs {
                let (c, p) = (&cols[pair.child], &cols[pair.parent]);
                let result = overlap::inclusion_sql(
                    f,
                    (&c.owner, &c.table, &c.column),
                    (&p.owner, &p.table, &p.column),
                )
                .and_then(|q| worker.preview(&q, 1))
                .and_then(|r| r.single_number().ok_or_else(|| "no counts".to_string()));
                let result = match result {
                    Ok(n) if n.len() == 2 => Some(InclusionResult {
                        population: n[0],
                        orphans: n[1],
                    }),
                    Ok(_) => None,
                    Err(e) => {
                        errors.push(json!({"database": db.name, "pair": format!("{} → {}", c.label(), p.label()), "error": redact(&e, &redaction)}));
                        None
                    }
                };
                // A data-only proposal the data does not bear out is not a candidate.
                let keep = !pair.code_sources.is_empty()
                    || result.as_ref().is_some_and(|r| {
                        r.verdict().is_relationship() || r.verdict().as_str() == "unclear"
                    });
                if keep {
                    out.push(candidate_object(&cols, pair, result.as_ref(), &measured_at));
                }
            }
            Ok((cols, pairs.len(), out))
        });
        match r {
            Ok((cols, proposed, out)) => {
                report.push(json!({"database": db.name, "columns_signed": cols.len(), "pairs_checked": proposed, "candidates": out.len()}));
                written.extend(out);
                all_cols.extend(cols);
            }
            Err(why) => report.push(json!({"database": db.name, "skipped": why})),
        }
    }
    written.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then(a.0.name.cmp(&b.0.name)));
    let mut ids = Vec::new();
    for (o, ev) in &written {
        ledger.append_evidence(ev)?;
        ledger.append_object(o)?;
        for (owner, table) in [
            ("child_owner", "child_table"),
            ("parent_owner", "parent_table"),
        ] {
            let end = objects.iter().find(|t| {
                t.kind == ObjectKind::Table
                    && s(t, "origin") == "live"
                    && s(t, "database") == s(o, "database")
                    && s(t, "owner") == s(o, owner)
                    && t.name == s(o, table)
            });
            if let Some(t) = end {
                ledger.append_relationship(&KirRelationship::deterministic(
                    RelationshipKind::Custom(ekos_semantic::business_semantics::DESCRIBES.into()),
                    o.id,
                    t.id,
                    "",
                ))?;
            }
        }
        ids.push(o.id.to_string());
    }
    let overlaps = if opts.cross_database {
        cross_database(
            &*ledger,
            &all_cols,
            &measured_at,
            opts.min_containment,
            opts.min_parent_distinct,
        )?
    } else {
        Vec::new()
    };
    let overlap_count = overlaps.len();
    ids.extend(overlaps);
    let path = current_path(config, cwd);
    std::fs::create_dir_all(path.parent().expect("a state directory"))?;
    let doc = json!({
        "rfc": "0252", "measured_at": measured_at, "ids": ids, "report": report,
        "errors": errors, "cross_database_overlaps": overlap_count,
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&doc)?)?;
    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "report": report, "candidates": written.iter().map(|(o, _)| brief(o)).collect::<Vec<_>>(),
                "cross_database_overlaps": overlap_count, "errors": errors,
            }))?
        );
        return Ok(());
    }
    for r in &report {
        match r.get("skipped") {
            Some(w) => println!(
                "{}: skipped — {}",
                r["database"].as_str().unwrap_or(""),
                w.as_str().unwrap_or("")
            ),
            None => println!(
                "{}: {} column(s) signed, {} pair(s) checked, {} candidate(s)",
                r["database"].as_str().unwrap_or(""),
                r["columns_signed"],
                r["pairs_checked"],
                r["candidates"]
            ),
        }
    }
    print_candidates(written.iter().map(|(o, _)| o));
    if opts.cross_database {
        println!("{overlap_count} cross-database overlap(s) recorded on SameAs candidates.");
    }
    if !errors.is_empty() {
        println!("{} error(s); see {}", errors.len(), path.display());
    }
    Ok(())
}

fn brief(o: &KirObject) -> Value {
    let g = |k: &str| o.properties.get(k).cloned().unwrap_or(Value::Null);
    json!({
        "id": o.id.to_string(), "name": o.name, "database": g("database"), "seed": g("seed"),
        "verdict": g("verdict"), "orphans": g("orphans"), "population": g("population"),
        "containment_estimate": g("containment_estimate"), "status": g("status"),
        "decision": g("decision"),
    })
}

fn print_candidates<'a>(it: impl Iterator<Item = &'a KirObject>) {
    for o in it {
        println!(
            "  [{}] {} — {}, {} orphan(s) of {} (estimate {})",
            s(o, "seed"),
            o.name,
            s(o, "verdict"),
            o.properties.get("orphans").cloned().unwrap_or(Value::Null),
            o.properties
                .get("population")
                .cloned()
                .unwrap_or(Value::Null),
            o.properties
                .get("containment_estimate")
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
}

/// Overlapping key columns across databases → `value_overlap` on the tables' `SameAs` candidate.
fn cross_database(
    ledger: &dyn KnowledgeStore,
    cols: &[Col],
    measured_at: &str,
    min_containment: f64,
    min_distinct: i64,
) -> Result<Vec<String>> {
    let existing: HashMap<(KirId, KirId), KirRelationship> = ledger
        .all_relationships()?
        .into_iter()
        .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == "SameAs"))
        .flat_map(|r| [((r.from, r.to), r.clone()), ((r.to, r.from), r)])
        .collect();
    let mut by_tables: BTreeMap<(Uuid, Uuid), Vec<Value>> = BTreeMap::new();
    for (i, a) in cols.iter().enumerate() {
        for b in &cols[i + 1..] {
            if a.database == b.database || !(a.unique || b.unique) || a.numeric != b.numeric {
                continue;
            }
            // Few values, or a dense id sequence, overlap with anything of the same size: only
            // the same table name makes such a pair worth showing.
            let same_name = a.table.eq_ignore_ascii_case(&b.table);
            let weak =
                a.sig.distinct.min(b.sig.distinct) < min_distinct || a.sig.dense() || b.sig.dense();
            if weak && !same_name {
                continue;
            }
            let (ab, ba) = (
                overlap::containment(&a.sig, &b.sig),
                overlap::containment(&b.sig, &a.sig),
            );
            if ab.min(ba) < min_containment {
                continue;
            }
            let (x, y) = if a.table_id.0 <= b.table_id.0 {
                (a, b)
            } else {
                (b, a)
            };
            by_tables.entry((x.table_id.0, y.table_id.0)).or_default().push(json!({
                "columns": [format!("{}:{}", x.database, x.label()), format!("{}:{}", y.database, y.label())],
                "containment": [(overlap::containment(&x.sig, &y.sig) * 1000.0).round() / 1000.0,
                                (overlap::containment(&y.sig, &x.sig) * 1000.0).round() / 1000.0],
                "distinct": [x.sig.distinct, y.sig.distinct],
                "measured_at": measured_at,
            }));
        }
    }
    let mut ids = Vec::new();
    for ((a, b), overlaps) in by_tables {
        let (a, b) = (KirId(a), KirId(b));
        let mut rel = existing.get(&(a, b)).cloned().unwrap_or_else(|| {
            let mut r =
                KirRelationship::deterministic(RelationshipKind::Custom("SameAs".into()), a, b, "");
            r.properties.insert("status".into(), json!("unconfirmed"));
            r
        });
        rel.properties
            .insert("value_overlap".into(), json!(overlaps));
        let ev = KirEvidence::new(
            SourceLocation::file("ekos relationships discover --cross-database"),
            "key values compared by salted MinHash across databases (RFC 0252 §3.4)".to_string(),
        );
        ledger.append_evidence(&ev)?;
        if !rel.evidence.contains(&ev.id) {
            rel.evidence.push(ev.id);
        }
        ledger.append_relationship(&rel)?;
        ids.push(rel.id.to_string());
    }
    Ok(ids)
}

/// `ekos relationships list`: the latest discovery's candidates, best first, with any decision.
pub fn list(config: &EkosConfig, cwd: &Path, json_out: bool) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let current = current_ids(config, cwd);
    let mut items: Vec<KirObject> = ledger
        .all_objects()?
        .into_iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == KIND))
        .filter(|o| {
            current
                .as_ref()
                .is_none_or(|c| c.contains(&o.id.to_string()))
        })
        .collect();
    items.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.name.cmp(&b.name)));
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&items.iter().map(brief).collect::<Vec<_>>())?
        );
    } else if items.is_empty() {
        println!("No inferred references (ekos relationships discover).");
    } else {
        print_candidates(items.iter());
    }
    Ok(())
}

/// MCP `ekos_relationships` (read-only): the latest discovery's inferred references through the
/// caller's store, best first, each with its seed, verdict, counts and any review decision.
pub fn agent_list(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn ekos_ledger::KnowledgeReader,
    args: &Value,
) -> Result<Value> {
    let table = args
        .get("table")
        .and_then(Value::as_str)
        .map(str::to_lowercase);
    let seed = args.get("seed").and_then(Value::as_str);
    let verdict = args.get("verdict").and_then(Value::as_str);
    let current = current_ids(config, cwd);
    let objects = ledger.all_objects()?;
    let decisions = ekos_semantic::review::decisions(&objects);
    let mut items: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == KIND))
        .filter(|o| {
            current
                .as_ref()
                .is_none_or(|c| c.contains(&o.id.to_string()))
        })
        .filter(|o| {
            table.as_deref().is_none_or(|t| {
                s(o, "child_table").to_lowercase() == t || s(o, "parent_table").to_lowercase() == t
            })
        })
        .filter(|o| seed.is_none_or(|x| s(o, "seed") == x))
        .filter(|o| verdict.is_none_or(|x| s(o, "verdict") == x))
        .collect();
    items.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.name.cmp(&b.name)));
    let out: Vec<Value> = items
        .into_iter()
        .take(200)
        .map(|o| {
            let mut v = brief(o);
            v["decision"] = decisions
                .get(&("relationships".to_string(), o.id.to_string()))
                .and_then(|d| d.properties.get("decision").cloned())
                .unwrap_or(json!("undecided — a hypothesis until a person reviews it"));
            v
        })
        .collect();
    Ok(json!({ "references": out }))
}
