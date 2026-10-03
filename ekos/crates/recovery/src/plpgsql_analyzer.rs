//! `PlPgSqlAnalyzerPass` — RFC 0163's "Where it runs": every routine in a PostgreSQL schema file,
//! parsed by `ekos-plpgsql` into a procedural IR and written to the ledger.
//!
//! Deterministic and side-effect-free, no LLM. Emits, per `.sql` file:
//!
//! - one `Custom("Procedure")` per `CREATE [OR REPLACE] FUNCTION|PROCEDURE`, carrying its signature
//!   and its **fidelity label** — computed by the parser from the IR, never asserted here — with the
//!   exact recovered/unrecovered statement counts, so a report can say "41 of 44 statements" rather
//!   than "recovered";
//! - one `Custom("ProcedureStatement")` per statement at every depth, with evidence that is the
//!   statement's own source text and line;
//! - `Contains` edges forming the tree (procedure → top-level statements, statement → nested
//!   statements), each child recording the branch it sits in (`then:0`, `else`, `handler:1`, …) so
//!   the tree keeps the routine's control flow, not just its membership.
//!
//! Routines in other languages (`LANGUAGE sql`, `c`, …) get a `Procedure` at `Signature` fidelity
//! and no statements: an empty body would read as "nothing in it", which is false.
//!
//! **Not here:** edges from a routine to the tables and routines it touches. Those need the embedded
//! SQL lowered into the Transformation IR (RFC 0164) and a whole-graph name match the way RFC 0075
//! links `TransformNode`s to tables; a per-file pass sees neither. The statement text needed for
//! both is already on every `ProcedureStatement`.

use crate::plpgsql_footprint::{Footprint, expression_footprint, statement_footprint};
use crate::sql_comments::{
    ObjectComment, ObjectCommentKind, extract_object_comments, match_object_comments,
};
use crate::sql_objects::{clip, file_kir_id};
use crate::sql_predicates::predicates_json;
use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_plpgsql::{Fidelity, ProcStmt, ProcedureIr, Span, line_of, parse_function, routines};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Bumped whenever this pass's output changes for the same input, so a cached run is not reused.
const LOGIC_VERSION: &str = "plpgsql-analyzer/3";

/// The most source text one statement's evidence carries. A statement longer than this is rare;
/// its exact byte span is always recorded, so the full text stays recoverable from the file.
const MAX_FRAGMENT: usize = 4096;

/// Per-file counters, for `ekos recover`'s summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlPgSqlStats {
    /// Routine definitions found, in any language.
    pub routines: usize,
    /// Of those, PL/pgSQL.
    pub plpgsql: usize,
    /// PL/pgSQL routines at `Statements` fidelity.
    pub complete: usize,
    /// PL/pgSQL routines at `Partial` fidelity.
    pub partial: usize,
    /// `ProcedureStatement` objects written.
    pub statements: usize,
    /// Of those, `Unrecovered`.
    pub unrecovered: usize,
    /// Definitions replaced by a later `CREATE OR REPLACE` of the same routine in the same file.
    pub redefined: usize,
    /// Set when the file could not be lexed at all; nothing was recovered from it.
    pub lex_error: Option<String>,
}

pub struct PlPgSqlAnalyzerPass {
    pass_id: String,
    source_path: String,
    sql: String,
    /// The owning `File` object's id key (project-qualified, observe-path-relative — RFC 0079),
    /// so each routine hangs off its file with a `Contains` edge.
    file_key: Option<String>,
    stats: Arc<Mutex<PlPgSqlStats>>,
}

impl PlPgSqlAnalyzerPass {
    pub fn new(source_path: impl Into<String>, sql: impl Into<String>) -> Self {
        let source_path = source_path.into();
        Self {
            pass_id: format!("plpgsql-analyzer:{source_path}"),
            sql: sql.into(),
            source_path,
            file_key: None,
            stats: Arc::new(Mutex::new(PlPgSqlStats::default())),
        }
    }

    /// Attach every routine to its `File` object (`Contains`), whose id is the v5 UUID of
    /// `file_key` — the same scheme `build` and every language analyzer use.
    pub fn with_file(mut self, file_key: impl Into<String>) -> Self {
        self.file_key = Some(file_key.into());
        self
    }

    /// Handle onto this pass's counters, readable after the `PassManager` has taken the pass.
    pub fn stats_handle(&self) -> Arc<Mutex<PlPgSqlStats>> {
        Arc::clone(&self.stats)
    }

    /// Whether a file is worth handing to this pass.
    ///
    /// Only files resolved to the `postgres` dialect, or that name `plpgsql` themselves. A T-SQL or
    /// MySQL procedure has no `LANGUAGE` clause, and "parsing" it as PL/pgSQL would produce a
    /// confident-looking `Partial` routine full of gaps that are really a different language.
    pub fn applies_to(dialect_name: &str, sql: &str) -> bool {
        matches!(dialect_name, "postgres" | "postgresql")
            || sql.to_ascii_lowercase().contains("plpgsql")
    }
}

#[async_trait]
impl CompilerPass for PlPgSqlAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(LOGIC_VERSION.as_bytes());
        hasher.update(self.source_path.as_bytes());
        hasher.update(self.file_key.as_deref().unwrap_or_default().as_bytes());
        hasher.update(self.sql.as_bytes());
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let (graph, stats) =
            recover_routines_in(&self.source_path, &self.sql, self.file_key.as_deref());
        *self.stats.lock().unwrap() = stats.clone();

        if let Some(e) = &stats.lex_error {
            tracing::warn!(pass = %self.pass_id, "plpgsql-analyzer: file does not lex: {e}");
        }
        if graph.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            routines = stats.routines,
            plpgsql = stats.plpgsql,
            complete = stats.complete,
            partial = stats.partial,
            statements = stats.statements,
            unrecovered = stats.unrecovered,
            "plpgsql-analyzer complete"
        );
        Ok(())
    }
}

fn kir_id(seed: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()))
}

/// A routine's structural key: its file, name and argument list. Overloads share a name and differ
/// in arguments, and are different routines.
fn procedure_key(source_path: &str, ir: &ProcedureIr) -> String {
    format!(
        "{source_path}:{}({})",
        ir.signature.name.to_ascii_lowercase(),
        ir.signature.arguments.join(", ")
    )
}

/// Recover every routine in one SQL file into KIR. Pure: same input, same graph (ids included).
pub fn recover_routines(source_path: &str, sql: &str) -> (KirGraph, PlPgSqlStats) {
    recover_routines_in(source_path, sql, None)
}

/// [`recover_routines`], with each routine attached to the `File` whose id key is `file_key`.
pub fn recover_routines_in(
    source_path: &str,
    sql: &str,
    file_key: Option<&str>,
) -> (KirGraph, PlPgSqlStats) {
    let mut stats = PlPgSqlStats::default();
    let mut graph = KirGraph::new();

    let found = match routines(sql) {
        Ok(r) => r,
        Err(e) => {
            stats.lex_error = Some(e.to_string());
            return (graph, stats);
        }
    };

    // `CREATE OR REPLACE` of the same routine later in the same file replaces it, exactly as it
    // does in the database. Keyed and ordered, so output order is the file's, deterministically.
    let mut by_key: BTreeMap<String, Parsed> = BTreeMap::new();
    for (position, routine) in found.iter().enumerate() {
        let Ok(ir) = parse_function(routine.text) else {
            continue;
        };
        stats.routines += 1;
        let parsed = Parsed {
            position,
            ir,
            offset: routine.offset,
            text: routine.text,
        };
        if by_key
            .insert(procedure_key(source_path, &parsed.ir), parsed)
            .is_some()
        {
            stats.redefined += 1;
            stats.routines -= 1;
        }
    }
    let mut ordered: Vec<(String, Parsed)> = by_key.into_iter().collect();
    ordered.sort_by_key(|(_, p)| p.position);

    // `COMMENT ON FUNCTION|PROCEDURE` in the same file: the author's own description.
    let comments = extract_object_comments(sql);
    let candidates: Vec<(String, usize)> = ordered
        .iter()
        .map(|(_, p)| (p.ir.signature.name.clone(), p.ir.signature.arguments.len()))
        .collect();
    let docs = match_object_comments(
        &comments,
        &[ObjectCommentKind::Function, ObjectCommentKind::Procedure],
        &candidates,
    );
    let file = FileCtx {
        source_path,
        sql,
        file_id: file_key.map(file_kir_id),
    };
    for (i, (key, parsed)) in ordered.iter().enumerate() {
        emit_procedure(
            &mut graph,
            &mut stats,
            &file,
            key,
            parsed,
            docs.get(&i).copied(),
        );
    }
    (graph, stats)
}

/// One routine definition as parsed, with where it sits in its file.
struct Parsed<'a> {
    /// Its position among the file's routine definitions, for file-order output.
    position: usize,
    ir: ProcedureIr,
    offset: usize,
    text: &'a str,
}

/// The file a routine is recovered from.
struct FileCtx<'a> {
    source_path: &'a str,
    sql: &'a str,
    /// The owning `File` object, when the caller knows its key.
    file_id: Option<KirId>,
}

fn emit_procedure(
    graph: &mut KirGraph,
    stats: &mut PlPgSqlStats,
    file: &FileCtx,
    key: &str,
    parsed: &Parsed,
    doc: Option<&ObjectComment>,
) {
    let (source_path, sql) = (file.source_path, file.sql);
    let Parsed {
        ir, offset, text, ..
    } = parsed;
    let (offset, text) = (*offset, *text);
    let proc_id = kir_id(&format!("plpgsql:procedure:{key}"));
    let line = line_of(sql, offset);

    let mut recovered = 0usize;
    let mut unrecovered = 0usize;
    for s in &ir.body {
        s.walk(&mut |x| {
            if matches!(x, ProcStmt::Unrecovered { .. }) {
                unrecovered += 1;
            } else {
                recovered += 1;
            }
        });
    }
    let fidelity = match ir.fidelity() {
        Fidelity::Signature => "signature",
        Fidelity::Partial { .. } => "partial",
        Fidelity::Statements => "statements",
    };
    let is_plpgsql = ir.fidelity() != Fidelity::Signature;
    if ir.signature.language == "plpgsql" {
        stats.plpgsql += 1;
        match ir.fidelity() {
            Fidelity::Statements => stats.complete += 1,
            Fidelity::Partial { .. } => stats.partial += 1,
            Fidelity::Signature => {}
        }
    }

    // The header — everything before the body — is what establishes the routine's existence and
    // signature, and is short; the body's own statements carry their own evidence.
    let header_end = text
        .find("$")
        .or_else(|| text.find('\''))
        .unwrap_or(text.len());
    let mut evidence = KirEvidence::new(
        SourceLocation {
            path: source_path.to_string(),
            line: Some(line),
            column: None,
        },
        clip(text[..header_end].trim_end(), MAX_FRAGMENT),
    );
    evidence.id = kir_id(&format!("plpgsql:procedure-evidence:{key}"));
    let evidence_id = graph.add_evidence(evidence);

    let gaps: Vec<u32> = ir
        .gaps()
        .iter()
        .map(|sp| line_of(sql, offset + sp.start))
        .collect();
    let mut obj = KirObject::new(
        ir.signature.name.clone(),
        ObjectKind::Custom("Procedure".into()),
    )
    .with_property("language", json!(ir.signature.language))
    .with_property("arguments", json!(ir.signature.arguments))
    .with_property("returns", json!(ir.signature.returns))
    .with_property("fidelity", json!(fidelity))
    .with_property("statements_recovered", json!(recovered))
    .with_property("statements_unrecovered", json!(unrecovered))
    .with_property("unrecovered_lines", json!(gaps))
    .with_property("dynamic_sql_sites", json!(ir.dynamic_sites().len()))
    .with_property(
        "eligible_for_reconstruction",
        json!(ir.eligible_for_reconstruction()),
    )
    .with_property(
        "declarations",
        json!(
            ir.declarations
                .iter()
                .map(|d| json!({"name": d.name, "type": d.data_type}))
                .collect::<Vec<_>>()
        ),
    )
    .with_property("source_path", json!(source_path))
    .with_property("line", json!(line))
    .with_property("span_start", json!(offset))
    .with_property("span_end", json!(offset + text.len()))
    // RFC 0088's symbol convention, so `llm_description` can read the routine's source.
    .with_property(
        "source_span",
        json!({"start_line": line, "end_line": line_of(sql, offset + text.len())}),
    );
    obj.id = proc_id;
    obj.evidence.push(evidence_id);

    if let Some(c) = doc {
        // The author's description outranks anything generated (RFC 0146 Phase 2).
        let mut ev = KirEvidence::new(
            SourceLocation {
                path: source_path.to_string(),
                line: Some(c.line),
                column: None,
            },
            clip(
                &format!("COMMENT ON FUNCTION {} IS {}", c.name, c.text),
                MAX_FRAGMENT,
            ),
        );
        ev.id = kir_id(&format!("plpgsql:procedure-comment:{key}"));
        obj.evidence.push(graph.add_evidence(ev));
        obj.properties.insert("description".into(), json!(c.text));
    }
    if let Some(file_id) = file.file_id {
        graph.add_relationship(KirRelationship::deterministic(
            RelationshipKind::Contains,
            file_id,
            proc_id,
            "",
        ));
    }

    // What the routine touches: the union of its statements' footprints and its declarations'
    // defaults — or, for a `LANGUAGE sql` routine, its body parsed as the SQL it is.
    let locals = routine_locals(ir);
    let mut footprint = Footprint::default();
    for d in &ir.declarations {
        if let Some(default) = &d.default {
            footprint.merge(expression_footprint(default));
        }
    }
    if is_plpgsql {
        let mut emitter = StatementEmitter {
            graph,
            stats,
            source_path,
            sql,
            key,
            procedure: &ir.signature.name,
            offset,
            next_index: 0,
            footprint: &mut footprint,
            locals: &locals,
        };
        for (order, stmt) in ir.body.iter().enumerate() {
            emitter.emit(stmt, proc_id, None, 0, order, "body");
        }
    } else if ir.signature.language == "sql"
        && let Some(body) = sql_body(text)
    {
        let fp = statement_footprint(&body);
        // RFC 0170: a `LANGUAGE sql` routine has no statements, so it carries its own predicates.
        let predicates = business_predicates(&fp.predicates, &locals);
        if !predicates.is_empty() {
            let base = text
                .find(body.as_str())
                .map_or(line, |at| line_of(sql, offset + at));
            obj.properties
                .insert("predicates".into(), predicates_json(&predicates, base));
        }
        footprint.merge(fp);
    }
    set_footprint(&mut obj.properties, &footprint);
    obj.properties
        .insert("footprint_fragments".into(), json!(footprint.fragments));
    if is_plpgsql
        && ir
            .signature
            .returns
            .as_deref()
            .is_some_and(|r| r.eq_ignore_ascii_case("trigger"))
    {
        let t = trigger_facts(ir);
        obj.properties
            .insert("assigns_new".into(), json!(t.assigns_new));
        obj.properties
            .insert("raises_exception".into(), json!(t.raises_exception));
        obj.properties
            .insert("returns_null".into(), json!(t.returns_null));
        // A placeholder: the whole body is `RETURN NEW|OLD`. Schemas create a trigger against one
        // and replace the function later (LedgerSMB: "dummy; actual function defined in …").
        // An empty body counts too (LedgerSMB 1.10: `BEGIN END;`). A *conditional* return is
        // not a placeholder: `RETURN NULL` in a BEFORE trigger drops rows, which is real logic.
        let pass_through = ir.declarations.is_empty()
            && match ir.body.as_slice() {
                [] => true,
                [
                    ProcStmt::Return {
                        value: Some(v),
                        query: None,
                        ..
                    },
                ] => v.trim().eq_ignore_ascii_case("new") || v.trim().eq_ignore_ascii_case("old"),
                _ => false,
            };
        obj.properties
            .insert("pass_through".into(), json!(pass_through));
    }
    graph.add_object(obj);
}

/// What a trigger function does to the row and around it — the structural facts RFC 0163's
/// trigger classification reads. Read from the IR, never from names.
struct TriggerFacts {
    /// `NEW` columns set, by assignment or by `… INTO new.col`.
    assigns_new: BTreeSet<String>,
    /// `RAISE EXCEPTION`s, a bare re-raising `RAISE` included.
    raises_exception: usize,
    /// Whether it ever returns `NULL` — in a `BEFORE` row trigger, that silently drops the row.
    returns_null: bool,
}

fn trigger_facts(ir: &ProcedureIr) -> TriggerFacts {
    let mut t = TriggerFacts {
        assigns_new: Default::default(),
        raises_exception: 0,
        returns_null: false,
    };
    let new_col = |target: &str, t: &mut TriggerFacts| {
        let target = target.trim().to_ascii_lowercase();
        if let Some(col) = target.strip_prefix("new.") {
            t.assigns_new.insert(col.to_string());
        }
    };
    for s in &ir.body {
        s.walk(&mut |x| match x {
            ProcStmt::Assign { target, .. } => new_col(target, &mut t),
            ProcStmt::Sql {
                into: Some(into), ..
            }
            | ProcStmt::Cursor {
                into: Some(into), ..
            } => {
                for target in into {
                    new_col(target, &mut t);
                }
            }
            ProcStmt::Raise { level, .. } if level == "exception" => t.raises_exception += 1,
            ProcStmt::Return { value: Some(v), .. } if v.trim().eq_ignore_ascii_case("null") => {
                t.returns_null = true
            }
            _ => {}
        });
    }
    t
}

/// A `LANGUAGE sql` routine's body: the dollar- or single-quoted string after `AS`, unescaped.
fn sql_body(text: &str) -> Option<String> {
    use ekos_plpgsql::lex::{Tok, lex};
    let toks = lex(text).ok()?;
    toks.windows(2).find_map(|w| match (&w[0].tok, &w[1].tok) {
        (Tok::Word(a), Tok::Dollar { body, .. } | Tok::Str(body))
            if a.eq_ignore_ascii_case("AS") =>
        {
            Some(body.clone())
        }
        _ => None,
    })
}

/// A routine's parameter and declared-variable names, lower-cased. Inside its SQL they parse as
/// column references (`status = in_status`, `in_from IS NULL`), but they are the routine's own
/// inputs, not business rules about a table.
fn routine_locals(ir: &ProcedureIr) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = ir
        .declarations
        .iter()
        .map(|d| d.name.to_ascii_lowercase())
        .collect();
    for arg in &ir.signature.arguments {
        let words: Vec<&str> = arg
            .split_whitespace()
            .skip_while(|w| {
                matches!(
                    w.to_ascii_uppercase().as_str(),
                    "IN" | "OUT" | "INOUT" | "VARIADIC"
                )
            })
            .collect();
        // `name type` names a parameter; a lone `type` does not.
        if words.len() >= 2 {
            names.insert(words[0].trim_matches('"').to_ascii_lowercase());
        }
    }
    names
}

/// RFC 0170: the predicates that test a real column — not a parameter or variable of the routine.
fn business_predicates(
    sites: &[crate::sql_predicates::PredicateSite],
    locals: &BTreeSet<String>,
) -> Vec<crate::sql_predicates::PredicateSite> {
    sites
        .iter()
        .filter(|s| !locals.contains(&s.column))
        .cloned()
        .collect()
}

fn set_footprint(props: &mut std::collections::HashMap<String, Value>, fp: &Footprint) {
    props.insert("reads".into(), json!(fp.reads));
    props.insert("writes".into(), json!(fp.writes));
    props.insert("inserts".into(), json!(fp.inserts));
    props.insert("updates".into(), json!(fp.updates));
    props.insert("deletes".into(), json!(fp.deletes));
    props.insert("calls".into(), json!(fp.calls));
    props.insert("footprint".into(), json!(fp.status()));
    if !fp.errors.is_empty() {
        props.insert("footprint_errors".into(), json!(fp.errors));
    }
}

/// The footprint of one statement's *own* SQL and expressions — not its children's, which are
/// statements of their own.
///
/// Not attempted: `RAISE` arguments (`RAISE SQLSTATE '22012'`, `USING ERRCODE = …` are not SQL
/// expressions) and `GET DIAGNOSTICS` (not SQL). A dynamic `EXECUTE` contributes the functions its
/// string-building expression calls; its target is genuinely unknown and stays a boundary.
fn statement_footprint_of(stmt: &ProcStmt) -> Footprint {
    use ekos_plpgsql::LoopKind;
    let mut fp = Footprint::default();
    let not_dynamic = |q: &str| !q.trim_start().to_ascii_uppercase().starts_with("EXECUTE");
    let expr = |fp: &mut Footprint, e: &str| {
        if !e.trim().is_empty() {
            fp.merge(expression_footprint(e));
        }
    };
    match stmt {
        ProcStmt::Sql { sql, .. } => {
            let head = sql.split_whitespace().next().unwrap_or_default();
            if !["GET", "NULL"].iter().any(|h| head.eq_ignore_ascii_case(h)) {
                fp.merge(statement_footprint(sql));
            }
        }
        ProcStmt::Perform { sql, .. } => fp.merge(statement_footprint(&format!("SELECT {sql}"))),
        ProcStmt::Assign { expr: e, .. } => expr(&mut fp, e),
        ProcStmt::If { branches, .. } => {
            for (cond, _) in branches {
                expr(&mut fp, cond);
            }
        }
        ProcStmt::Case {
            operand, branches, ..
        } => {
            if let Some(o) = operand {
                expr(&mut fp, o);
            }
            for (label, _) in branches {
                expr(&mut fp, label);
            }
        }
        ProcStmt::Loop { kind, .. } => match kind {
            LoopKind::While { condition } => expr(&mut fp, condition),
            LoopKind::ForRange { from, to, .. } => {
                expr(&mut fp, from);
                expr(&mut fp, to);
            }
            LoopKind::ForQuery { sql, .. } if not_dynamic(sql) => {
                fp.merge(statement_footprint(sql))
            }
            LoopKind::ForEach { array, .. } => expr(&mut fp, array),
            _ => {}
        },
        ProcStmt::Exit { when: Some(w), .. } => expr(&mut fp, w),
        ProcStmt::Return { value, query, .. } => {
            if let Some(q) = query.as_deref().filter(|q| not_dynamic(q)) {
                fp.merge(statement_footprint(q));
            }
            if let Some(v) = value.as_deref().filter(|v| !v.is_empty()) {
                expr(&mut fp, v);
            }
        }
        ProcStmt::Cursor { query: Some(q), .. } if not_dynamic(q) => {
            fp.merge(statement_footprint(q))
        }
        ProcStmt::DynamicExecute { expr: e, .. } => {
            // The constructed text's target is unknown; only what builds it is real.
            let mut built = expression_footprint(e);
            built.reads.clear();
            built.writes.clear();
            fp.merge(built);
        }
        ProcStmt::Block { declarations, .. } => {
            for d in declarations {
                if let Some(default) = &d.default {
                    expr(&mut fp, default);
                }
            }
        }
        _ => {}
    }
    fp
}

struct StatementEmitter<'a> {
    graph: &'a mut KirGraph,
    stats: &'a mut PlPgSqlStats,
    source_path: &'a str,
    sql: &'a str,
    key: &'a str,
    procedure: &'a str,
    /// Where the routine starts in the file; statement spans are relative to the routine.
    offset: usize,
    next_index: usize,
    /// The routine's footprint, accumulated statement by statement.
    footprint: &'a mut Footprint,
    /// The routine's parameter and variable names (RFC 0170: not columns).
    locals: &'a BTreeSet<String>,
}

impl StatementEmitter<'_> {
    /// Emit `stmt` and, recursively, everything nested inside it. Statements are numbered in
    /// pre-order, so a parent always precedes its children and the numbering is the reading order.
    fn emit(
        &mut self,
        stmt: &ProcStmt,
        parent: KirId,
        parent_index: Option<usize>,
        depth: usize,
        order: usize,
        branch: &str,
    ) {
        let index = self.next_index;
        self.next_index += 1;
        self.stats.statements += 1;
        if matches!(stmt, ProcStmt::Unrecovered { .. }) {
            self.stats.unrecovered += 1;
        }

        let Span { start, end } = stmt.span();
        let (start, end) = (self.offset + start, self.offset + end);
        let line = line_of(self.sql, start);
        let source = self.sql.get(start..end).unwrap_or_default();
        let id = kir_id(&format!("plpgsql:statement:{}:{index}", self.key));

        let mut evidence = KirEvidence::new(
            SourceLocation {
                path: self.source_path.to_string(),
                line: Some(line),
                column: None,
            },
            fragment_for(stmt, source),
        );
        evidence.id = kir_id(&format!("plpgsql:statement-evidence:{}:{index}", self.key));
        let evidence_id = self.graph.add_evidence(evidence);

        let mut obj = KirObject::new(
            format!("{}#{index}", self.procedure),
            ObjectKind::Custom("ProcedureStatement".into()),
        );
        obj.id = id;
        obj.properties = semantics(stmt);
        let fp = statement_footprint_of(stmt);
        set_footprint(&mut obj.properties, &fp);
        // RFC 0170: the statement's predicates. Its embedded SQL starts on the statement's line.
        let predicates = business_predicates(&fp.predicates, self.locals);
        if !predicates.is_empty() {
            obj.properties
                .insert("predicates".into(), predicates_json(&predicates, line));
        }
        self.footprint.merge(fp);
        for (k, v) in [
            ("procedure", json!(self.procedure)),
            ("index", json!(index)),
            ("parent_index", json!(parent_index)),
            ("depth", json!(depth)),
            ("order", json!(order)),
            ("branch", json!(branch)),
            ("source_path", json!(self.source_path)),
            ("line", json!(line)),
            ("span_start", json!(start)),
            ("span_end", json!(end)),
        ] {
            obj.properties.insert(k.into(), v);
        }
        obj.evidence.push(evidence_id);
        self.graph.add_object(obj);

        let mut contains =
            KirRelationship::deterministic(RelationshipKind::Contains, parent, id, "");
        contains.properties.insert("branch".into(), json!(branch));
        contains.properties.insert("order".into(), json!(order));
        contains.evidence.push(evidence_id);
        self.graph.add_relationship(contains);

        let child = |s: &[ProcStmt], branch: String, this: &mut Self| {
            for (order, c) in s.iter().enumerate() {
                this.emit(c, id, Some(index), depth + 1, order, &branch);
            }
        };
        match stmt {
            ProcStmt::If {
                branches,
                else_branch,
                ..
            }
            | ProcStmt::Case {
                branches,
                else_branch,
                ..
            } => {
                for (i, (_, body)) in branches.iter().enumerate() {
                    child(body, format!("then:{i}"), self);
                }
                if let Some(e) = else_branch {
                    child(e, "else".into(), self);
                }
            }
            ProcStmt::Loop { body, .. } => child(body, "body".into(), self),
            ProcStmt::Block {
                body, exception, ..
            } => {
                child(body, "body".into(), self);
                for (i, h) in exception.iter().enumerate() {
                    child(&h.body, format!("handler:{i}"), self);
                }
            }
            _ => {}
        }
    }
}

/// A statement's own semantics as properties: its serialized IR form, minus spans and minus the
/// nested statements, which are objects of their own. Derived from serde rather than listed by
/// hand, so a new `ProcStmt` variant is carried without this pass changing.
fn semantics(stmt: &ProcStmt) -> std::collections::HashMap<String, Value> {
    let Ok(Value::Object(mut map)) = serde_json::to_value(stmt) else {
        return Default::default();
    };
    map.remove("span");
    map.remove("body");
    if let Some(Value::Array(branches)) = map.remove("branches") {
        // `[condition, [statements]]` → the conditions alone, in branch order.
        let conditions: Vec<Value> = branches
            .into_iter()
            .filter_map(|b| match b {
                Value::Array(mut pair) if !pair.is_empty() => Some(pair.swap_remove(0)),
                _ => None,
            })
            .collect();
        map.insert("conditions".into(), Value::Array(conditions));
    }
    if let Some(e) = map.remove("else_branch") {
        map.insert("has_else".into(), json!(!e.is_null()));
    }
    if let Some(Value::Array(handlers)) = map.remove("exception") {
        let conditions: Vec<Value> = handlers
            .into_iter()
            .map(|h| h.get("conditions").cloned().unwrap_or(Value::Null))
            .collect();
        map.insert("handlers".into(), Value::Array(conditions));
    }
    if let Some(Value::Array(decls)) = map.remove("declarations") {
        let decls: Vec<Value> = decls
            .into_iter()
            .map(|d| json!({"name": d.get("name"), "type": d.get("data_type")}))
            .collect();
        map.insert("declarations".into(), Value::Array(decls));
    }
    map.into_iter().collect()
}

/// The evidence text for a statement. A leaf cites itself in full; a compound statement cites its
/// first line only, because its body is cited statement by statement and repeating it at every
/// level would store a deep routine once per nesting level.
fn fragment_for(stmt: &ProcStmt, source: &str) -> String {
    let compound = matches!(
        stmt,
        ProcStmt::If { .. }
            | ProcStmt::Case { .. }
            | ProcStmt::Loop { .. }
            | ProcStmt::Block { .. }
    );
    if compound {
        match source.split_once('\n') {
            Some((first, _)) => format!("{} …", clip(first.trim_end(), 200)),
            None => clip(source, 200),
        }
    } else {
        clip(source, MAX_FRAGMENT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "-- schema\n\
CREATE TABLE t (id int);\n\
CREATE OR REPLACE FUNCTION pay(in_id int, in_amt numeric(10,2)) RETURNS int AS $$\n\
DECLARE\n  n int := 0;\n\
BEGIN\n\
  IF in_amt > 0 THEN\n    UPDATE t SET id = in_id;\n  ELSE\n    RAISE EXCEPTION 'bad';\n  END IF;\n\
  BEGIN\n    INSERT INTO t VALUES (1);\n  EXCEPTION WHEN unique_violation THEN\n    RETURN 0;\n  END;\n\
  EXECUTE 'SELECT 1';\n\
  RETURN n;\n\
END $$ LANGUAGE plpgsql;\n\
CREATE FUNCTION pay(in_id int) RETURNS int AS $$ SELECT 1 $$ LANGUAGE sql;\n\
CREATE FUNCTION broken() RETURNS int AS $$ BEGIN ¿¿ nonsense; RETURN 1; END $$ LANGUAGE plpgsql;\n";

    fn of_kind<'a>(g: &'a KirGraph, kind: &str) -> Vec<&'a KirObject> {
        g.objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == kind))
            .collect()
    }

    fn prop<'a>(o: &'a KirObject, k: &str) -> &'a Value {
        o.properties.get(k).unwrap_or(&Value::Null)
    }

    #[test]
    fn every_routine_becomes_a_procedure_with_an_honest_fidelity_label() {
        let (g, stats) = recover_routines("db/schema.sql", FILE);
        let procs = of_kind(&g, "Procedure");
        assert_eq!(procs.len(), 3, "overloads are distinct routines");
        let label = |name: &str, args: usize| {
            procs
                .iter()
                .find(|p| p.name == name && prop(p, "arguments").as_array().unwrap().len() == args)
                .map(|p| prop(p, "fidelity").as_str().unwrap().to_string())
                .unwrap()
        };
        assert_eq!(label("pay", 2), "statements");
        assert_eq!(label("pay", 1), "signature");
        assert_eq!(label("broken", 0), "partial");

        let broken = procs.iter().find(|p| p.name == "broken").unwrap();
        assert_eq!(prop(broken, "statements_unrecovered"), &json!(1));
        assert_eq!(prop(broken, "statements_recovered"), &json!(1));
        assert_eq!(prop(broken, "unrecovered_lines"), &json!([21]));
        assert_eq!(prop(broken, "eligible_for_reconstruction"), &json!(false));

        let pay = procs
            .iter()
            .find(|p| p.name == "pay" && prop(p, "language") == "plpgsql");
        let pay = pay.unwrap();
        assert_eq!(prop(pay, "line"), &json!(3));
        assert_eq!(prop(pay, "dynamic_sql_sites"), &json!(1));
        assert_eq!(
            prop(pay, "arguments"),
            &json!(["in_id int", "in_amt numeric(10,2)"])
        );

        assert_eq!(
            stats,
            PlPgSqlStats {
                routines: 3,
                plpgsql: 2,
                complete: 1,
                partial: 1,
                statements: 10,
                unrecovered: 1,
                redefined: 0,
                lex_error: None,
            }
        );
    }

    /// A `LANGUAGE sql` routine has a body this crate does not parse; it gets no statements rather
    /// than an empty body that would read as "nothing in it".
    #[test]
    fn a_non_plpgsql_routine_has_no_statements() {
        let (g, _) = recover_routines("f.sql", FILE);
        let sql_pay = of_kind(&g, "Procedure")
            .into_iter()
            .find(|p| prop(p, "language") == "sql")
            .unwrap()
            .id;
        assert!(!g.relationships.iter().any(|r| r.from == sql_pay));
    }

    /// Every statement cites its own source text at the right line, and the tree records which
    /// branch each child sits in.
    #[test]
    fn statements_cite_their_source_and_keep_the_control_flow() {
        let (g, _) = recover_routines("db/schema.sql", FILE);
        let stmts: Vec<&KirObject> = of_kind(&g, "ProcedureStatement")
            .into_iter()
            .filter(|s| prop(s, "procedure") == "pay")
            .collect();
        assert_eq!(stmts.len(), 8);
        let evidence = |o: &KirObject| {
            g.evidence
                .iter()
                .find(|e| e.id == o.evidence[0])
                .unwrap()
                .clone()
        };
        for s in &stmts {
            let (a, b) = (
                prop(s, "span_start").as_u64().unwrap() as usize,
                prop(s, "span_end").as_u64().unwrap() as usize,
            );
            let ev = evidence(s);
            assert_eq!(ev.location.line, Some(line_of(FILE, a)));
            let text = &FILE[a..b];
            assert!(
                ev.fragment == text || ev.fragment.ends_with(" …"),
                "{} vs {text:?}",
                ev.fragment
            );
        }

        let by_stmt = |tag: &str| {
            stmts
                .iter()
                .find(|s| prop(s, "stmt") == tag)
                .copied()
                .unwrap_or_else(|| panic!("no {tag}"))
        };
        let iff = by_stmt("if");
        assert_eq!(prop(iff, "conditions"), &json!(["in_amt > 0"]));
        assert_eq!(prop(iff, "has_else"), &json!(true));
        assert_eq!(evidence(iff).fragment, "IF in_amt > 0 THEN …");
        let raise = by_stmt("raise");
        assert_eq!(prop(raise, "branch"), &json!("else"));
        assert_eq!(prop(raise, "parent_index"), prop(iff, "index"));
        assert_eq!(evidence(raise).fragment, "RAISE EXCEPTION 'bad'");
        assert_eq!(evidence(raise).location.line, Some(10));
        assert_eq!(prop(by_stmt("sql"), "branch"), &json!("then:0"));
        let block = by_stmt("block");
        assert_eq!(prop(block, "handlers"), &json!([["unique_violation"]]));
        let ret = stmts
            .iter()
            .find(|s| prop(s, "stmt") == "return" && prop(s, "branch") == "handler:0")
            .unwrap();
        assert_eq!(prop(ret, "parent_index"), prop(block, "index"));

        // The Contains edge to a child carries the same branch.
        let edge = g.relationships.iter().find(|r| r.to == raise.id).unwrap();
        assert_eq!(edge.kind, RelationshipKind::Contains);
        assert_eq!(edge.from, iff.id);
        assert_eq!(edge.properties.get("branch"), Some(&json!("else")));
    }

    /// Same input, same graph — object, evidence and relationship ids included (RFC 0135 Part C).
    #[test]
    fn recovery_is_deterministic() {
        let ids = |g: &KirGraph| {
            (
                g.objects.iter().map(|o| o.id).collect::<Vec<_>>(),
                g.evidence.iter().map(|e| e.id).collect::<Vec<_>>(),
                g.relationships.iter().map(|r| r.id).collect::<Vec<_>>(),
            )
        };
        let (a, _) = recover_routines("db/schema.sql", FILE);
        let (b, _) = recover_routines("db/schema.sql", FILE);
        assert_eq!(ids(&a), ids(&b));
        // A different file holding the same routine is a different routine.
        let (c, _) = recover_routines("other.sql", FILE);
        assert_ne!(ids(&a).0, ids(&c).0);
    }

    /// `CREATE OR REPLACE` later in the same file replaces the earlier definition, as it does in
    /// the database — one routine, the later body.
    #[test]
    fn a_redefinition_in_the_same_file_replaces_the_earlier_one() {
        let sql = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END $$ LANGUAGE plpgsql;\n\
                   CREATE OR REPLACE FUNCTION f() RETURNS int AS $$ BEGIN PERFORM g(); RETURN 2; END $$ LANGUAGE plpgsql;";
        let (g, stats) = recover_routines("f.sql", sql);
        assert_eq!(of_kind(&g, "Procedure").len(), 1);
        assert_eq!(stats.redefined, 1);
        assert_eq!(stats.routines, 1);
        assert_eq!(of_kind(&g, "ProcedureStatement").len(), 2);
    }

    #[test]
    fn a_file_that_does_not_lex_recovers_nothing_and_says_why() {
        let (g, stats) = recover_routines("f.sql", "CREATE FUNCTION f() AS $$ BEGIN");
        assert!(g.objects.is_empty());
        assert!(stats.lex_error.is_some());
    }

    #[test]
    fn only_postgres_files_or_files_naming_plpgsql_are_analyzed() {
        assert!(PlPgSqlAnalyzerPass::applies_to(
            "postgres",
            "CREATE TABLE t (id int);"
        ));
        assert!(PlPgSqlAnalyzerPass::applies_to(
            "generic",
            "CREATE FUNCTION f() ... LANGUAGE PLPGSQL;"
        ));
        assert!(!PlPgSqlAnalyzerPass::applies_to(
            "mssql",
            "CREATE PROCEDURE p AS BEGIN SELECT 1 END"
        ));
    }

    /// Each statement records what its own SQL and expressions touch; the routine records the
    /// union, and a `LANGUAGE sql` routine gets its body's footprint without statements.
    #[test]
    fn statements_and_routines_record_what_they_read_write_and_call() {
        let sql = "CREATE FUNCTION post(in_id int) RETURNS int AS $$\n\
DECLARE t_uid int := person__get_my_entity_id();\n\
BEGIN\n\
  IF EXISTS (SELECT 1 FROM account WHERE id = in_id) THEN\n\
    INSERT INTO journal_line (account_id) SELECT id FROM account WHERE id = in_id;\n\
  END IF;\n\
  PERFORM setting_increment('glnumber');\n\
  FOR r IN SELECT * FROM acc_trans WHERE trans_id = in_id LOOP\n\
    UPDATE invoice SET allocated = 0 WHERE id = r.invoice_id;\n\
  END LOOP;\n\
  EXECUTE format('DELETE FROM %I', 'secret_target');\n\
  GET DIAGNOSTICS n = ROW_COUNT;\n\
  RETURN t_uid;\n\
END $$ LANGUAGE plpgsql;\n\
CREATE FUNCTION open_items(in_acc int) RETURNS SETOF open_item AS $$\n\
  SELECT * FROM open_item WHERE account_id = in_acc;\n\
  UPDATE account SET touched = true WHERE id = in_acc;\n\
$$ LANGUAGE sql;";
        let (g, _) = recover_routines("f.sql", sql);
        let strs = |o: &KirObject, k: &str| -> Vec<String> {
            prop(o, k)
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        };
        let procs = of_kind(&g, "Procedure");
        let post = procs.iter().find(|p| p.name == "post").unwrap();
        assert_eq!(strs(post, "reads"), ["acc_trans", "account"]);
        assert_eq!(strs(post, "writes"), ["invoice", "journal_line"]);
        let calls = strs(post, "calls");
        for c in ["person__get_my_entity_id", "setting_increment", "format"] {
            assert!(calls.contains(&c.to_string()), "{calls:?}");
        }
        // The dynamic statement's constructed target is never claimed.
        assert!(!strs(post, "writes").contains(&"secret_target".to_string()));
        assert_eq!(
            prop(post, "footprint"),
            &json!("parsed"),
            "{:?}",
            post.properties
        );

        let stmt = |tag: &str| {
            of_kind(&g, "ProcedureStatement")
                .into_iter()
                .find(|s| prop(s, "procedure") == "post" && prop(s, "stmt") == tag)
                .unwrap()
        };
        // The IF reads what its condition reads; the INSERT inside it is its own statement.
        assert_eq!(strs(stmt("if"), "reads"), ["account"]);
        assert!(strs(stmt("if"), "writes").is_empty());
        assert_eq!(strs(stmt("loop"), "reads"), ["acc_trans"]);
        assert_eq!(prop(stmt("loop"), "footprint"), &json!("parsed"));

        let items = procs.iter().find(|p| p.name == "open_items").unwrap();
        assert_eq!(prop(items, "fidelity"), &json!("signature"));
        assert_eq!(strs(items, "reads"), ["open_item"]);
        assert_eq!(strs(items, "writes"), ["account"]);
    }

    /// A routine hangs off its `File`, carries the line span `llm_description` reads source by,
    /// and takes the author's `COMMENT ON FUNCTION` as its description, cited at the comment.
    #[test]
    fn a_routine_belongs_to_its_file_and_carries_its_documented_description() {
        let sql = "CREATE FUNCTION pay(in_id int) RETURNS int AS $$\nBEGIN\n  RETURN 1;\nEND $$ LANGUAGE plpgsql;\n\
                   COMMENT ON FUNCTION pay(int) IS $$ Posts a payment. $$;\n\
                   CREATE FUNCTION quiet() RETURNS int AS $$ SELECT 1 $$ LANGUAGE sql;";
        let (g, _) = recover_routines_in(
            "sql/modules/Payment.sql",
            sql,
            Some("sql/modules/Payment.sql"),
        );
        let file_id = KirId(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            b"sql/modules/Payment.sql",
        ));
        let procs = of_kind(&g, "Procedure");
        for p in &procs {
            assert!(
                g.relationships
                    .iter()
                    .any(|r| r.kind == RelationshipKind::Contains
                        && r.from == file_id
                        && r.to == p.id),
                "{} has no Contains edge from its File",
                p.name
            );
        }
        let pay = procs.iter().find(|p| p.name == "pay").unwrap();
        assert_eq!(
            prop(pay, "source_span"),
            &json!({"start_line": 1, "end_line": 4})
        );
        assert_eq!(prop(pay, "description"), &json!("Posts a payment."));
        let cited = g
            .evidence
            .iter()
            .find(|e| pay.evidence.contains(&e.id) && e.fragment.starts_with("COMMENT ON"))
            .expect("the description cites its comment");
        assert_eq!(cited.location.line, Some(5));
        let quiet = procs.iter().find(|p| p.name == "quiet").unwrap();
        assert_eq!(
            prop(quiet, "description"),
            &Value::Null,
            "no comment, no description"
        );
    }

    /// RFC 0163 triggers: a trigger function records the structural facts its trigger is
    /// classified by — which `NEW` columns it sets, whether it raises, whether it drops the row,
    /// and what it writes, per operation.
    #[test]
    fn a_trigger_function_records_what_its_classification_needs() {
        let sql = "CREATE FUNCTION trg() RETURNS trigger AS $$\nBEGIN\n\
  IF new.amount < 0 THEN RAISE EXCEPTION 'negative'; END IF;\n\
  IF new.skip THEN RETURN NULL; END IF;\n\
  NEW.updated := now();\n\
  INSERT INTO open_item (account_id) VALUES (new.chart_id) RETURNING id INTO new.open_item_id;\n\
  INSERT INTO audit_log SELECT new.*;\n\
  UPDATE balance SET total = total + new.amount;\n\
  RETURN NEW;\nEND $$ LANGUAGE plpgsql;\n\
CREATE FUNCTION plain() RETURNS int AS $$ BEGIN RETURN 1; END $$ LANGUAGE plpgsql;";
        let (g, _) = recover_routines("t.sql", sql);
        let procs = of_kind(&g, "Procedure");
        let trg = procs.iter().find(|p| p.name == "trg").unwrap();
        assert_eq!(
            prop(trg, "assigns_new"),
            &json!(["open_item_id", "updated"])
        );
        assert_eq!(prop(trg, "raises_exception"), &json!(1));
        assert_eq!(prop(trg, "returns_null"), &json!(true));
        assert_eq!(prop(trg, "inserts"), &json!(["audit_log", "open_item"]));
        assert_eq!(prop(trg, "updates"), &json!(["balance"]));
        assert_eq!(prop(trg, "deletes"), &json!([]));
        assert_eq!(prop(trg, "pass_through"), &json!(false));
        let (stub, _) = recover_routines(
            "s.sql",
            "CREATE FUNCTION s() RETURNS trigger AS $$ BEGIN -- dummy\n RETURN NEW; END $$ LANGUAGE plpgsql;",
        );
        assert_eq!(
            prop(of_kind(&stub, "Procedure")[0], "pass_through"),
            &json!(true)
        );
        let (empty, _) = recover_routines(
            "s.sql",
            "CREATE FUNCTION e() RETURNS trigger AS $$ BEGIN END; $$ LANGUAGE plpgsql;",
        );
        assert_eq!(
            prop(of_kind(&empty, "Procedure")[0], "pass_through"),
            &json!(true)
        );
        let (cond, _) = recover_routines(
            "s.sql",
            "CREATE FUNCTION c() RETURNS trigger AS $$ BEGIN IF tg_op = 'DELETE' THEN RETURN NULL; END IF; RETURN NEW; END $$ LANGUAGE plpgsql;",
        );
        assert_eq!(
            prop(of_kind(&cond, "Procedure")[0], "pass_through"),
            &json!(false)
        );
        // Only trigger functions carry the trigger facts.
        let plain = procs.iter().find(|p| p.name == "plain").unwrap();
        assert_eq!(prop(plain, "assigns_new"), &Value::Null);
    }

    #[tokio::test]
    async fn the_pass_writes_one_knowledge_artifact() {
        use ekos_compiler_core::EkosConfig;
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf());
        let mut pass = PlPgSqlAnalyzerPass::new("db/schema.sql", FILE);
        let stats = pass.stats_handle();
        pass.run(&mut ctx).await.unwrap();
        assert_eq!(ctx.artifact_store.list().unwrap().len(), 1);
        assert_eq!(stats.lock().unwrap().complete, 1);
    }
}
