//! The BIRD adapter (RFC 0240 §2.7): Mini-Dev and Dev, SQLite.
//!
//! Inputs are the question file (`question_id`, `db_id`, `question`, `evidence`, `SQL`,
//! `difficulty`; JSON array or JSON Lines), and per database the `.sqlite` file and its
//! `database_description/*.csv` dictionary. `prepare` extracts only those two kinds of file from
//! the archive — the archive also carries question files and gold SQL, which never leave it — and
//! writes one project source per database: `schema.sql`, the tables as SQLite declares them plus
//! a `COMMENT ON COLUMN` per dictionary row. That is the substitute for a `sqlite` database kind
//! and a `[[data-dictionaries]]` entry until RFC 0236/0237 exist; see the RFC's implementation
//! notes.

use super::context::ValueMatch;
use super::context::{
    DbPack, SchemaInfo, ValueIndex, extract_sql, join_pairs, raw_ddl, render_values, sql_shape,
};
use super::context::{PROFILE_VERSION, Profile, Shown, render_value_groups};
use super::leakage::{self, normalise};
use super::report::BenchReport;
use super::sqlexec::{self, Db, Row};
use super::suite::{self, Suite, SuiteDirs};
use super::{
    BenchAdapter, BenchScenario, ContextKind, ExperimentConfig, ExportTarget, Prediction,
    PrepareCtx, ProjectSpec, RunCtx, Score, Split,
};
use super::{Category, PromptKind};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use ekos_recovery::llm::{LlmRequest, LlmResponse, Message};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The fixed baseline prompt published with Mini-Dev 1024
/// (`baseline_inference/prompt_template.txt`), byte for byte. Every cell uses it; only the schema
/// text and the question block differ.
pub const PROMPT_TEMPLATE: &str = "Task Overview:
You are a data science expert. Below, you are provided with a database schema and a natural language question. Your task is to understand the schema and generate a valid SQL query to answer the question.

Database Engine:
{engine}

Database Schema:
{schema}This schema describes the database's structure, including tables, columns, primary keys, foreign keys, and any relevant relationships or constraints.

Question:
{question_block}

Instructions:
- Make sure you only output the information that is asked in the question. If the question asks for a specific column, make sure to only include that column in the SELECT clause, nothing more.
- The generated query should return all of the information asked in the question without any missing or extra information.
- Before generating the final SQL query, please think through the steps of how to write the query.

Output Format:
In your answer, please enclose the generated SQL query in a code block:
```sql
-- Your SQL query
```

Take a deep breath and think step by step to find the correct SQL query.
";

/// RFC 0241's `ekos-v1` template: the baseline, plus three instructions on columns, row grain
/// and answer order. Only cells with `prompt = "ekos-v1"` use it.
pub const EKOS_V1_TEMPLATE: &str = "Task Overview:
You are a data science expert. Below, you are provided with a database schema and a natural language question. Your task is to understand the schema and generate a valid SQL query to answer the question.

Database Engine:
{engine}

Database Schema:
{schema}This schema describes the database's structure, including tables, columns, primary keys, foreign keys, and any relevant relationships or constraints. Column comments give each column's meaning, its null and distinct counts, and example values.

Question:
{question_block}

Instructions:
- Return exactly the columns the question asks for, in the order it asks for them, and nothing else.
- The generated query should return all of the information asked in the question without any missing or extra information.
- Before counting or averaging, check the grain of each table from its row and distinct counts: when an entity appears in several rows (e.g. one row per printing or per year), count DISTINCT entities if the question is about entities.
- Use the exact spelling of values shown in the schema comments; a comma-separated list column is matched with LIKE '%value%'.
- Keep computations in the form the question states (e.g. a rate as a count divided by a count).

Output Format:
Give the SQL query first, in one code block:
```sql
-- Your SQL query
```
Then explain it in at most three sentences.
";

pub const PROMPT_VERSION: &str = "bird-baseline-1024-v1";

const CHECK_PROMPT: &str = "Running that query on the database {problem}. If the question expects \
                            rows, the filters, joins or value spellings are likely wrong. Correct \
                            the query, or repeat it if an empty result is right. Answer with the \
                            SQL in one ```sql code block.";

const REPAIR_PROMPT: &str = "Running that query on the database failed:\n{error}\n\nCorrect the \
                             query. Answer with the corrected SQL in one ```sql code block.";

/// The separator the official evaluator splits a prediction on (`evaluation_utils.package_sqls`).
pub const BIRD_SEPARATOR: &str = "\t----- bird -----\t";

/// The file name `prepare` writes into each project.
pub const SCHEMA_FILE: &str = "schema.sql";

const PROJECT_CONFIG: &str = r#"
# RFC 0240: generated by `ekos eval bench prepare`. Identifiers are double-quoted, which the
# postgres dialect reads; the file holds only DDL and COMMENT ON statements.
[[recover.sql.dialect-rules]]
path-glob = "**/*.sql"
dialect = "postgres"
"#;

pub struct BirdAdapter {
    suite: Suite,
    dirs: SuiteDirs,
}

impl BirdAdapter {
    pub fn new(suite: Suite, dirs: SuiteDirs) -> Self {
        Self { suite, dirs }
    }

    fn all_scenarios(&self) -> Result<Vec<BenchScenario>> {
        let path = self.dirs.locate(&self.suite.questions);
        load_questions(&path)
    }

    fn db_dir(&self, db_id: &str) -> PathBuf {
        if self.suite.databases.url.is_some() {
            self.dirs.databases().join(db_id)
        } else {
            self.dirs.locate(&self.suite.databases).join(db_id)
        }
    }

    /// Unpacks `<prefix><db>/<db>.sqlite` and `<prefix><db>/database_description/*.csv` for every
    /// database the questions use. Nothing else in the archive is read.
    fn extract(&self, db_ids: &BTreeSet<String>) -> Result<()> {
        let source = &self.suite.databases;
        let archive = self.dirs.locate(source);
        let marker = self.dirs.databases().join(".extracted");
        let want = source.sha256.clone().unwrap_or_default();
        let complete = db_ids
            .iter()
            .all(|d| self.db_dir(d).join(format!("{d}.sqlite")).is_file());
        if complete && std::fs::read_to_string(&marker).ok().as_deref() == Some(want.as_str()) {
            return Ok(());
        }
        let prefix = source.prefix.clone().unwrap_or_default();
        let file = std::fs::File::open(&archive)
            .with_context(|| format!("opening {}", archive.display()))?;
        let mut zip =
            zip::ZipArchive::new(file).with_context(|| format!("reading {}", archive.display()))?;
        let mut written = 0usize;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let Some(name) = entry.enclosed_name() else {
                continue;
            };
            let name = name.to_string_lossy().replace('\\', "/");
            let Some(rest) = name.strip_prefix(&prefix) else {
                continue;
            };
            let Some(out) = allowlisted(rest, db_ids) else {
                continue;
            };
            let out = self.dirs.databases().join(out);
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut f = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut f)?;
            written += 1;
        }
        for d in db_ids {
            if !self.db_dir(d).join(format!("{d}.sqlite")).is_file() {
                bail!("{} has no {prefix}{d}/{d}.sqlite", archive.display());
            }
        }
        tracing::info!("extracted {written} files from {}", archive.display());
        std::fs::write(&marker, want)?;
        Ok(())
    }
}

/// `<db>/<db>.sqlite` or `<db>/database_description/<x>.csv`, for a database in `db_ids`; any
/// other archive entry is `None`.
fn allowlisted(rest: &str, db_ids: &BTreeSet<String>) -> Option<String> {
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        [db, file] if db_ids.contains(*db) && *file == format!("{db}.sqlite") => {
            Some(format!("{db}/{file}"))
        }
        [db, "database_description", file]
            if db_ids.contains(*db)
                && file.to_lowercase().ends_with(".csv")
                && !file.starts_with('.') =>
        {
            Some(format!("{db}/database_description/{file}"))
        }
        _ => None,
    }
}

/// Reads a BIRD question file: a JSON array (the original releases) or JSON Lines (Mini-Dev
/// 1024 on Hugging Face).
pub fn load_questions(path: &Path) -> Result<Vec<BenchScenario>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let rows: Vec<serde_json::Value> = if text.trim_start().starts_with('[') {
        serde_json::from_str(&text)?
    } else {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?
    };
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let s = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let id = match r.get("question_id") {
                Some(serde_json::Value::Number(n)) => n.to_string(),
                Some(serde_json::Value::String(s)) => s.clone(),
                _ => i.to_string(),
            };
            let sql = r
                .get("SQL")
                .or_else(|| r.get("sql"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("{}: question {id} has no SQL", path.display()))?;
            Ok(BenchScenario {
                id,
                db_id: s("db_id"),
                question: s("question"),
                evidence: s("evidence"),
                gold_sql: sql.to_string(),
                difficulty: s("difficulty"),
            })
        })
        .collect()
}

// ── Project sources ──────────────────────────────────────────────────────────────────────────

fn dq(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// One dictionary row: column, description, value description.
type DictRow = (String, String, String);

/// `database_description/<table>.csv`, as written by BIRD: UTF-8 with a BOM, or Latin-1.
fn read_dictionary(desc_dir: &Path) -> Result<BTreeMap<String, Vec<DictRow>>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(desc_dir) else {
        return Ok(out);
    };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    files.sort();
    for path in files {
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
        {
            continue;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)?.read_to_end(&mut bytes)?;
        let text = match String::from_utf8(bytes.clone()) {
            Ok(t) => t,
            Err(_) => bytes.iter().map(|b| *b as char).collect(),
        };
        let text = text.trim_start_matches('\u{feff}');
        let rows = parse_csv(text);
        let Some(header) = rows.first() else {
            continue;
        };
        let col = |name: &str| {
            header
                .iter()
                .position(|h| h.trim().eq_ignore_ascii_case(name))
        };
        let (Some(c_name), Some(c_desc)) = (col("original_column_name"), col("column_description"))
        else {
            continue;
        };
        let c_values = col("value_description");
        let table = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let entries = rows[1..]
            .iter()
            .filter_map(|r| {
                let get = |i: usize| r.get(i).map(|v| v.trim().to_string()).unwrap_or_default();
                let name = get(c_name);
                (!name.is_empty())
                    .then(|| (name, get(c_desc), c_values.map(get).unwrap_or_default()))
            })
            .collect();
        out.insert(table.to_lowercase(), entries);
    }
    Ok(out)
}

/// RFC 4180 CSV: quoted fields, doubled quotes, newlines inside quotes.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// The project source for one database: its tables (from SQLite's own catalogue, so the names
/// are the database's) and one `COMMENT ON COLUMN` per dictionary row that names a real column.
/// Also returns the normalised dictionary text.
pub fn project_schema(db: &Db, desc_dir: &Path) -> Result<(String, String)> {
    let schema = SchemaInfo::read(db)?;
    let dictionary = read_dictionary(desc_dir)?;
    let mut out = String::from(
        "-- Generated by `ekos eval bench prepare` (RFC 0240) from the database file and\n-- its database_description/ dictionary. Do not edit.\n\n",
    );
    let mut dict_text = String::new();
    for t in &schema.table_names {
        let cols = &schema.tables[&t.to_lowercase()];
        let mut body: Vec<String> = cols
            .iter()
            .map(|(c, ty, _)| {
                let ty = if ty.trim().is_empty() {
                    "TEXT"
                } else {
                    ty.trim()
                };
                format!("  {} {ty}", dq(c))
            })
            .collect();
        let mut pk: Vec<&(String, String, i64)> = cols.iter().filter(|c| c.2 > 0).collect();
        pk.sort_by_key(|c| c.2);
        if !pk.is_empty() {
            let names: Vec<String> = pk.iter().map(|c| dq(&c.0)).collect();
            body.push(format!("  PRIMARY KEY ({})", names.join(", ")));
        }
        for (from, to_table, to_col) in foreign_keys(db, t)? {
            let to_col = to_col.unwrap_or_else(|| from.clone());
            body.push(format!(
                "  FOREIGN KEY ({}) REFERENCES {} ({})",
                dq(&from),
                dq(&to_table),
                dq(&to_col)
            ));
        }
        out.push_str(&format!(
            "CREATE TABLE {} (\n{}\n);\n\n",
            dq(t),
            body.join(",\n")
        ));
        let Some(rows) = dictionary.get(&t.to_lowercase()) else {
            continue;
        };
        for (name, desc, values) in rows {
            let Some((col, _, _)) = cols
                .iter()
                .find(|(c, _, _)| c.trim().eq_ignore_ascii_case(name.trim()))
            else {
                continue;
            };
            let desc = one_line(desc);
            let values = one_line(values);
            dict_text.push_str(&format!("{desc} {values} "));
            let text = match (desc.is_empty(), values.is_empty()) {
                (true, true) => continue,
                (false, true) => desc,
                (true, false) => format!("values: {values}"),
                (false, false) => format!("{desc}; values: {values}"),
            };
            out.push_str(&format!(
                "COMMENT ON COLUMN {}.{} IS '{}';\n",
                dq(t),
                dq(col),
                text.replace('\'', "''")
            ));
        }
        out.push('\n');
    }
    Ok((out, normalise(&dict_text)))
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `(column, referenced table, referenced column)` per foreign key of `table`.
fn foreign_keys(db: &Db, table: &str) -> Result<Vec<(String, String, Option<String>)>> {
    let mut stmt = db
        .connection()
        .prepare(&format!("PRAGMA foreign_key_list({})", dq(table)))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(3)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Foreign keys of every table as `a.x = b.y` — the join paths raw DDL states.
fn ddl_joins(db: &Db, schema: &SchemaInfo) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for t in &schema.table_names {
        for (from, to, to_col) in foreign_keys(db, t)? {
            let to_col = to_col.unwrap_or_else(|| from.clone());
            out.push(format!("{t}.{from} = {to}.{to_col}"));
        }
    }
    Ok(out)
}

/// Fills `{engine}`, `{schema}` and `{question_block}` in one pass, so text inside one value is
/// never read as a placeholder.
pub fn fill_prompt(engine: &str, schema: &str, question_block: &str) -> String {
    fill_prompt_with(PROMPT_TEMPLATE, engine, schema, question_block)
}

/// [`fill_prompt`] for any template with the same placeholders.
pub fn fill_prompt_with(
    template: &str,
    engine: &str,
    schema: &str,
    question_block: &str,
) -> String {
    let mut out = String::with_capacity(template.len() + schema.len() + 256);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let (value, len) = if tail.starts_with("{engine}") {
            (engine, "{engine}".len())
        } else if tail.starts_with("{schema}") {
            (schema, "{schema}".len())
        } else if tail.starts_with("{question_block}") {
            (question_block, "{question_block}".len())
        } else {
            ("{", 1)
        };
        out.push_str(value);
        rest = &tail[len..];
    }
    out.push_str(rest);
    out
}

/// The baseline's question block: the evidence line first, when there is one and it is on.
pub fn question_block(s: &BenchScenario, evidence: bool) -> String {
    if evidence && !s.evidence.trim().is_empty() {
        format!("{}\n{}", s.evidence.trim(), s.question.trim())
    } else {
        s.question.trim().to_string()
    }
}

/// A canonical key for a result set, for voting between candidates.
fn result_key(rows: &[Row]) -> Vec<Row> {
    let mut set: Vec<Row> = rows.to_vec();
    set.sort();
    set.dedup();
    set
}

/// One model call, counted on `p`: tokens processed, tokens billed, and whether the LLM cache
/// answered it (RFC 0241 §2.1).
async fn call(rt: &RunCtx<'_>, p: &mut Prediction, req: &LlmRequest<'_>) -> Result<LlmResponse> {
    let hits_before = rt.llm.cache_stats().map(|(hits, _)| hits);
    let resp = rt.llm.complete(req).await?;
    let hits_after = rt.llm.cache_stats().map(|(hits, _)| hits);
    p.calls += 1;
    p.model = resp.model.clone();
    p.input_tokens += resp.input_tokens as u64;
    p.output_tokens += resp.output_tokens as u64;
    if matches!((hits_before, hits_after), (Some(b), Some(a)) if a > b) {
        p.cached_calls += 1;
    } else {
        p.billed_input_tokens += resp.input_tokens as u64;
        p.billed_output_tokens += resp.output_tokens as u64;
    }
    Ok(resp)
}

/// The schema text for one candidate (RFC 0241 §2.3): variant 0 is the cell's context; 1 the
/// pack without example values; 2 raw DDL followed by the column profiles.
fn render_schema(
    context: ContextKind,
    variant: usize,
    h: &super::DbHandle,
    pack: Option<&DbPack>,
    profile: Option<&Profile>,
    empties: bool,
) -> Result<String> {
    let shown = Shown {
        examples: true,
        empties,
    };
    let raw = || -> Result<String> {
        let mut text = raw_ddl(&h.db)?;
        if let Some(prof) = profile {
            text.push_str(&prof.render_block(&h.schema, shown));
        }
        Ok(text)
    };
    Ok(match (variant, context, pack) {
        (0, ContextKind::EkosPack, Some(pack)) => pack.render_with(profile, shown),
        (1, _, Some(pack)) => pack.render_with(
            profile,
            Shown {
                examples: false,
                ..shown
            },
        ),
        _ => raw()?,
    })
}

/// A candidate answer: its SQL and, when it ran, its rows.
type Candidate = (String, Option<Vec<Row>>);

/// `value`, or the error that `prepare` has not built `what` for `db_id`.
fn required<'a, T>(value: Option<&'a T>, what: &str, db_id: &str) -> Result<&'a T> {
    value.ok_or_else(|| anyhow!("no {what} for {db_id} — run `ekos eval bench prepare`"))
}

/// What the model is shown as tables, columns and join paths, recorded for the measures.
fn set_context(
    cfg: &ExperimentConfig,
    h: &super::DbHandle,
    db_id: &str,
    p: &mut Prediction,
) -> Result<()> {
    match cfg.context {
        ContextKind::RawDdl => {
            p.context_tables = h.schema.tables.keys().cloned().collect();
            p.context_columns = h
                .schema
                .tables
                .values()
                .flat_map(|c| c.iter().map(|(n, _, _)| n.to_lowercase()))
                .collect();
            p.context_joins = ddl_joins(&h.db, &h.schema)?;
        }
        ContextKind::EkosPack => {
            let pack = required(h.pack.as_ref(), "EKOS pack", db_id)?;
            p.context_tables = pack.table_names();
            p.context_columns = pack.column_names();
            p.context_joins = pack.joins.clone();
        }
    }
    Ok(())
}

/// The value-lookup text for the prompt (empty when the cell has none); records the matches.
fn lookup_values(
    cfg: &ExperimentConfig,
    h: &super::DbHandle,
    s: &BenchScenario,
    p: &mut Prediction,
) -> Result<String> {
    if !cfg.value_lookup {
        return Ok(String::new());
    }
    let index = required(h.values.as_ref(), "value index", &s.db_id)?;
    if cfg.value_lookup_version < 2 {
        p.values = index.lookup(&s.question);
        return Ok(render_values(&p.values));
    }
    let groups = index.lookup_v2(&s.question, h.profile.as_ref());
    p.values = groups
        .iter()
        .flat_map(|g| {
            g.columns.iter().map(|(t, c, _)| ValueMatch {
                table: t.clone(),
                column: c.clone(),
                value: g.value.clone(),
            })
        })
        .collect();
    Ok(render_value_groups(&groups))
}

/// What to do with one answer: keep it, or send it back with feedback.
enum Step {
    Accept(String, Vec<Row>),
    Retry { feedback: String, error: String },
}

fn judge(
    sql: Option<&String>,
    h: &super::DbHandle,
    cfg: &ExperimentConfig,
    may_retry: bool,
) -> Step {
    let Some(q) = sql else {
        let error = "the answer held no SQL code block".to_string();
        return Step::Retry {
            feedback: REPAIR_PROMPT.replace("{error}", &error),
            error,
        };
    };
    match h.db.query(q, cfg.limits()) {
        Ok(rows) => match result_problem(&rows).filter(|_| cfg.result_checks && may_retry) {
            Some(why) => Step::Retry {
                feedback: CHECK_PROMPT.replace("{problem}", why),
                error: why.to_string(),
            },
            None => Step::Accept(q.clone(), rows),
        },
        Err(e) => Step::Retry {
            feedback: REPAIR_PROMPT.replace("{error}", &e.to_string()),
            error: e.to_string(),
        },
    }
}

/// One candidate: ask, then up to `r` repair rounds. `None` when no SQL was ever given.
async fn answer_candidate(
    rt: &RunCtx<'_>,
    cfg: &ExperimentConfig,
    h: &super::DbHandle,
    prompt: &str,
    p: &mut Prediction,
) -> Result<Option<Candidate>> {
    let req = LlmRequest {
        system: "",
        user: prompt,
        prompt_version: PROMPT_VERSION,
        max_tokens: cfg.max_tokens,
        history: &[],
    };
    let mut resp = call(rt, p, &req).await?;
    let mut sql = extract_sql(&resp.content);
    let mut repairs = 0;
    loop {
        let (feedback, error) = match judge(sql.as_ref(), h, cfg, repairs < cfg.r) {
            Step::Accept(q, rows) => return Ok(Some((q, Some(rows)))),
            Step::Retry { feedback, error } => (feedback, error),
        };
        if repairs >= cfg.r {
            p.error = Some(error);
            if sql.is_none() {
                p.truncated = resp.output_tokens >= cfg.max_tokens;
            }
            return Ok(sql.map(|q| (q, None)));
        }
        repairs += 1;
        p.repairs += 1;
        let history = [
            Message {
                role: "user",
                content: prompt,
            },
            Message {
                role: "assistant",
                content: &resp.content,
            },
        ];
        let req = LlmRequest {
            system: "",
            user: &feedback,
            prompt_version: PROMPT_VERSION,
            max_tokens: cfg.max_tokens,
            history: &history,
        };
        resp = call(rt, p, &req).await?;
        sql = extract_sql(&resp.content);
    }
}

/// Self-consistency: the candidate whose result most executed candidates share; ties go to the
/// earliest; with none executed, the first candidate.
fn vote(candidates: &[Candidate]) -> Option<usize> {
    let mut votes: Vec<(Vec<Row>, usize, usize)> = Vec::new();
    for (i, (_, rows)) in candidates.iter().enumerate() {
        let Some(rows) = rows else { continue };
        let key = result_key(rows);
        match votes.iter_mut().find(|v| v.0 == key) {
            Some(v) => v.1 += 1,
            None => votes.push((key, 1, i)),
        }
    }
    votes
        .iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)))
        .map(|v| v.2)
        .or(if candidates.is_empty() { None } else { Some(0) })
}

/// What a result check sends back (RFC 0241 §2.3); never anything about the gold answer.
fn result_problem(rows: &[Row]) -> Option<&'static str> {
    if rows.is_empty() {
        Some("ran, but returned no rows")
    } else if rows.len() == 1 && rows[0].iter().all(|c| *c == sqlexec::Cell::Null) {
        Some("ran, but returned a single NULL")
    } else {
        None
    }
}

/// Why a prediction that ran is right or wrong.
fn category(predicted: &[Row], gold: &[Row], ex: bool) -> Category {
    if ex {
        return Category::Correct;
    }
    let width = |rows: &[Row]| rows.first().map(Vec::len);
    match (width(predicted), width(gold)) {
        (Some(a), Some(b)) if a != b => Category::WrongColumns,
        _ => {
            let distinct =
                |rows: &[Row]| rows.iter().collect::<std::collections::HashSet<_>>().len();
            if distinct(predicted) != distinct(gold) {
                Category::WrongRows
            } else {
                Category::WrongValues
            }
        }
    }
}

#[async_trait(?Send)]
impl BenchAdapter for BirdAdapter {
    fn id(&self) -> &'static str {
        "bird"
    }

    fn prepare(&self, ctx: &PrepareCtx) -> Result<Vec<ProjectSpec>> {
        if self.suite.dialect != "sqlite" {
            bail!(
                "suite {:?} is {}; only sqlite suites run today (RFC 0240 §2.7)",
                self.suite.name,
                self.suite.dialect
            );
        }
        suite::verify(
            &self.suite.questions,
            &self.dirs.locate(&self.suite.questions),
        )?;
        let scenarios = self.all_scenarios()?;
        let db_ids: BTreeSet<String> = scenarios.iter().map(|s| s.db_id.clone()).collect();
        if self.suite.databases.url.is_some() {
            suite::verify(
                &self.suite.databases,
                &self.dirs.locate(&self.suite.databases),
            )?;
            self.extract(&db_ids)?;
        }
        let needles = leakage::Needles::from_scenarios(&scenarios);
        std::fs::create_dir_all(self.dirs.values())?;
        let mut specs = Vec::new();
        for db_id in &db_ids {
            let db_path = self.database_path(db_id);
            let db = Db::open(&db_path)?;
            let (schema_sql, dictionary_text) =
                project_schema(&db, &self.db_dir(db_id).join("database_description"))?;
            let source_dir = self.dirs.projects().join(db_id);
            std::fs::create_dir_all(&source_dir)?;
            let schema_path = source_dir.join(SCHEMA_FILE);
            if std::fs::read_to_string(&schema_path).ok().as_deref() != Some(schema_sql.as_str()) {
                std::fs::write(&schema_path, &schema_sql)?;
            }
            let files: BTreeSet<String> = [SCHEMA_FILE.to_string()].into();
            leakage::check_allowlist(&source_dir, &files)?;
            let hits = leakage::without_shipped(
                leakage::scan_files(&source_dir, &needles)?,
                &scenarios,
                &dictionary_text,
            );
            if !hits.is_empty() {
                bail!(
                    "leakage: {} benchmark text(s) found in project source {} — first: question \
                     {} ({:?})",
                    hits.len(),
                    source_dir.display(),
                    hits[0].question_id,
                    hits[0].kind
                );
            }
            let values_path = self.dirs.values().join(format!("{db_id}.json"));
            if ctx.force || !values_path.is_file() {
                let schema = SchemaInfo::read(&db)?;
                let index = ValueIndex::build(&db, &schema)?;
                std::fs::write(&values_path, serde_json::to_string(&index)?)?;
            }
            let profile_path = self.dirs.values().join(format!("{db_id}.profile.json"));
            let current = std::fs::read_to_string(&profile_path)
                .ok()
                .and_then(|t| serde_json::from_str::<Profile>(&t).ok())
                .is_some_and(|p| p.version >= PROFILE_VERSION);
            if ctx.force || !current {
                let schema = SchemaInfo::read(&db)?;
                let profile = Profile::build(&db, &schema)?;
                std::fs::write(&profile_path, serde_json::to_string(&profile)?)?;
            }
            specs.push(ProjectSpec {
                name: db_id.clone(),
                db_id: db_id.clone(),
                source_dir,
                files,
                config: PROJECT_CONFIG.to_string(),
                source_digest: suite::sha256_hex(schema_sql.as_bytes()),
                dictionary_text,
            });
        }
        Ok(specs)
    }

    fn scenarios(&self, split: Split) -> Result<Vec<BenchScenario>> {
        Ok(super::split::select(
            &self.all_scenarios()?,
            &self.suite.split,
            split,
        ))
    }

    fn database_path(&self, db_id: &str) -> PathBuf {
        self.db_dir(db_id).join(format!("{db_id}.sqlite"))
    }

    async fn run_one(
        &self,
        s: &BenchScenario,
        cfg: &ExperimentConfig,
        rt: &RunCtx<'_>,
    ) -> Result<Prediction> {
        if cfg.few_shot > 0 {
            bail!(
                "few-shot examples come from a train split only, and suite {:?} has none",
                self.suite.name
            );
        }
        let h = rt
            .dbs
            .get(&s.db_id)
            .ok_or_else(|| anyhow!("database {} is not open", s.db_id))?;
        let mut p = Prediction::default();
        set_context(cfg, h, &s.db_id, &mut p)?;
        let profile = if cfg.profiles {
            Some(required(h.profile.as_ref(), "column profile", &s.db_id)?)
        } else {
            None
        };
        let values_text = lookup_values(cfg, h, s, &mut p)?;
        let template = match cfg.prompt {
            PromptKind::Baseline => PROMPT_TEMPLATE,
            PromptKind::EkosV1 => EKOS_V1_TEMPLATE,
        };
        p.prompt_hash = suite::sha256_hex(template.as_bytes());
        let block = question_block(s, cfg.evidence);
        let mut candidates: Vec<Candidate> = Vec::new();
        let started = Instant::now();
        for i in 0..cfg.n {
            let variant = if cfg.vary_context { i % 3 } else { 0 };
            let mut schema_text = render_schema(
                cfg.context,
                variant,
                h,
                h.pack.as_ref(),
                profile,
                cfg.profile_version >= 2,
            )?;
            schema_text.push_str(&values_text);
            let prompt = fill_prompt_with(template, "SQLite", &schema_text, &block);
            if let Some(c) = answer_candidate(rt, cfg, h, &prompt, &mut p).await? {
                candidates.push(c);
            }
        }
        p.latency_ms = started.elapsed().as_millis() as u64;
        let chosen = vote(&candidates);
        p.sql = chosen.map(|i| candidates[i].0.clone());
        if chosen.is_some_and(|i| candidates[i].1.is_some()) {
            p.error = None;
            p.truncated = false;
        }
        Ok(p)
    }

    fn schema_text(&self, cfg: &ExperimentConfig, h: &super::DbHandle) -> Result<String> {
        let profile = if cfg.profiles {
            Some(required(
                h.profile.as_ref(),
                "column profile",
                "this database",
            )?)
        } else {
            None
        };
        render_schema(
            cfg.context,
            0,
            h,
            h.pack.as_ref(),
            profile,
            cfg.profile_version >= 2,
        )
    }

    fn score(
        &self,
        s: &BenchScenario,
        p: &Prediction,
        cfg: &ExperimentConfig,
        rt: &RunCtx<'_>,
    ) -> Result<Score> {
        let h = rt
            .dbs
            .get(&s.db_id)
            .ok_or_else(|| anyhow!("database {} is not open", s.db_id))?;
        let limits = cfg.limits();
        let gold =
            h.db.query(&s.gold_sql, limits)
                .map_err(|e| anyhow!("gold SQL of question {} failed: {e}", s.id))?;
        let mut score = Score::default();
        if let Some(sql) = &p.sql {
            match h.db.query(sql, limits) {
                Ok(rows) => {
                    score.executed = true;
                    score.ex = sqlexec::ex(&rows, &gold);
                    score.category = category(&rows, &gold, score.ex);
                    score.soft_f1 = sqlexec::soft_f1(&rows, &gold);
                    if score.ex && cfg.ves_iterations > 0 {
                        let ratios = sqlexec::time_ratios(
                            &h.db,
                            sql,
                            &s.gold_sql,
                            cfg.ves_iterations,
                            limits,
                        );
                        score.rves_reward = sqlexec::rves_reward(&ratios);
                    }
                    score.grounded = sql_shape(sql).is_some_and(|shape| {
                        shape.tables.iter().all(|t| p.context_tables.contains(t))
                    });
                }
                Err(e) => {
                    score.exec_error = Some(e.to_string());
                    score.category = Category::ExecError;
                }
            }
        } else {
            score.exec_error = p.error.clone();
            score.category = Category::NoSql;
        }
        if let Some(gold_shape) = sql_shape(&s.gold_sql) {
            let tables: Vec<&String> = gold_shape
                .tables
                .iter()
                .filter(|t| h.schema.has_table(t))
                .collect();
            let columns: Vec<&String> = gold_shape
                .columns
                .iter()
                .filter(|c| h.schema.has_column(c))
                .collect();
            let total = tables.len() + columns.len();
            if total > 0 {
                let hit = tables
                    .iter()
                    .filter(|t| p.context_tables.contains(**t))
                    .count()
                    + columns
                        .iter()
                        .filter(|c| p.context_columns.contains(**c))
                        .count();
                score.schema_recall = Some(hit as f64 / total as f64);
            }
            let gold_joins: Vec<&(String, String)> = gold_shape
                .joins
                .iter()
                .filter(|(a, b)| h.schema.has_table(a) && h.schema.has_table(b))
                .collect();
            if !gold_joins.is_empty() {
                let offered = join_pairs(&p.context_joins);
                let hit = gold_joins.iter().filter(|j| offered.contains(**j)).count();
                score.join_recall = Some(hit as f64 / gold_joins.len() as f64);
            }
            if cfg.value_lookup
                && let Some(index) = &h.values
            {
                let known: Vec<&String> = gold_shape
                    .literals
                    .iter()
                    .filter(|l| index.columns.iter().any(|(_, _, v)| v.contains(l)))
                    .collect();
                if !known.is_empty() {
                    let hit = known
                        .iter()
                        .filter(|l| p.values.iter().any(|m| &m.value == **l))
                        .count();
                    score.value_hit = Some(hit as f64 / known.len() as f64);
                }
            }
        }
        Ok(score)
    }

    fn export(&self, run: &BenchReport, target: ExportTarget, out_dir: &Path) -> Result<PathBuf> {
        let ExportTarget::BirdSubmission = target;
        std::fs::create_dir_all(out_dir)?;
        let stem = format!(
            "predict_{}_{}_{}",
            run.suite,
            run.cell.to_lowercase(),
            run.split
        );
        let predict = out_dir.join(format!("{stem}.json"));
        std::fs::write(&predict, submission_json(run))?;
        let metadata = serde_json::json!({
            "method": format!("EKOS ({} cell {})", self.suite.name, run.cell),
            "model": run.model,
            "size": "UNK",
            "oracle_knowledge": run.oracle_knowledge,
            "new_dev": run.new_dev,
            "suite": run.suite,
            "suite_version": run.suite_version,
            "split": run.split,
            "questions": run.n,
            "planned": run.planned,
            "self_consistency_candidates": run.config.n,
            "few_shot": run.few_shot_used,
            "ex_local": run.metrics.ex.mean,
            "config_hash": run.config_hash,
            "prompt_hash": run.prompt_hash,
            "ledger_fingerprints": run.ledger_fingerprints,
            "budget_stopped": run.budget_stopped,
            "contaminated": run.contaminated,
            "predictions_file": predict.file_name().map(|f| f.to_string_lossy().to_string()),
            "format": "JSON object: question_id → \"<SQL>\\t----- bird -----\\t<db_id>\", in question \
                       order — the prediction format of bird-bench/mini_dev evaluation_utils.package_sqls",
            "submission": "BIRD asks for test submissions by e-mail (bird.bench23@gmail.com), following \
                           its submission guideline; sending is a human step. Mark a cleaned-split run \
                           as New Dev.",
        });
        std::fs::write(
            out_dir.join(format!("{stem}.metadata.json")),
            serde_json::to_string_pretty(&metadata)?,
        )?;
        Ok(predict)
    }
}

/// The prediction file, keys in the report's question order (a JSON map would sort them, and the
/// official evaluator pairs predictions with gold lines by position).
pub fn submission_json(run: &BenchReport) -> String {
    let mut out = String::from("{\n");
    for (i, q) in run.questions.iter().enumerate() {
        // Kept as answered: flattening lines would let a trailing `--` comment swallow the rest.
        let sql = q.prediction.sql.as_deref().unwrap_or("");
        let value = format!("{sql}{BIRD_SEPARATOR}{}", q.db_id);
        out.push_str(&format!(
            "    {}: {}{}\n",
            serde_json::Value::String(q.id.clone()),
            serde_json::Value::String(value),
            if i + 1 < run.questions.len() { "," } else { "" }
        ));
    }
    out.push('}');
    out.push('\n');
    out
}

/// The official evaluator's gold file for `scenarios`: `<SQL>\t<db_id>` per line.
pub fn gold_file(scenarios: &[BenchScenario]) -> String {
    scenarios
        .iter()
        .map(|s| format!("{}\t{}\n", one_line(&s.gold_sql), s.db_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_databases_and_dictionaries_leave_the_archive() {
        let dbs: BTreeSet<String> = ["shop".to_string()].into();
        assert_eq!(
            allowlisted("shop/shop.sqlite", &dbs).as_deref(),
            Some("shop/shop.sqlite")
        );
        assert!(allowlisted("shop/database_description/orders.csv", &dbs).is_some());
        for refused in [
            "mini_dev_sqlite.json",
            "mini_dev_sqlite_gold.sql",
            "shop/gold.sql",
            "shop/database_description/../../x.csv",
            "other/other.sqlite",
            "shop/shop.sql",
        ] {
            assert!(allowlisted(refused, &dbs).is_none(), "{refused}");
        }
    }

    #[test]
    fn csv_parsing_handles_quotes_and_newlines() {
        let rows = parse_csv("a,b,c\n\"x, y\",\"he said \"\"hi\"\"\",\"line1\nline2\"\r\n1,2,3");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], vec!["x, y", "he said \"hi\"", "line1\nline2"]);
        assert_eq!(rows[2], vec!["1", "2", "3"]);
    }

    #[test]
    fn the_prompt_is_the_baseline_template_filled_once() {
        let s = BenchScenario {
            id: "1".into(),
            db_id: "d".into(),
            question: "How many {schema}?".into(),
            evidence: "count rows".into(),
            gold_sql: "SELECT 1".into(),
            difficulty: "simple".into(),
        };
        let p = fill_prompt("SQLite", "CREATE TABLE t (a);\n", &question_block(&s, true));
        assert!(p.contains("Database Engine:\nSQLite\n"));
        assert!(p.contains("CREATE TABLE t (a);\nThis schema describes"));
        assert!(p.contains("Question:\ncount rows\nHow many {schema}?\n"));
        let off = question_block(&s, false);
        assert_eq!(off, "How many {schema}?");
    }

    #[test]
    fn submission_keys_keep_question_order() {
        use super::super::{Prediction, QuestionResult, Score};
        let q = |id: &str| QuestionResult {
            id: id.into(),
            db_id: "db".into(),
            difficulty: "simple".into(),
            prediction: Prediction {
                sql: Some("SELECT\n 1".into()),
                ..Default::default()
            },
            score: Score::default(),
        };
        let mut report = super::super::fixture::empty_report();
        report.questions = vec![q("2"), q("10"), q("1")];
        let text = submission_json(&report);
        let keys: Vec<&str> = text
            .lines()
            .filter_map(|l| l.trim().split(':').next())
            .filter(|k| k.starts_with('"'))
            .collect();
        assert_eq!(keys, vec!["\"2\"", "\"10\"", "\"1\""]);
        assert!(
            text.contains("SELECT\\n 1\\t----- bird -----\\tdb"),
            "{text}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.as_object().unwrap().len(), 3);
    }
}
