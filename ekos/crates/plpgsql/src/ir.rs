//! RFC 0163 — the procedural IR.
//!
//! # Why a second IR
//!
//! `TransformNode` (`semantic/src/transform_ir.rs`) is a **dataflow** graph — `Source`, `Filter`,
//! `Join`, `Aggregate`, `Calculate`, `Sink`, `Unmapped` — shared by Pentaho and plain SQL. PL/pgSQL
//! is **imperative**: ordered effects, conditions, loops, early returns, exceptions.
//!
//! Forcing control flow into a dataflow graph would either lose the ordering, making the IR wrong,
//! or give every existing consumer node kinds that mean nothing in its own domain. So the procedural
//! layer owns *order and condition*, and each embedded SQL statement is carried with its text and
//! span for the dataflow layer to lower separately.

use serde::{Deserialize, Serialize};

/// Byte offsets into the function body. Every statement cites its source exactly, the way RFC 0150
/// cites IL ranges — a claim about behaviour that cannot point at a span is not a claim this system
/// makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarDecl {
    pub name: String,
    pub data_type: String,
    pub default: Option<String>,
    pub constant: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorOp {
    Open,
    Fetch,
    Move,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "loop")]
pub enum LoopKind {
    Plain,
    While {
        condition: String,
    },
    ForRange {
        var: String,
        from: String,
        to: String,
        reverse: bool,
    },
    ForQuery {
        var: String,
        sql: String,
    },
    ForEach {
        var: String,
        array: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExceptionHandler {
    /// The conditions caught: `no_data_found`, `unique_violation`, `others`.
    pub conditions: Vec<String>,
    pub body: Vec<ProcStmt>,
    pub span: Span,
}

/// One statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "stmt")]
pub enum ProcStmt {
    /// An embedded SQL statement. RFC 0164 lowers `sql` into the dataflow IR; this layer records
    /// that it happens *here*, in this order, under these conditions.
    Sql {
        sql: String,
        into: Option<Vec<String>>,
        span: Span,
    },
    Assign {
        target: String,
        expr: String,
        span: Span,
    },
    If {
        branches: Vec<(String, Vec<ProcStmt>)>,
        else_branch: Option<Vec<ProcStmt>>,
        span: Span,
    },
    Case {
        operand: Option<String>,
        branches: Vec<(String, Vec<ProcStmt>)>,
        else_branch: Option<Vec<ProcStmt>>,
        span: Span,
    },
    Loop {
        kind: LoopKind,
        body: Vec<ProcStmt>,
        label: Option<String>,
        span: Span,
    },
    Exit {
        label: Option<String>,
        when: Option<String>,
        is_continue: bool,
        span: Span,
    },
    Return {
        value: Option<String>,
        query: Option<String>,
        next: bool,
        span: Span,
    },
    Raise {
        level: String,
        message: String,
        span: Span,
    },
    Block {
        declarations: Vec<VarDecl>,
        body: Vec<ProcStmt>,
        exception: Vec<ExceptionHandler>,
        span: Span,
    },
    Perform {
        sql: String,
        span: Span,
    },
    /// A cursor operation. Cursors are how a lot of older PL/pgSQL walks a result set, and a
    /// routine using one is not a routine with a gap in it.
    Cursor {
        op: CursorOp,
        name: String,
        /// The query, for `OPEN … FOR`.
        query: Option<String>,
        /// The targets, for `FETCH … INTO`.
        into: Option<Vec<String>>,
        span: Span,
    },
    /// Dynamic SQL. The IR records that the statement is **constructed**, and from what.
    ///
    /// This is a boundary, not a failure: the target genuinely is not statically known, and marking
    /// it is more useful than pretending otherwise. RFC 0164 refuses to reconstruct across one.
    DynamicExecute {
        expr: String,
        using: Vec<String>,
        into: Option<Vec<String>>,
        span: Span,
    },
    /// Position known, semantics not recovered. **Never silently omitted** — a dropped statement is
    /// how an anti-invention check ends up with nothing to check against.
    Unrecovered {
        raw: String,
        reason: String,
        span: Span,
    },
}

impl ProcStmt {
    pub fn span(&self) -> Span {
        match self {
            Self::Sql { span, .. }
            | Self::Assign { span, .. }
            | Self::If { span, .. }
            | Self::Case { span, .. }
            | Self::Loop { span, .. }
            | Self::Exit { span, .. }
            | Self::Return { span, .. }
            | Self::Raise { span, .. }
            | Self::Block { span, .. }
            | Self::Perform { span, .. }
            | Self::Cursor { span, .. }
            | Self::DynamicExecute { span, .. }
            | Self::Unrecovered { span, .. } => *span,
        }
    }

    /// Every span in this statement and everything nested inside it, handlers included.
    pub(crate) fn spans_mut(&mut self, f: &mut dyn FnMut(&mut Span)) {
        let each = |v: &mut Vec<ProcStmt>, f: &mut dyn FnMut(&mut Span)| {
            for s in v {
                s.spans_mut(f);
            }
        };
        match self {
            Self::If {
                branches,
                else_branch,
                span,
            }
            | Self::Case {
                branches,
                else_branch,
                span,
                ..
            } => {
                f(span);
                for (_, body) in branches {
                    each(body, f);
                }
                if let Some(e) = else_branch {
                    each(e, f);
                }
            }
            Self::Loop { body, span, .. } => {
                f(span);
                each(body, f);
            }
            Self::Block {
                declarations,
                body,
                exception,
                span,
            } => {
                f(span);
                for d in declarations {
                    f(&mut d.span);
                }
                each(body, f);
                for h in exception {
                    f(&mut h.span);
                    each(&mut h.body, f);
                }
            }
            Self::Sql { span, .. }
            | Self::Assign { span, .. }
            | Self::Exit { span, .. }
            | Self::Return { span, .. }
            | Self::Raise { span, .. }
            | Self::Perform { span, .. }
            | Self::Cursor { span, .. }
            | Self::DynamicExecute { span, .. }
            | Self::Unrecovered { span, .. } => f(span),
        }
    }

    /// Walk this statement and everything nested inside it.
    pub fn walk(&self, f: &mut impl FnMut(&ProcStmt)) {
        f(self);
        let mut each = |v: &Vec<ProcStmt>| {
            for s in v {
                s.walk(f);
            }
        };
        match self {
            Self::If {
                branches,
                else_branch,
                ..
            }
            | Self::Case {
                branches,
                else_branch,
                ..
            } => {
                for (_, body) in branches {
                    each(body);
                }
                if let Some(e) = else_branch {
                    each(e);
                }
            }
            Self::Loop { body, .. } => each(body),
            Self::Block {
                body, exception, ..
            } => {
                each(body);
                for h in exception {
                    each(&h.body);
                }
            }
            _ => {}
        }
    }
}

/// How much of a routine was actually recovered.
///
/// Computed from the IR, never asserted by the producer — the same discipline RFC 0150 applies to
/// binary recovery levels, and the reason a consumer can trust the label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "fidelity")]
pub enum Fidelity {
    /// Signature only. The body was not parsed — a C or PL/Python routine.
    Signature,
    /// Parsed, with gaps. Carries the counts so a consumer can decide for itself and a report can
    /// say "41 of 44 statements recovered" rather than "recovered".
    Partial {
        recovered: usize,
        unrecovered: usize,
    },
    /// Every statement recovered.
    Statements,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcSignature {
    pub name: String,
    pub arguments: Vec<String>,
    pub returns: Option<String>,
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcedureIr {
    pub signature: ProcSignature,
    pub declarations: Vec<VarDecl>,
    pub body: Vec<ProcStmt>,
    fidelity: Fidelity,
}

impl ProcedureIr {
    /// Build, computing fidelity from the body.
    ///
    /// The invariant: **nothing is labelled `Statements` that contains a single `Unrecovered`
    /// node.** Enforced here rather than by convention, because the label is what a consumer trusts
    /// and a producer asserting it is exactly the thing that should not be possible.
    pub fn new(signature: ProcSignature, declarations: Vec<VarDecl>, body: Vec<ProcStmt>) -> Self {
        let mut recovered = 0usize;
        let mut unrecovered = 0usize;
        for s in &body {
            s.walk(&mut |x| {
                if matches!(x, ProcStmt::Unrecovered { .. }) {
                    unrecovered += 1;
                } else {
                    recovered += 1;
                }
            });
        }
        let fidelity = if unrecovered > 0 {
            Fidelity::Partial {
                recovered,
                unrecovered,
            }
        } else {
            Fidelity::Statements
        };
        Self {
            signature,
            declarations,
            body,
            fidelity,
        }
    }

    /// A routine whose body was not parsed at all.
    pub fn signature_only(signature: ProcSignature) -> Self {
        Self {
            signature,
            declarations: Vec::new(),
            body: Vec::new(),
            fidelity: Fidelity::Signature,
        }
    }

    pub fn fidelity(&self) -> Fidelity {
        self.fidelity
    }

    /// Whether RFC 0164 may attempt a constrained reconstruction of this routine.
    ///
    /// Only at `Statements`. A model given a partial recovery confidently fills the gaps, and the
    /// anti-invention check cannot catch an invention that fills a hole the check cannot see.
    pub fn eligible_for_reconstruction(&self) -> bool {
        self.fidelity == Fidelity::Statements
    }

    /// Every unrecovered statement's span, so a finding can point at what was missed.
    pub fn gaps(&self) -> Vec<Span> {
        let mut out = Vec::new();
        for s in &self.body {
            s.walk(&mut |x| {
                if let ProcStmt::Unrecovered { span, .. } = x {
                    out.push(*span);
                }
            });
        }
        out
    }

    /// Statements that construct SQL at runtime. Recovered faithfully, and a boundary RFC 0164 will
    /// not cross.
    pub fn dynamic_sites(&self) -> Vec<Span> {
        let mut out = Vec::new();
        for s in &self.body {
            s.walk(&mut |x| {
                if let ProcStmt::DynamicExecute { span, .. } = x {
                    out.push(*span);
                }
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Span {
        Span { start: 0, end: 1 }
    }

    fn sig() -> ProcSignature {
        ProcSignature {
            name: "f".into(),
            arguments: vec![],
            returns: Some("int".into()),
            language: "plpgsql".into(),
        }
    }

    fn sql(s: &str) -> ProcStmt {
        ProcStmt::Sql {
            sql: s.into(),
            into: None,
            span: span(),
        }
    }

    #[test]
    fn a_fully_recovered_body_is_labelled_statements() {
        let ir = ProcedureIr::new(sig(), vec![], vec![sql("SELECT 1"), sql("SELECT 2")]);
        assert_eq!(ir.fidelity(), Fidelity::Statements);
        assert!(ir.eligible_for_reconstruction());
        assert!(ir.gaps().is_empty());
    }

    /// The invariant. A producer cannot assert `Statements` over a body with a gap, because it
    /// cannot assert the label at all.
    #[test]
    fn one_unrecovered_statement_prevents_the_statements_label() {
        let ir = ProcedureIr::new(
            sig(),
            vec![],
            vec![
                sql("SELECT 1"),
                ProcStmt::Unrecovered {
                    raw: "???".into(),
                    reason: "unparseable".into(),
                    span: span(),
                },
            ],
        );
        assert_eq!(
            ir.fidelity(),
            Fidelity::Partial {
                recovered: 1,
                unrecovered: 1
            }
        );
        assert!(
            !ir.eligible_for_reconstruction(),
            "a model given a partial recovery fills the gaps, and the check cannot see it"
        );
        assert_eq!(ir.gaps().len(), 1);
    }

    /// A gap nested inside a branch counts. Walking only the top level is how a partial body gets
    /// labelled complete.
    #[test]
    fn a_nested_gap_is_found() {
        let ir = ProcedureIr::new(
            sig(),
            vec![],
            vec![ProcStmt::If {
                branches: vec![(
                    "x > 0".into(),
                    vec![ProcStmt::Unrecovered {
                        raw: "???".into(),
                        reason: "unparseable".into(),
                        span: Span { start: 5, end: 9 },
                    }],
                )],
                else_branch: None,
                span: span(),
            }],
        );
        assert!(matches!(ir.fidelity(), Fidelity::Partial { .. }));
        assert_eq!(ir.gaps(), vec![Span { start: 5, end: 9 }]);
    }

    /// Dynamic SQL is a faithful recovery of something that genuinely is not statically known, so
    /// it does **not** reduce fidelity — but it is reported as a boundary.
    #[test]
    fn dynamic_execute_does_not_reduce_fidelity_but_is_reported() {
        let ir = ProcedureIr::new(
            sig(),
            vec![],
            vec![ProcStmt::DynamicExecute {
                expr: "'SELECT ' || col".into(),
                using: vec![],
                into: None,
                span: Span { start: 3, end: 7 },
            }],
        );
        assert_eq!(ir.fidelity(), Fidelity::Statements);
        assert!(ir.eligible_for_reconstruction());
        assert_eq!(ir.dynamic_sites(), vec![Span { start: 3, end: 7 }]);
    }

    #[test]
    fn a_signature_only_routine_is_never_eligible() {
        let ir = ProcedureIr::signature_only(ProcSignature {
            language: "c".into(),
            ..sig()
        });
        assert_eq!(ir.fidelity(), Fidelity::Signature);
        assert!(!ir.eligible_for_reconstruction());
    }

    /// An empty body is `Statements` — there was nothing to miss — but it is still worth asserting,
    /// because "recovered everything" over nothing is the kind of vacuous pass that hides elsewhere.
    #[test]
    fn an_empty_body_is_statements_with_nothing_recovered() {
        let ir = ProcedureIr::new(sig(), vec![], vec![]);
        assert_eq!(ir.fidelity(), Fidelity::Statements);
        assert!(ir.gaps().is_empty());
    }

    #[test]
    fn walk_reaches_every_nesting_construct() {
        let inner = sql("SELECT 1");
        for outer in [
            ProcStmt::Loop {
                kind: LoopKind::Plain,
                body: vec![inner.clone()],
                label: None,
                span: span(),
            },
            ProcStmt::Block {
                declarations: vec![],
                body: vec![inner.clone()],
                exception: vec![],
                span: span(),
            },
            ProcStmt::Block {
                declarations: vec![],
                body: vec![],
                exception: vec![ExceptionHandler {
                    conditions: vec!["others".into()],
                    body: vec![inner.clone()],
                    span: span(),
                }],
                span: span(),
            },
            ProcStmt::Case {
                operand: None,
                branches: vec![("x".into(), vec![inner.clone()])],
                else_branch: None,
                span: span(),
            },
            ProcStmt::If {
                branches: vec![],
                else_branch: Some(vec![inner.clone()]),
                span: span(),
            },
        ] {
            let mut n = 0;
            outer.walk(&mut |_| n += 1);
            assert_eq!(n, 2, "walk missed the nested statement in {outer:?}");
        }
    }
}
