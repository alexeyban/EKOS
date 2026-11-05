//! A tiny BIRD-shaped suite — two databases, ten questions — and a scripted model, so the whole
//! prepare → run → score → compare → export path runs in `cargo test` without a download or a
//! model (RFC 0240 §4).
//!
//! The scripted model answers like a model that needs help with values: three questions name a
//! value whose spelling only the evidence or question-time value lookup shows ('retail', 'Maths',
//! 'Gryff'); without it the model guesses another spelling and is wrong. One question's first
//! answer does not run, so a repair round is exercised, and one is always wrong.

use super::report::{BenchReport, Metrics};
use super::stats::Interval;
use super::{ContextKind, ExperimentConfig, Split};
use anyhow::Result;
use async_trait::async_trait;
use ekos_recovery::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const SUITE_TOML: &str = r#"name = "fixture"
adapter = "bird"
version = "fixture-1"
dialect = "sqlite"
notes = "RFC 0240 test fixture: two databases, ten questions."

[questions]
file = "questions.jsonl"

[databases]
file = "databases"

[split]
seed = 240
tune-fraction = 0.4
"#;

const SHOP: &str = "
CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, city TEXT, segment TEXT);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, amount REAL, status TEXT,
  FOREIGN KEY (customer_id) REFERENCES customers (id));
INSERT INTO customers VALUES (1,'Ann','Paris','retail'),(2,'Bob','Lyon','corporate'),
  (3,'Cid','Paris','corporate'),(4,'Dee','Nice','retail');
INSERT INTO orders VALUES (1,1,10.5,'paid'),(2,1,20,'paid'),(3,2,5,'open'),(4,3,7.5,'paid'),
  (5,4,100,'refunded');
";

const SCHOOL: &str = "
CREATE TABLE students (sid INTEGER PRIMARY KEY, name TEXT NOT NULL, grade INTEGER, house TEXT);
CREATE TABLE scores (sid INTEGER, subject TEXT, score INTEGER,
  FOREIGN KEY (sid) REFERENCES students (sid));
INSERT INTO students VALUES (1,'Ivy',10,'Gryff'),(2,'Jon',11,'Raven'),(3,'Kim',10,'Raven'),
  (4,'Lou',12,'Gryff');
INSERT INTO scores VALUES (1,'Maths',91),(1,'History',70),(2,'Maths',65),(2,'History',88),
  (3,'Maths',79),(4,'History',95),(4,'Maths',82);
";

const SHOP_CUSTOMERS_CSV: &str =
    "\u{feff}original_column_name,column_name,column_description,data_format,value_description
id,,customer id,integer,
name,,customer name,text,
city,,home city,text,
segment,,customer segment,text,\"retail: a private person; corporate: a company\"
";

const SHOP_ORDERS_CSV: &str =
    "original_column_name,column_name,column_description,data_format,value_description
id,,order id,integer,
customer_id,,the customer who placed the order,integer,
amount,,order amount,real,unit: euro
status,,order status,text,paid / open / refunded
";

const SCHOOL_SCORES_CSV: &str =
    "original_column_name,column_name,column_description,data_format,value_description
sid,,student id,integer,
subject,,the subject,text,
score,,exam score,integer,0 to 100
";

/// `(id, db, difficulty, question, evidence, gold)`.
pub const QUESTIONS: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "0",
        "shop",
        "simple",
        "How many customers live in Paris?",
        "",
        "SELECT COUNT(*) FROM customers WHERE city = 'Paris'",
    ),
    (
        "1",
        "shop",
        "moderate",
        "What is the total amount of paid orders placed by retail customers?",
        "retail customers refers to segment = 'retail'",
        "SELECT SUM(o.amount) FROM orders AS o JOIN customers AS c ON c.id = o.customer_id WHERE c.segment = 'retail' AND o.status = 'paid'",
    ),
    (
        "2",
        "shop",
        "simple",
        "List the names of customers from Lyon.",
        "",
        "SELECT name FROM customers WHERE city = 'Lyon'",
    ),
    (
        "3",
        "shop",
        "challenging",
        "Which customer has the largest refunded order?",
        "largest refers to MAX(amount)",
        "SELECT c.name FROM customers AS c JOIN orders AS o ON o.customer_id = c.id WHERE o.status = 'refunded' ORDER BY o.amount DESC LIMIT 1",
    ),
    (
        "4",
        "shop",
        "moderate",
        "How many open orders are there?",
        "",
        "SELECT COUNT(*) FROM orders WHERE status = 'open'",
    ),
    (
        "5",
        "school",
        "simple",
        "How many students are in grade 10?",
        "",
        "SELECT COUNT(*) FROM students WHERE grade = 10",
    ),
    (
        "6",
        "school",
        "moderate",
        "What is the average maths score?",
        "maths refers to subject = 'Maths'",
        "SELECT AVG(score) FROM scores WHERE subject = 'Maths'",
    ),
    (
        "7",
        "school",
        "simple",
        "List the students in house Gryff.",
        "",
        "SELECT name FROM students WHERE house = 'Gryff'",
    ),
    (
        "8",
        "school",
        "challenging",
        "Which student has the highest score in History?",
        "",
        "SELECT s.name FROM students AS s JOIN scores AS c ON c.sid = s.sid WHERE c.subject = 'History' ORDER BY c.score DESC LIMIT 1",
    ),
    (
        "9",
        "school",
        "moderate",
        "How many students scored above 80 in any subject?",
        "",
        "SELECT COUNT(DISTINCT sid) FROM scores WHERE score > 80",
    ),
];

/// Writes the fixture suite into `suite_dir` (which becomes `evals/bench/fixture`'s shape).
pub fn write_suite(suite_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(suite_dir.join("configs"))?;
    std::fs::write(suite_dir.join("suite.toml"), SUITE_TOML)?;
    let mut lines = String::new();
    for (id, db, difficulty, q, ev, sql) in QUESTIONS {
        lines.push_str(&serde_json::to_string(&serde_json::json!({
            "question_id": id.parse::<u32>().unwrap_or(0), "db_id": db, "question": q,
            "evidence": ev, "SQL": sql, "difficulty": difficulty,
        }))?);
        lines.push('\n');
    }
    std::fs::write(suite_dir.join("questions.jsonl"), lines)?;
    for (db, sql, dict) in [
        (
            "shop",
            SHOP,
            vec![
                ("customers", SHOP_CUSTOMERS_CSV),
                ("orders", SHOP_ORDERS_CSV),
            ],
        ),
        ("school", SCHOOL, vec![("scores", SCHOOL_SCORES_CSV)]),
    ] {
        let dir = suite_dir.join("databases").join(db);
        std::fs::create_dir_all(dir.join("database_description"))?;
        let path = dir.join(format!("{db}.sqlite"));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let conn = rusqlite::Connection::open(&path)?;
        conn.execute_batch(sql)?;
        for (table, csv) in dict {
            std::fs::write(
                dir.join("database_description")
                    .join(format!("{table}.csv")),
                csv,
            )?;
        }
    }
    for (file, cfg) in [
        ("a.toml", cell("A", ContextKind::RawDdl, false, false)),
        ("b.toml", cell("B", ContextKind::RawDdl, true, false)),
        ("c.toml", cell("C", ContextKind::EkosPack, false, true)),
        ("d.toml", cell("D", ContextKind::EkosPack, true, true)),
        (
            "c-prime.toml",
            cell("C-prime", ContextKind::EkosPack, false, false),
        ),
        ("e.toml", rfc_0241_cell("E", false, false, 1)),
        ("f.toml", rfc_0241_cell("F", true, false, 1)),
        ("g.toml", rfc_0241_cell("G", true, true, 1)),
        ("h.toml", rfc_0241_cell("H", true, true, 3)),
        ("j.toml", rfc_0242_cell("J", super::Strategy::Loop)),
        ("k.toml", rfc_0242_cell("K", super::Strategy::Agent)),
    ] {
        std::fs::write(suite_dir.join("configs").join(file), toml::to_string(&cfg)?)?;
    }
    Ok(())
}

fn cell(name: &str, context: ContextKind, evidence: bool, value_lookup: bool) -> ExperimentConfig {
    ExperimentConfig {
        name: name.into(),
        description: String::new(),
        context,
        evidence,
        value_lookup,
        few_shot: 0,
        n: 1,
        r: 1,
        max_tokens: 512,
        ves_iterations: 1,
        timeout_seconds: 10,
        profiles: false,
        profile_version: 1,
        value_lookup_version: 1,
        prompt: super::PromptKind::Baseline,
        result_checks: false,
        vary_context: false,
        strategy: super::Strategy::Single,
        max_turns: 6,
        verify: false,
        checks: Vec::new(),
        llm: serde_json::json!({"provider": "ollama", "model": "fixture"}),
    }
}

/// RFC 0242's cells: G's context with the detector loop (`J`) or the tool-using agent (`K`).
fn rfc_0242_cell(name: &str, strategy: super::Strategy) -> ExperimentConfig {
    let mut c = rfc_0241_cell(name, true, true, 1);
    c.result_checks = false;
    c.strategy = strategy;
    c
}

/// RFC 0241's cells: the pack with profiles; value lookup v2; the ekos-v1 prompt with result
/// checks; and `n` varied-context candidates.
fn rfc_0241_cell(name: &str, values_v2: bool, ekos: bool, n: usize) -> ExperimentConfig {
    let mut c = cell(name, ContextKind::EkosPack, false, values_v2);
    c.profiles = true;
    c.max_tokens = 4096;
    if values_v2 {
        c.value_lookup_version = 2;
    }
    if ekos {
        c.prompt = super::PromptKind::EkosV1;
        c.result_checks = true;
    }
    if n > 1 {
        c.n = n;
        c.vary_context = true;
    }
    c
}

/// What the scripted model answers to a request whose text (prompt plus history) is `text`.
pub fn scripted_answer(text: &str) -> String {
    let Some((id, _, _, _, _, gold)) = QUESTIONS.iter().find(|q| text.contains(q.3)) else {
        return "I cannot tell which question this is.".into();
    };
    let repair = text.contains("Running that query on the database");
    // A result check ("returned no rows" / "a single NULL") makes it look up the spelling.
    let checked = text.contains("returned no rows") || text.contains("returned a single NULL");
    let sql = match *id {
        "1" if !text.contains("'retail'") && !checked => gold.replace("'retail'", "'Retail'"),
        "6" if !text.contains("'Maths'") && !checked => gold.replace("'Maths'", "'maths'"),
        "7" if !text.contains("'Gryff'") && !checked => gold.replace("'Gryff'", "'gryff'"),
        "4" if !repair => "SELECT COUNT(*) FROM order_s WHERE status = 'open'".into(),
        "9" => "SELECT COUNT(*) FROM scores WHERE score > 80".into(),
        _ => gold.to_string(),
    };
    format!("Let me think step by step.\n```sql\n{sql};\n```\n")
}

/// The scripted model as an [`LlmProvider`]; refuses for budget after `budget_calls` calls.
pub struct ScriptedLlm {
    pub calls: AtomicUsize,
    pub budget_calls: Option<usize>,
}

impl ScriptedLlm {
    pub fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            budget_calls: None,
        }
    }
}

impl Default for ScriptedLlm {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for ScriptedLlm {
    fn model_name(&self) -> &str {
        "scripted"
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.budget_calls.is_some_and(|limit| n >= limit) {
            return Err(LlmError::Budget("the fixture budget is spent".into()));
        }
        let mut text = String::new();
        for m in req.history {
            text.push_str(m.content);
            text.push('\n');
        }
        text.push_str(req.user);
        Ok(LlmResponse {
            content: scripted_answer(&text),
            model: "scripted".into(),
            input_tokens: (text.len() / 4) as u32,
            output_tokens: 20,
            ..Default::default()
        })
    }
}

/// A report with nothing in it, for tests that fill only what they need.
pub fn empty_report() -> BenchReport {
    let zero = Interval {
        n: 0,
        mean: 0.0,
        low: 0.0,
        high: 0.0,
    };
    BenchReport {
        report_version: super::report::REPORT_VERSION,
        suite: "fixture".into(),
        suite_version: "fixture-1".into(),
        adapter: "bird".into(),
        new_dev: false,
        cell: "A".into(),
        config: cell("A", ContextKind::RawDdl, false, false),
        config_hash: "0".repeat(64),
        prompt_hash: "0".repeat(64),
        model: "scripted".into(),
        split: Split::All,
        question_ids_hash: "0".repeat(64),
        planned: 0,
        n: 0,
        tuned_on: false,
        contaminated: None,
        few_shot_used: 0,
        oracle_knowledge: false,
        ledger_fingerprints: Default::default(),
        budget_stopped: None,
        started_at: "2026-11-03T00:00:00Z".into(),
        finished_at: "2026-11-03T00:00:00Z".into(),
        metrics: Metrics {
            ex: zero,
            soft_f1: zero,
            rves: 0.0,
            grounding_rate: 0.0,
            executed_rate: 0.0,
            schema_recall: None,
            join_recall: None,
            value_hit_rate: None,
            by_difficulty: Default::default(),
            by_database: Default::default(),
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
            cost_known: false,
            latency_mean_ms: 0.0,
            latency_p95_ms: 0,
            categories: Default::default(),
            truncated: 0,
            calls: 0,
            cached_calls: 0,
            billed_input_tokens: 0,
            billed_output_tokens: 0,
            turns: 0,
            open_findings: Default::default(),
        },
        questions: Vec::new(),
    }
}
