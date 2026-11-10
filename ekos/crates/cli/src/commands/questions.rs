//! RFC 0254: `ekos questions list|show|export|digest|reassign|decline|mute` and
//! `ekos check code-vs-data`; the commit step that derives open questions and the code-vs-data
//! findings; MCP `ekos_questions`.
//!
//! Questions are derived (`ekos_semantic::questions`), never written by a model; their status is
//! computed on read. Routing state (who a question is with, who said "not me", muted areas) is
//! operational, not knowledge: it lives in `<state>/questions/routing.json`, never the ledger.
//! Revealing a question's unexplained values runs one aggregate on the live source for the person
//! reading it and prints the answer — it is never stored, logged or sent anywhere.

use super::sql_check::{ProbeWorker, Target};
use anyhow::{Context, Result, anyhow, bail};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirObject, ObjectKind};
use ekos_ledger::{KnowledgeReader, KnowledgeStore};
use ekos_semantic::questions::{self, Origins, Owners, Reading, Status};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

fn dir(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("questions")
}

pub fn current_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    dir(config, cwd).join("current.json")
}

fn routing_path(config: &EkosConfig, cwd: &Path) -> PathBuf {
    dir(config, cwd).join("routing.json")
}

fn ids_in(path: &Path) -> Option<BTreeSet<String>> {
    let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some(
        v["ids"]
            .as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
    )
}

pub fn current_ids(config: &EkosConfig, cwd: &Path) -> Option<BTreeSet<String>> {
    ids_in(&current_path(config, cwd))
}

fn s<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties.get(k).and_then(Value::as_str).unwrap_or("")
}

fn is_question(o: &KirObject) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == questions::KIND)
}

/// Tables a person put out of scope (RFC 0213 `scope` decisions `out`), lowercase.
fn out_of_scope(objects: &[KirObject]) -> BTreeSet<String> {
    ekos_semantic::review::decisions(objects)
        .into_iter()
        .filter(|((subject, _), d)| {
            subject == "scope"
                && d.properties
                    .get("decision")
                    .and_then(|x| x["scope"].as_str())
                    == Some("out")
        })
        .filter_map(|((_, id), _)| id.strip_prefix("table:").map(str::to_string))
        .collect()
}

fn owners(config: &EkosConfig) -> Owners {
    Owners {
        rules: config
            .questions
            .owners
            .iter()
            .map(|o| (o.pattern.clone(), o.people.clone()))
            .collect(),
        reviewers: config.questions.reviewers.clone(),
    }
}

/// What the commit step did.
#[derive(Debug, Default, Clone)]
pub struct QuestionStats {
    pub derived: usize,
    pub by_type: BTreeMap<String, usize>,
    pub code_vs_data: usize,
}

/// Re-derives the `code_vs_data` conflicts from the latest profile and keeps the conflicts
/// manifest current. Returns how many findings there are.
pub fn refresh_code_vs_data(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
) -> Result<usize> {
    let Some(signals) = super::profile::load(config, cwd) else {
        return Ok(0);
    };
    let objects = ledger.all_objects()?;
    let refs = super::relationships::current_ids(config, cwd);
    let g = ekos_semantic::code_vs_data::detect(
        &objects,
        &signals,
        refs.as_ref(),
        ekos_semantic::code_vs_data::Settings {
            min_share: config.code_vs_data.min_share,
        },
    );
    super::conflicts::replace_code_vs_data(config, cwd, ledger, &g)
}

/// Derives the open questions from what is current and appends them; writes the manifest.
pub fn commit_step(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
) -> Result<Option<QuestionStats>> {
    if !config.questions.enabled {
        return Ok(None);
    }
    let mut stats = QuestionStats {
        code_vs_data: refresh_code_vs_data(config, cwd, ledger)?,
        ..Default::default()
    };
    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;
    let semantics = ids_in(&config.ekos_dir(cwd).join("semantics").join("current.json"));
    let conflicts = super::conflicts::current_ids(config, cwd);
    let references = super::relationships::current_ids(config, cwd);
    let grain = super::profile::grain_ids(config, cwd);
    let scope = out_of_scope(&objects);
    let owners = owners(config);
    let derived = questions::derive(&Origins {
        objects: &objects,
        relationships: &relationships,
        semantics_current: semantics.as_ref(),
        conflicts_current: conflicts.as_ref(),
        references_current: references.as_ref(),
        grain_current: grain.as_ref(),
        out_of_scope: &scope,
        owners: &owners,
    });
    for q in &derived {
        ledger.append_object(q)?;
        *stats
            .by_type
            .entry(s(q, "question_type").to_string())
            .or_default() += 1;
    }
    stats.derived = derived.len();
    let path = current_path(config, cwd);
    std::fs::create_dir_all(dir(config, cwd))?;
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "rfc": "0254",
            "ids": derived.iter().map(|q| q.id.to_string()).collect::<Vec<_>>(),
            "by_type": stats.by_type,
        }))?,
    )
    .with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(stats))
}

/// The one-line summary `ekos commit` prints.
pub fn summary_line(s: &QuestionStats) -> String {
    let types = s
        .by_type
        .iter()
        .map(|(k, n)| format!("{n} {k}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} open question(s){}; {} code-vs-data finding(s) — `ekos questions list`",
        s.derived,
        if types.is_empty() {
            String::new()
        } else {
            format!(" ({types})")
        },
        s.code_vs_data
    )
}

// ── Routing state ────────────────────────────────────────────────────────────────────────────

/// `<state>/questions/routing.json`.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Routing {
    /// Question id → the person it was handed to explicitly.
    #[serde(default)]
    assigned: BTreeMap<String, String>,
    /// Question id → people who said "not me".
    #[serde(default)]
    declined: BTreeMap<String, Vec<String>>,
    /// Person → table patterns they muted.
    #[serde(default)]
    muted: BTreeMap<String, Vec<String>>,
}

fn load_routing(config: &EkosConfig, cwd: &Path) -> Routing {
    std::fs::read(routing_path(config, cwd))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_routing(config: &EkosConfig, cwd: &Path, r: &Routing) -> Result<()> {
    std::fs::create_dir_all(dir(config, cwd))?;
    std::fs::write(routing_path(config, cwd), serde_json::to_vec_pretty(r)?)?;
    Ok(())
}

fn glob(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.to_lowercase(), text.to_lowercase());
    match p.split_once('*') {
        None => p == t,
        Some((head, rest)) => {
            t.starts_with(head)
                && (rest.is_empty()
                    || (0..=t.len() - head.len()).any(|i| glob(rest, &t[head.len() + i..])))
        }
    }
}

/// Who a question is with now: the person it was handed to, else the first addressee who did not
/// decline it and has not muted its table.
fn current_addressee(q: &KirObject, r: &Routing) -> Option<String> {
    let id = q.id.to_string();
    if let Some(p) = r.assigned.get(&id) {
        return Some(p.clone());
    }
    let declined = r.declined.get(&id).cloned().unwrap_or_default();
    q.properties
        .get("addressees")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .find(|p| {
            !declined.iter().any(|d| d.eq_ignore_ascii_case(p))
                && !r
                    .muted
                    .get(*p)
                    .is_some_and(|pats| pats.iter().any(|pat| glob(pat, s(q, "table"))))
        })
        .map(str::to_string)
}

// ── Reading ──────────────────────────────────────────────────────────────────────────────────

/// One question as shown: id, type, status, priority, addressee, text, context.
fn view(q: &KirObject, st: Status, addressee: Option<String>, answers: &[&KirObject]) -> Value {
    json!({
        "id": q.id.to_string(),
        "type": s(q, "question_type"),
        "status": st.as_str(),
        "priority": q.properties.get("priority"),
        "blocking": q.properties.get("blocking"),
        "table": s(q, "table"),
        "column": s(q, "column"),
        "text": s(q, "text"),
        "addressee": addressee,
        "addressees": q.properties.get("addressees"),
        "context": q.properties.get("context"),
        "origin": {"kind": s(q, "origin_kind"), "id": s(q, "origin_id")},
        "answers": answers.iter().map(|t| json!({
            "testimony": t.id.to_string(), "author": s(t, "author"), "text": s(t, "text"),
            "channel": s(t, "channel"), "sent_at": s(t, "sent_at"),
        })).collect::<Vec<_>>(),
    })
}

/// Filters for listing.
#[derive(Default)]
pub struct Filter<'a> {
    pub status: Option<&'a str>,
    pub qtype: Option<&'a str>,
    pub table: Option<&'a str>,
    pub addressee: Option<&'a str>,
    pub limit: usize,
}

/// The current questions with their computed status, ranked, filtered.
pub fn read(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
    f: &Filter<'_>,
) -> Result<Vec<Value>> {
    let objects = ledger.all_objects()?;
    let by_id: HashMap<String, &KirObject> =
        objects.iter().map(|o| (o.id.to_string(), o)).collect();
    let current = current_ids(config, cwd);
    let decisions = ekos_semantic::review::decisions(&objects);
    let answers = questions::answers_index(&objects);
    let routing = load_routing(config, cwd);
    let reading = Reading {
        by_id: &by_id,
        questions_current: current.as_ref(),
        decisions: &decisions,
        answers: &answers,
    };
    let mut qs: Vec<&KirObject> = objects.iter().filter(|o| is_question(o)).collect();
    qs.sort_by_key(|q| questions::rank(q));
    qs.dedup_by(|a, b| a.id == b.id);
    let want_status = f.status.unwrap_or("open");
    let mut out = Vec::new();
    for q in qs {
        let st = questions::status(q, &reading);
        if want_status != "all" && st.as_str() != want_status {
            continue;
        }
        if f.qtype.is_some_and(|t| s(q, "question_type") != t)
            || f.table
                .is_some_and(|t| !s(q, "table").eq_ignore_ascii_case(t))
        {
            continue;
        }
        let who = current_addressee(q, &routing);
        if f.addressee
            .is_some_and(|a| who.as_deref().is_none_or(|w| !w.eq_ignore_ascii_case(a)))
        {
            continue;
        }
        let ans = answers.get(&q.id.to_string()).cloned().unwrap_or_default();
        out.push(view(q, st, who, &ans));
        if f.limit > 0 && out.len() >= f.limit {
            break;
        }
    }
    Ok(out)
}

/// `ekos questions list`.
pub fn list(config: &EkosConfig, cwd: &Path, f: &Filter<'_>, json_out: bool) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let qs = read(config, cwd, &*ledger, f)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&qs)?);
        return Ok(());
    }
    if qs.is_empty() {
        println!("No {} questions.", f.status.unwrap_or("open"));
        return Ok(());
    }
    for q in &qs {
        println!(
            "{} [{}{}] {} — {}",
            &q["id"].as_str().unwrap_or("")[..8],
            q["type"].as_str().unwrap_or(""),
            if q["blocking"] == true {
                ", blocking"
            } else {
                ""
            },
            q["text"].as_str().unwrap_or(""),
            q["addressee"]
                .as_str()
                .map_or("no addressee".to_string(), |a| format!("ask {a}"))
        );
    }
    println!("{} question(s).", qs.len());
    Ok(())
}

fn find<'a>(qs: &'a [Value], id: &str) -> Result<&'a Value> {
    let m: Vec<&Value> = qs
        .iter()
        .filter(|q| q["id"].as_str().is_some_and(|x| x.starts_with(id)))
        .collect();
    match m.as_slice() {
        [one] => Ok(one),
        [] => bail!("no question {id} (ekos questions list --status all)"),
        _ => bail!("{id} names {} questions; give more of the id", m.len()),
    }
}

/// `ekos questions show <id> [--reveal]`.
pub fn show(config: &EkosConfig, cwd: &Path, id: &str, reveal: bool, json_out: bool) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let all = read(
        config,
        cwd,
        &*ledger,
        &Filter {
            status: Some("all"),
            ..Default::default()
        },
    )?;
    let mut q = find(&all, id)?.clone();
    if reveal {
        q["revealed"] = reveal_values(config, cwd, &*ledger, &q)?;
    }
    if json_out {
        println!("{}", serde_json::to_string_pretty(&q)?);
        return Ok(());
    }
    println!(
        "{} ({}, {})",
        q["text"].as_str().unwrap_or(""),
        q["type"].as_str().unwrap_or(""),
        q["status"].as_str().unwrap_or("")
    );
    println!("  id: {}", q["id"].as_str().unwrap_or(""));
    println!("  ask: {}", q["addressees"]);
    println!("  context: {}", q["context"]);
    for a in q["answers"].as_array().into_iter().flatten() {
        println!(
            "  answered by {} ({}): {}",
            a["author"].as_str().unwrap_or(""),
            a["channel"].as_str().unwrap_or(""),
            a["text"].as_str().unwrap_or("")
        );
    }
    if let Some(r) = q.get("revealed") {
        println!("  values (live, not stored): {r}");
    }
    Ok(())
}

/// RFC 0254 §3.2: the top values of a column no code names, live, for the person reading.
/// Counts only for a personal column; a value in fewer than `min_cell` rows is not shown.
fn reveal_values(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeStore,
    q: &Value,
) -> Result<Value> {
    if q["type"] != "unexplained_values" {
        bail!("only an `unexplained_values` question has values to reveal");
    }
    let ctx = &q["context"];
    let db_name = ctx["database"]
        .as_str()
        .ok_or_else(|| anyhow!("the question names no database"))?;
    let db = config
        .databases
        .iter()
        .find(|d| d.name == db_name)
        .ok_or_else(|| anyhow!("no database {db_name:?} in this project"))?;
    let (table, column) = (
        q["table"].as_str().unwrap_or(""),
        q["column"].as_str().unwrap_or(""),
    );
    let objects = ledger.all_objects()?;
    let pii_confirmed =
        ekos_semantic::review::decisions(&objects)
            .iter()
            .any(|((subj, item), d)| {
                subj == "pii"
                    && item.to_lowercase().ends_with(&format!("{table}.{column}"))
                    && d.properties.get("decision").and_then(|x| x["pii"].as_str())
                        == Some("confirm")
            });
    if pii_confirmed || ekos_common::pii::classify_name(column).is_some() {
        return Ok(json!({"withheld": "a personal-data column: counts only (see the question)"}));
    }
    let t = objects
        .iter()
        .find(|o| {
            o.kind == ObjectKind::Table
                && s(o, "origin") == "live"
                && s(o, "database") == db_name
                && o.name.eq_ignore_ascii_case(table)
        })
        .ok_or_else(|| anyhow!("no live table {table} in {db_name}"))?;
    let real_column = t
        .properties
        .get("columns")
        .and_then(Value::as_array)
        .and_then(|cs| {
            cs.iter()
                .filter_map(|c| c["name"].as_str())
                .find(|n| n.eq_ignore_ascii_case(column))
        })
        .ok_or_else(|| anyhow!("no column {column} in {table}"))?
        .to_string();
    let items = super::semantics::items_in(config, cwd, ledger)?;
    let literals: Vec<String> = super::profile::code_literals_for(&items, table, column);
    let flavor = super::profile::flavor_of(db.kind);
    let sql = ekos_observation_sdk::signals::reveal_sql(
        flavor,
        s(t, "owner"),
        &t.name,
        &real_column,
        &literals,
        config.questions.reveal_top,
    )
    .map_err(|e| anyhow!(e))?;
    let rows = tokio::task::block_in_place(|| {
        let (worker, status) = ProbeWorker::start(&Target::Database(db.clone()), false);
        let worker = worker.ok_or_else(|| match status {
            Some(Err(e)) => anyhow!(ekos_common::redaction::redact(
                &e,
                &config.redaction_config()
            )),
            _ => anyhow!("not connected"),
        })?;
        use ekos_sqlgen::probe::Probe;
        worker
            .preview(&sql, config.questions.reveal_top)
            .map(|p| p.rows)
            .map_err(|e| {
                anyhow!(ekos_common::redaction::redact(
                    &e,
                    &config.redaction_config()
                ))
            })
    })?;
    Ok(Value::Array(ekos_observation_sdk::signals::suppress_small(
        &rows,
        config.questions.min_cell,
        &config.redaction_config(),
    )))
}

/// `ekos questions export --format md|csv|json`: a questionnaire grouped by table.
pub fn export(
    config: &EkosConfig,
    cwd: &Path,
    f: &Filter<'_>,
    format: &str,
    out: Option<&Path>,
) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let qs = read(config, cwd, &*ledger, f)?;
    let text = match format {
        "json" => serde_json::to_string_pretty(&qs)?,
        "csv" => {
            let mut w = String::from("id,table,column,type,question,ask,answer\n");
            let q = |x: &str| format!("\"{}\"", x.replace('"', "\"\""));
            for x in &qs {
                w.push_str(&format!(
                    "{},{},{},{},{},{},\n",
                    q(x["id"].as_str().unwrap_or("")),
                    q(x["table"].as_str().unwrap_or("")),
                    q(x["column"].as_str().unwrap_or("")),
                    q(x["type"].as_str().unwrap_or("")),
                    q(x["text"].as_str().unwrap_or("")),
                    q(x["addressee"].as_str().unwrap_or("")),
                ));
            }
            w
        }
        "md" => markdown(&qs),
        other => bail!("--format {other}: md, csv or json"),
    };
    match out {
        Some(p) => std::fs::write(p, text).with_context(|| format!("writing {}", p.display()))?,
        None => print!("{text}"),
    }
    Ok(())
}

fn markdown(qs: &[Value]) -> String {
    let mut by_table: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for q in qs {
        by_table
            .entry(q["table"].as_str().unwrap_or("").to_string())
            .or_default()
            .push(q);
    }
    let mut out = String::from(
        "# Questions for the source analysis\n\nAnswer under each question; keep the id.\n",
    );
    for (t, items) in by_table {
        out.push_str(&format!(
            "\n## {}\n",
            if t.is_empty() { "(project)" } else { &t }
        ));
        for q in items {
            out.push_str(&format!(
                "\n### {}\n\n- id: `{}`\n- type: {}\n- ask: {}\n\n**Answer:**\n\n",
                q["text"].as_str().unwrap_or(""),
                q["id"].as_str().unwrap_or(""),
                q["type"].as_str().unwrap_or(""),
                q["addressee"].as_str().unwrap_or("—")
            ));
        }
    }
    out
}

/// `ekos questions digest`: per person, at most `per-person-per-day` open questions, highest
/// priority first. Delivery through channels is RFC 0253's; this prints (or emits JSON for them).
pub fn digest(config: &EkosConfig, cwd: &Path, json_out: bool) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let qs = read(
        config,
        cwd,
        &*ledger,
        &Filter {
            status: Some("open"),
            ..Default::default()
        },
    )?;
    let cap = config.questions.per_person_per_day.max(1);
    let mut per: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    let mut unrouted = 0usize;
    for q in &qs {
        match q["addressee"].as_str() {
            Some(p) => {
                let v = per.entry(p.to_string()).or_default();
                if v.len() < cap {
                    v.push(q);
                }
            }
            None => unrouted += 1,
        }
    }
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"per_person": per, "unrouted": unrouted, "cap": cap})
            )?
        );
        return Ok(());
    }
    for (p, items) in &per {
        println!("{p}: {} question(s)", items.len());
        for q in items {
            println!(
                "  - {} ({})",
                q["text"].as_str().unwrap_or(""),
                &q["id"].as_str().unwrap_or("")[..8]
            );
        }
    }
    if unrouted > 0 {
        println!(
            "{unrouted} open question(s) have nobody to ask: set [questions] owners or reviewers."
        );
    }
    Ok(())
}

/// `ekos questions reassign <id> --to <person>`.
pub fn reassign(config: &EkosConfig, cwd: &Path, id: &str, to: &str) -> Result<()> {
    let full = resolve_id(config, cwd, id)?;
    let mut r = load_routing(config, cwd);
    r.assigned.insert(full.clone(), to.to_string());
    save_routing(config, cwd, &r)?;
    println!("{} is now with {to}.", &full[..8]);
    Ok(())
}

/// `ekos questions decline <id> --as <person>`: "not me" — the question moves on.
pub fn decline(config: &EkosConfig, cwd: &Path, id: &str, by: &str) -> Result<()> {
    let full = resolve_id(config, cwd, id)?;
    let mut r = load_routing(config, cwd);
    if r.assigned
        .get(&full)
        .is_some_and(|p| p.eq_ignore_ascii_case(by))
    {
        r.assigned.remove(&full);
    }
    let d = r.declined.entry(full.clone()).or_default();
    if !d.iter().any(|x| x.eq_ignore_ascii_case(by)) {
        d.push(by.to_string());
    }
    save_routing(config, cwd, &r)?;
    println!(
        "{by} declined {}; it moves to the next person who may know.",
        &full[..8]
    );
    Ok(())
}

/// `ekos questions mute <pattern> --as <person>`: no questions about matching tables.
pub fn mute(config: &EkosConfig, cwd: &Path, pattern: &str, by: &str) -> Result<()> {
    let mut r = load_routing(config, cwd);
    let v = r.muted.entry(by.to_string()).or_default();
    if !v.contains(&pattern.to_string()) {
        v.push(pattern.to_string());
    }
    save_routing(config, cwd, &r)?;
    println!("{by} gets no questions about {pattern}.");
    Ok(())
}

fn resolve_id(config: &EkosConfig, cwd: &Path, id: &str) -> Result<String> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let all = read(
        config,
        cwd,
        &*ledger,
        &Filter {
            status: Some("all"),
            ..Default::default()
        },
    )?;
    Ok(find(&all, id)?["id"].as_str().unwrap_or("").to_string())
}

/// `ekos check code-vs-data`: recompute from the ledger and the latest profile, and print.
pub fn check(config: &EkosConfig, cwd: &Path, json_out: bool) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let Some(signals) = super::profile::load(config, cwd) else {
        bail!(
            "no profile yet: run `ekos profile run` (and `ekos relationships discover` for lookups)"
        );
    };
    let objects = ledger.all_objects()?;
    let refs = super::relationships::current_ids(config, cwd);
    let g = ekos_semantic::code_vs_data::detect(
        &objects,
        &signals,
        refs.as_ref(),
        ekos_semantic::code_vs_data::Settings {
            min_share: config.code_vs_data.min_share,
        },
    );
    let rows: Vec<Value> = g
        .objects
        .iter()
        .map(|o| json!({"check": s(o, "check"), "table": s(o, "subject_name"), "attribute": s(o, "attribute"),
                        "claims": o.properties.get("claims"), "measured": o.properties.get("measured")}))
        .collect();
    if json_out {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    for r in &rows {
        let claims: Vec<String> = r["claims"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| c["value"].as_str().unwrap_or("").to_string())
            .collect();
        println!(
            "[{}] {}: {}",
            r["check"].as_str().unwrap_or(""),
            r["table"].as_str().unwrap_or(""),
            claims.join(" / ")
        );
        println!("    measured: {}", r["measured"]);
    }
    println!("{} finding(s).", rows.len());
    Ok(())
}

/// MCP `ekos_questions` (read-only): the questions through the caller's store.
pub fn agent_list(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
    args: &Value,
) -> Result<Value> {
    let get = |k: &str| args.get(k).and_then(Value::as_str);
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 200) as usize;
    let qs = read(
        config,
        cwd,
        ledger,
        &Filter {
            status: get("status"),
            qtype: get("type"),
            table: get("table"),
            addressee: get("addressee"),
            limit,
        },
    )?;
    Ok(json!({
        "questions": qs,
        "note": "Derived from what code, data and people have not explained yet. Answering one is a person's act (RFC 0253 testimony); an agent may not answer for them.",
    }))
}
