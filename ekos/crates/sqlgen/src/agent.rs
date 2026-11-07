//! A tool loop over `LlmProvider::complete` (RFC 0242 §2.2).
//!
//! `LlmProvider` has no native tool use, so the protocol is text: the model replies with one or
//! more ```` ```tool ```` blocks holding `{"tool": …, "args": {…}}`, or with the final SQL in a
//! ```` ```sql ```` block. Every submission is checked (lint + probes); findings the model has
//! not yet seen for that query go back while turns remain, with the context each finding names.
//!
//! With `tools: false` the same loop is RFC 0242's detector loop (cell J): no tools offered,
//! only checks and their context after each answer.

use crate::extract_sql;
use crate::lint::{Fetch, Finding, Severity, lint};
use crate::probe::{PREVIEW_ROWS, Preview, Probe, probe};
use crate::schema::Schema;
use ekos_recovery::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse, Message};
use ekos_sql_dialect_sdk::DialectFacts;
use serde::{Deserialize, Serialize};
use sqlparser::dialect::Dialect;
use std::collections::BTreeSet;

/// Longest tool output put into the conversation.
const MAX_TOOL_CHARS: usize = 3000;
/// Most tool calls run from one reply.
const MAX_TOOLS_PER_TURN: usize = 4;

/// What the checks and tools work over.
pub struct Toolbox<'a> {
    pub schema: &'a Schema,
    pub dialect: &'a dyn Dialect,
    pub facts: DialectFacts,
    /// The database, when one may be queried (read-only).
    pub probe: Option<&'a dyn Probe>,
    /// Finding codes in use; `None` means all.
    pub checks: Option<&'a BTreeSet<String>>,
}

impl Toolbox<'_> {
    /// Lint and probe findings for `sql`, filtered to the codes in use, and the result preview.
    pub fn check(&self, sql: &str) -> (Option<Preview>, Vec<Finding>) {
        let (analysis, mut findings) = match lint(sql, self.dialect, &self.facts, self.schema) {
            Ok((a, f)) => (Some(a), f),
            Err((_, f)) => (None, f),
        };
        let mut preview = None;
        if let Some(db) = self.probe {
            let (p, f) = probe(sql, analysis.as_ref(), self.schema, db);
            // The engine ran it: a parse failure was the parser's gap, not the query's.
            if p.is_some() {
                findings.retain(|f| f.code != "E001");
            }
            preview = p;
            findings.extend(f);
        }
        if let Some(enabled) = self.checks {
            findings.retain(|f| enabled.contains(&f.code));
        }
        (preview, findings)
    }

    /// Runs one tool; the text is what the model is shown.
    pub fn run_tool(&self, call: &ToolCall) -> String {
        let arg = |k: &str| {
            call.args
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let lines = |v: Vec<String>, none: &str| {
            if v.is_empty() {
                none.to_string()
            } else {
                v.join("\n")
            }
        };
        let text = match call.tool.as_str() {
            "search_schema" => lines(self.schema.search(&arg("text")), "nothing matches"),
            "describe_table" => self
                .schema
                .describe(&arg("name"))
                .unwrap_or_else(|| format!("no table named {}", arg("name"))),
            "join_path" => match self.schema.join_path(&arg("from"), &arg("to")) {
                Some(p) if p.is_empty() => "same table".into(),
                Some(p) => p.join("\n"),
                None => "no foreign-key path between them".into(),
            },
            "find_values" => {
                let column = call.args.get("column").and_then(|v| v.as_str());
                lines(
                    self.schema.find_values(&arg("text"), column),
                    "no stored value contains that text",
                )
            }
            "run_sql" => match self.probe {
                None => "no database is available to run queries".into(),
                Some(db) => match db.preview(&arg("sql"), PREVIEW_ROWS) {
                    Ok(p) => p.render(),
                    Err(e) => format!("error: {e}"),
                },
            },
            // RFC 0246: a metric a semantic model defines.
            "metric" => self.schema.describe_metric(&arg("name")),
            "check_sql" => {
                let (_, f) = self.check(&arg("sql"));
                if f.is_empty() {
                    "no problems found".into()
                } else {
                    render_findings(&f)
                }
            }
            other => format!("unknown tool {other}; the tools are {}", TOOLS.join(", ")),
        };
        cap(&text)
    }

    /// The context each finding's `fetch` names, deduplicated.
    pub fn context_for(&self, findings: &[Finding]) -> String {
        let mut seen = BTreeSet::new();
        let mut out = String::new();
        for f in findings {
            let Some(fetch) = &f.fetch else { continue };
            if !seen.insert(fetch.clone()) {
                continue;
            }
            let (title, call) = match fetch {
                Fetch::DescribeTable { table } => (
                    format!("Table {table}:"),
                    ToolCall::new("describe_table", serde_json::json!({ "name": table })),
                ),
                Fetch::FindValues { text, column } => (
                    format!("Stored values containing '{text}':"),
                    ToolCall::new(
                        "find_values",
                        serde_json::json!({ "text": text, "column": column }),
                    ),
                ),
                Fetch::JoinPath { from, to } => (
                    format!("Foreign keys from {from} to {to}:"),
                    ToolCall::new("join_path", serde_json::json!({ "from": from, "to": to })),
                ),
            };
            out.push_str(&format!("{title}\n{}\n\n", self.run_tool(&call)));
        }
        out
    }
}

pub const TOOLS: &[&str] = &[
    "search_schema",
    "describe_table",
    "join_path",
    "find_values",
    "run_sql",
    "check_sql",
    "metric",
];

/// One tool call as the model wrote it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    #[serde(default)]
    pub args: serde_json::Map<String, serde_json::Value>,
}

impl ToolCall {
    fn new(tool: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool: tool.into(),
            args: args.as_object().cloned().unwrap_or_default(),
        }
    }
}

/// The ```tool blocks of a reply, in order; malformed ones are returned as errors.
pub fn tool_calls(reply: &str) -> Vec<Result<ToolCall, String>> {
    reply
        .split("```")
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .filter_map(|(_, b)| b.strip_prefix("tool"))
        .map(|body| serde_json::from_str::<ToolCall>(body.trim()).map_err(|e| format!("{e}")))
        .collect()
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Model replies in total (the first answer included).
    pub max_turns: usize,
    pub max_tokens: u32,
    pub prompt_version: &'static str,
    /// Offer the tools (the agent); `false` is the detector loop.
    pub tools: bool,
    /// Send the first clean submission back once with its result, asking whether it answers
    /// exactly what was asked (RFC 0242 cell K2); with tools, also ask for literals to be checked.
    pub verify: bool,
}

/// One model call.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CallStat {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The LLM cache answered it (nothing billed).
    pub cached: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentOutcome {
    /// The last SQL the model submitted.
    pub sql: Option<String>,
    pub turns: usize,
    /// Tool names in the order called.
    pub tools: Vec<String>,
    /// Every finding code raised on a submission, in order.
    pub findings: Vec<String>,
    /// Codes still open on the final SQL.
    pub open_findings: Vec<String>,
    pub calls: Vec<CallStat>,
    /// No SQL at the end, and the last reply used the whole output allowance.
    pub truncated: bool,
}

/// The tool protocol, appended to the task prompt when tools are on.
pub fn protocol(max_turns: usize, verify: bool) -> String {
    protocol_with(max_turns, verify, false)
}

/// [`protocol`], also offering the `metric` tool when the schema has metrics a semantic model
/// defines (RFC 0246). Without metrics the text is exactly [`protocol`]'s, so benchmark prompts
/// do not change.
pub fn protocol_with(max_turns: usize, verify: bool, metrics: bool) -> String {
    let metric_line = if metrics {
        "- metric {\"name\"}: how a metric the semantic model defines is computed (by name or \
         synonym); use that definition when the question asks for it\n"
    } else {
        ""
    };
    let text = format!(
        "\nTools:\nBefore answering you may look things up. To use tools, reply with one or more \
         ```tool blocks, each holding one JSON object, and nothing else. For example:\n\
         ```tool\n{{\"tool\": \"find_values\", \"args\": {{\"text\": \"Germany\"}}}}\n```\n\
         - search_schema {{\"text\"}}: tables and columns whose names, comments or example values \
         mention the words\n\
         - describe_table {{\"name\"}}: every column of a table with its type, keys, counts and \
         examples\n\
         - join_path {{\"from\", \"to\"}}: the foreign-key joins that connect two tables\n\
         - find_values {{\"text\", \"column\" (optional, table.column)}}: stored values containing \
         the text, and the columns that hold them\n\
         - run_sql {{\"sql\"}}: the first {PREVIEW_ROWS} rows of a read-only query\n\
         - check_sql {{\"sql\"}}: likely problems in a query (unknown names, value spellings, \
         joins that repeat rows, integer division, NULL ordering, ties)\n\
         {metric_line}\
         When you are sure, give the final SQL in one ```sql block; it is checked before it is \
         accepted. You have at most {max_turns} replies in total.\n"
    );
    if verify {
        format!(
            "{text}Check every value you compare with a column (names, codes, places, dates) with \
             find_values first: the data may spell or format it differently.\n"
        )
    } else {
        text
    }
}

/// Runs the loop. `prompt` is the full task prompt (schema, question, instructions); with tools
/// on, [`protocol`] is appended. Provider errors (a spent budget among them) end the loop.
pub async fn run(
    llm: &dyn LlmProvider,
    prompt: &str,
    tb: &Toolbox<'_>,
    cfg: &AgentConfig,
) -> Result<AgentOutcome, LlmError> {
    let mut s = Session {
        tb,
        cfg,
        turns: Vec::new(),
        out: AgentOutcome::default(),
        shown: BTreeSet::new(),
        verified: false,
    };
    let mut user = if cfg.tools {
        let metrics = !tb.schema.metrics.is_empty();
        format!(
            "{prompt}{}",
            protocol_with(cfg.max_turns, cfg.verify, metrics)
        )
    } else {
        prompt.to_string()
    };
    let max_turns = cfg.max_turns.max(1);
    for turn in 0..max_turns {
        let last = turn + 1 == max_turns;
        if last && turn > 0 {
            user.push_str(
                "\n\nThis is your last reply: answer with the final SQL in one ```sql block.",
            );
        }
        let resp = s.ask(llm, &user).await?;
        let next = s.answer(&resp, last, max_turns - turn - 1);
        s.turns.push((std::mem::take(&mut user), resp.content));
        match next {
            Some(text) => user = text,
            None => break,
        }
    }
    if s.out.sql.is_some() {
        s.out.truncated = false;
    }
    Ok(s.out)
}

/// One conversation: the turns so far and what has been shown.
struct Session<'a, 'b> {
    tb: &'a Toolbox<'b>,
    cfg: &'a AgentConfig,
    /// `(user, assistant)` pairs, oldest first.
    turns: Vec<(String, String)>,
    out: AgentOutcome,
    /// `(code, message)` of findings already sent back.
    shown: BTreeSet<(String, String)>,
    /// The result has been shown once (findings feedback or the verify step).
    verified: bool,
}

impl Session<'_, '_> {
    /// One model call with the history so far, counted on the outcome.
    async fn ask(&mut self, llm: &dyn LlmProvider, user: &str) -> Result<LlmResponse, LlmError> {
        let history: Vec<Message> = self
            .turns
            .iter()
            .flat_map(|(u, a)| {
                [
                    Message {
                        role: "user",
                        content: u.as_str(),
                    },
                    Message {
                        role: "assistant",
                        content: a.as_str(),
                    },
                ]
            })
            .collect();
        let req = LlmRequest {
            system: "",
            user,
            prompt_version: self.cfg.prompt_version,
            max_tokens: self.cfg.max_tokens,
            history: &history,
        };
        let hits_before = llm.cache_stats().map(|(h, _)| h);
        let resp = llm.complete(&req).await?;
        let hits_after = llm.cache_stats().map(|(h, _)| h);
        self.out.turns += 1;
        self.out.calls.push(CallStat {
            model: resp.model.clone(),
            input_tokens: resp.input_tokens as u64,
            output_tokens: resp.output_tokens as u64,
            cached: matches!((hits_before, hits_after), (Some(b), Some(a)) if a > b),
        });
        Ok(resp)
    }

    /// What to send next, or `None` when the conversation is over.
    fn answer(&mut self, resp: &LlmResponse, last: bool, left: usize) -> Option<String> {
        let calls = if self.cfg.tools {
            tool_calls(&resp.content)
        } else {
            Vec::new()
        };
        if !calls.is_empty() && !last {
            return Some(self.use_tools(&calls, left));
        }
        if let Some(sql) = extract_sql(&resp.content) {
            return self.submit(sql, last);
        }
        self.out.truncated = resp.output_tokens >= self.cfg.max_tokens;
        if last {
            None
        } else if self.cfg.tools {
            Some(
                "Reply with ```tool blocks to look things up, or with the final SQL in one ```sql \
                 block."
                    .into(),
            )
        } else {
            Some(
                "The answer held no SQL code block. Answer with the SQL in one ```sql code block."
                    .into(),
            )
        }
    }

    fn use_tools(&mut self, calls: &[Result<ToolCall, String>], left: usize) -> String {
        let mut text = String::from("Tool results:\n");
        for c in calls.iter().take(MAX_TOOLS_PER_TURN) {
            match c {
                Ok(call) => {
                    self.out.tools.push(call.tool.clone());
                    text.push_str(&format!(
                        "### {} {}\n{}\n\n",
                        call.tool,
                        serde_json::Value::Object(call.args.clone()),
                        self.tb.run_tool(call)
                    ));
                }
                Err(e) => text.push_str(&format!("### malformed tool block: {e}\n\n")),
            }
        }
        text.push_str(&format!("You have {left} replies left."));
        text
    }

    /// Checks a submission: `None` accepts it; otherwise the findings go back. An error always
    /// goes back; a warning only the first time it is seen for this query.
    fn submit(&mut self, sql: String, last: bool) -> Option<String> {
        let (preview, findings) = self.tb.check(&sql);
        self.out
            .findings
            .extend(findings.iter().map(|f| f.code.clone()));
        self.out.open_findings = findings.iter().map(|f| f.code.clone()).collect();
        self.out.sql = Some(sql);
        let fresh = findings.iter().any(|f| {
            f.severity == Severity::Error
                || !self.shown.contains(&(f.code.clone(), f.message.clone()))
        });
        if last {
            return None;
        }
        if !fresh {
            if !self.cfg.verify || self.verified {
                return None;
            }
            self.verified = true;
            return preview.map(|p| verify_message(&p));
        }
        self.verified = true;
        for f in &findings {
            self.shown.insert((f.code.clone(), f.message.clone()));
        }
        Some(feedback(
            &findings,
            preview.as_ref(),
            self.tb,
            self.cfg.tools,
        ))
    }
}

/// The verify step: the result, and the question to check it against.
fn verify_message(p: &Preview) -> String {
    let mut head = p.clone();
    head.truncated |= head.rows.len() > 10;
    head.rows.truncate(10);
    format!(
        "That SQL runs. Its result begins:\n{}\n\nCheck it against the question before it is \
         final: exactly the columns asked for, in that order; one row per thing the question \
         asks about (no duplicates from joins, no missing rows); values in the form asked \
         (units, percentages, rounding). If it answers the question as asked, repeat the SQL \
         unchanged; otherwise correct it. Answer with the SQL in one ```sql block.",
        head.render()
    )
}

/// What a submission with findings is answered with.
fn feedback(
    findings: &[Finding],
    preview: Option<&Preview>,
    tb: &Toolbox<'_>,
    tools: bool,
) -> String {
    let mut text = format!("Checking that SQL found:\n{}\n", render_findings(findings));
    if let Some(p) = preview {
        let mut head = p.clone();
        head.truncated |= head.rows.len() > 5;
        head.rows.truncate(5);
        text.push_str(&format!("\nIts result begins:\n{}\n", head.render()));
    }
    let context = tb.context_for(findings);
    if !context.is_empty() {
        text.push_str(&format!("\n{context}"));
    }
    text.push_str(
        "\nCorrect the query, or, if it already answers the question as asked, repeat it \
         unchanged. Answer with the SQL in one ```sql block.",
    );
    if tools {
        text.push_str(" You may use tools first.");
    }
    text
}

pub fn render_findings(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| format!("- {}: {}", f.code, f.message))
        .collect::<Vec<_>>()
        .join("\n")
}

fn cap(s: &str) -> String {
    if s.len() <= MAX_TOOL_CHARS {
        return s.to_string();
    }
    let mut end = MAX_TOOL_CHARS;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… (cut)", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::tests::Mem;
    use crate::schema::tests::fixture;
    use async_trait::async_trait;
    use sqlparser::dialect::SQLiteDialect;
    use std::sync::Mutex;

    /// Replies from a script, one per call, and records what it was sent.
    struct Script {
        replies: Mutex<Vec<String>>,
        seen: Mutex<Vec<String>>,
    }

    impl Script {
        fn new(replies: &[&str]) -> Script {
            Script {
                replies: Mutex::new(replies.iter().rev().map(|s| s.to_string()).collect()),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for Script {
        fn model_name(&self) -> &str {
            "script"
        }
        async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
            self.seen.lock().unwrap().push(req.user.to_string());
            let content = self.replies.lock().unwrap().pop().unwrap_or_default();
            Ok(LlmResponse {
                content,
                model: "script".into(),
                input_tokens: 10,
                output_tokens: 5,
                ..Default::default()
            })
        }
    }

    fn schema() -> Schema {
        let mut s = fixture();
        s.tables[0].columns.push(crate::Column {
            name: "colour".into(),
            data_type: "TEXT".into(),
            ..Default::default()
        });
        s
    }

    fn cfg(tools: bool, max_turns: usize) -> AgentConfig {
        AgentConfig {
            max_turns,
            max_tokens: 100,
            prompt_version: "test",
            tools,
            verify: false,
        }
    }

    #[tokio::test]
    async fn tools_run_then_a_clean_submission_is_accepted() {
        let s = schema();
        let db = Mem::new();
        let tb = Toolbox {
            schema: &s,
            dialect: &SQLiteDialect {},
            facts: DialectFacts::default(),
            probe: Some(&db),
            checks: None,
        };
        let llm = Script::new(&[
            "```tool\n{\"tool\":\"describe_table\",\"args\":{\"name\":\"cards\"}}\n```\n\
             ```tool\n{\"tool\":\"run_sql\",\"args\":{\"sql\":\"SELECT COUNT(*) FROM cards\"}}\n```",
            "```sql\nSELECT name FROM cards WHERE colour = 'Germany'\n```",
        ]);
        let o = run(&llm, "Q", &tb, &cfg(true, 6)).await.unwrap();
        assert_eq!(o.tools, vec!["describe_table", "run_sql"]);
        assert_eq!(
            o.sql.as_deref(),
            Some("SELECT name FROM cards WHERE colour = 'Germany'")
        );
        assert_eq!(o.turns, 2);
        let seen = llm.seen.lock().unwrap();
        assert!(seen[0].contains("Tools:"));
        assert!(seen[1].contains("table cards") && seen[1].contains("(1 row)"));
    }

    #[tokio::test]
    async fn findings_go_back_once_then_a_repeat_is_accepted() {
        let s = schema();
        let db = Mem::new();
        let tb = Toolbox {
            schema: &s,
            dialect: &SQLiteDialect {},
            facts: DialectFacts::default(),
            probe: Some(&db),
            checks: None,
        };
        let wrong = "```sql\nSELECT id FROM cards WHERE colour = 'germany'\n```";
        let llm = Script::new(&[wrong, wrong]);
        let o = run(&llm, "Q", &tb, &cfg(false, 6)).await.unwrap();
        assert_eq!(
            o.turns, 2,
            "a warning is shown once; the model may keep its answer"
        );
        assert_eq!(o.findings, vec!["P001", "P003", "P001", "P003"]);
        assert_eq!(o.open_findings, vec!["P001", "P003"]);
        let seen = llm.seen.lock().unwrap();
        assert!(seen[1].contains("P003") && seen[1].contains("Its result begins"));
        assert!(seen[1].contains("Stored values containing 'germany'"));
    }

    #[tokio::test]
    async fn errors_repeat_until_the_turn_cap_and_disabled_codes_are_ignored() {
        let s = schema();
        let db = Mem::new();
        let only: BTreeSet<String> = ["E000".to_string()].into();
        let tb = Toolbox {
            schema: &s,
            dialect: &SQLiteDialect {},
            facts: DialectFacts::default(),
            probe: Some(&db),
            checks: Some(&only),
        };
        let bad = "```sql\nSELECT nope FROM cards\n```";
        let llm = Script::new(&[bad, bad, bad]);
        let o = run(&llm, "Q", &tb, &cfg(false, 3)).await.unwrap();
        assert_eq!(o.turns, 3);
        assert_eq!(o.open_findings, vec!["E000"], "L001 is not in use");
        assert!(llm.seen.lock().unwrap()[2].contains("last reply"));
    }

    #[tokio::test]
    async fn a_reply_without_sql_or_tools_gets_the_reminder() {
        let s = schema();
        let tb = Toolbox {
            schema: &s,
            dialect: &SQLiteDialect {},
            facts: DialectFacts::default(),
            probe: None,
            checks: None,
        };
        let llm = Script::new(&["I am not sure.", "```sql\nSELECT 1\n```"]);
        let o = run(&llm, "Q", &tb, &cfg(true, 4)).await.unwrap();
        assert_eq!(o.sql.as_deref(), Some("SELECT 1"));
        assert!(llm.seen.lock().unwrap()[1].contains("Reply with ```tool blocks"));
    }

    #[tokio::test]
    async fn verify_shows_a_clean_result_once() {
        let s = schema();
        let db = Mem::new();
        let tb = Toolbox {
            schema: &s,
            dialect: &SQLiteDialect {},
            facts: DialectFacts::default(),
            probe: Some(&db),
            checks: None,
        };
        let ok = "```sql\nSELECT name FROM cards WHERE colour = 'Germany'\n```";
        let llm = Script::new(&[ok, ok]);
        let mut c = cfg(true, 6);
        c.verify = true;
        let o = run(&llm, "Q", &tb, &c).await.unwrap();
        assert_eq!(o.turns, 2);
        let seen = llm.seen.lock().unwrap();
        assert!(seen[0].contains("with find_values first"));
        assert!(seen[1].contains("That SQL runs") && seen[1].contains("Goyf"));
        assert!(!protocol(6, false).contains("find_values first"));
    }

    #[test]
    fn rfc0246_the_metric_tool_is_offered_only_with_metrics() {
        assert_eq!(protocol(5, true), protocol_with(5, true, false));
        assert!(!protocol(5, false).contains("- metric"));
        assert!(protocol_with(5, false, true).contains("- metric {\"name\"}"));
    }

    #[test]
    fn tool_blocks_parse_and_malformed_ones_are_reported() {
        let calls = tool_calls(
            "x ```tool\n{\"tool\":\"join_path\",\"args\":{\"from\":\"a\"}}\n``` \
                                ```tool\nnot json\n``` ```sql\nSELECT 1\n```",
        );
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].as_ref().unwrap().tool, "join_path");
        assert!(calls[1].is_err());
        assert_eq!(extract_sql("```tool\n{}\n```"), None);
    }
}
