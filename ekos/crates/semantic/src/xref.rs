//! RFC 0233 — the cross-reference integrity check.
//!
//! EKOS resolves names across systems in several places (RFC 0163/0169 routine and view
//! footprints, RFC 0075 transformation steps, RFC 0209 SQL embedded in Java) and used to drop a
//! name that resolved to nothing, or to several objects, without a word. This turns those names
//! into diagnostics: what referred, from which file and line, what it looked for, the closest
//! candidates, the evidence, a confidence and a fingerprint stable across recompiles.
//!
//! Pure: objects in, [`Report`] out. Resolution uses [`NameIndex`], the rule lineage links by, so
//! a name this check calls resolved is exactly a name lineage linked. Findings never go into the
//! ledger — `ekos commit` writes them to the integrity index file and `ekos check refs` recomputes.
//!
//! Phase 1 sources (RFC 0233 §4): `Procedure`/`View` `reads`/`writes` (lines from their
//! `ProcedureStatement`s), Pentaho and SQL `TransformNode` Source/Sink `object_name`, `JavaMethod`
//! `sql[]`, and dbt `source()` tables against the schemas other systems define. Routine calls are
//! Phase 2: built-ins (`coalesce`, `now`) are recorded as calls and are not yet told apart.

use crate::procedure_lineage::{NameIndex, is_live};
use ekos_kir::{KirId, KirObject, ObjectKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Bump when the rules change what a report contains, so a stored baseline is known to be older.
pub const XREF_VERSION: u32 = 2;

/// The Java SQL scanner's stand-in for a fragment it could not fold (`java_sql::PLACEHOLDER`).
const JAVA_PLACEHOLDER: &str = "ekos_expr";

/// The kind of finding. Phase 1 produces the first two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DiagnosticKind {
    UnresolvedReference,
    AmbiguousReference,
    UnreachableSymbol,
    DynamicReferenceUnknown,
    ReferenceCycle,
    OrphanArtifact,
}

impl DiagnosticKind {
    pub const ALL: [DiagnosticKind; 6] = [
        Self::UnresolvedReference,
        Self::AmbiguousReference,
        Self::UnreachableSymbol,
        Self::DynamicReferenceUnknown,
        Self::ReferenceCycle,
        Self::OrphanArtifact,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnresolvedReference => "UnresolvedReference",
            Self::AmbiguousReference => "AmbiguousReference",
            Self::UnreachableSymbol => "UnreachableSymbol",
            Self::DynamicReferenceUnknown => "DynamicReferenceUnknown",
            Self::ReferenceCycle => "ReferenceCycle",
            Self::OrphanArtifact => "OrphanArtifact",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Ordered `Info < Warning < Error`, so "at or above" is a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "info" => Some(Self::Info),
            "warning" => Some(Self::Warning),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Where a reference comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRef {
    pub object_id: KirId,
    pub object_kind: String,
    pub object_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Which recovery produced the reference (`procedure-footprint`, `java-sql`, `pentaho`, …).
    pub extractor: String,
    /// `read`, `write` or (dbt sources) `declare`.
    pub access: String,
}

/// What was looked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetSpec {
    /// The name as the source wrote it.
    pub written: String,
    /// Lower-cased, quotes stripped — what was compared.
    pub normalized: String,
    /// The kind of object expected; Phase 1 checks relations only.
    pub expected: String,
    /// The schema qualifier, when the name had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// A compiled object the target might be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: KirId,
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub severity: Severity,
    pub confidence: f64,
    pub source: SourceRef,
    pub target_spec: TargetSpec,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<Candidate>,
    /// The referring object's evidence ids.
    #[serde(default)]
    pub evidence: Vec<KirId>,
    /// The target points at a namespace EKOS did not compile — a coverage gap, not a defect.
    #[serde(default)]
    pub coverage_gap: bool,
    /// The reference leaves its own system (code, ETL or dbt → schema).
    #[serde(default)]
    pub cross_system: bool,
    /// Why this severity and confidence, in words.
    pub reason: String,
    /// Stable across recompiles: kind, source kind/file/name, access and the normalized target.
    pub fingerprint: String,
}

/// Resolution counts for one slice of the references.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    pub references: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub ambiguous: usize,
}

impl Counts {
    /// Resolved / (resolved + unresolved + ambiguous); `None` when nothing was checked.
    pub fn resolution_rate(&self) -> Option<f64> {
        let checked = self.resolved + self.unresolved + self.ambiguous;
        (checked > 0).then(|| self.resolved as f64 / checked as f64)
    }

    fn add(&mut self, outcome: Outcome) {
        self.references += 1;
        match outcome {
            Outcome::Resolved => self.resolved += 1,
            Outcome::Unresolved => self.unresolved += 1,
            Outcome::Ambiguous => self.ambiguous += 1,
        }
    }
}

/// RFC 0233 §6.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub all: Counts,
    pub cross_system: Counts,
    /// Names that cannot be resolved by reading them: Java placeholders, template markers.
    pub dynamic: usize,
    /// System catalog relations (`pg_catalog.*`, `dual`, …) — not references to the estate.
    pub system: usize,
    /// References from an object whose source file is gone from the committed manifest.
    pub stale_sources: usize,
    /// Imports of packages from outside the workspace (CPAN, npm, PyPI) — not checked.
    #[serde(default)]
    pub external: usize,
    /// `ReferenceCycle`s between modules.
    #[serde(default)]
    pub cycles: usize,
    /// `OrphanArtifact`s by kind.
    #[serde(default)]
    pub orphans: BTreeMap<String, usize>,
    /// Phase 3: per language family, reachability coverage beside its dynamic share.
    #[serde(default)]
    pub reachability: BTreeMap<String, crate::xref_reach::FamilyReach>,
    pub by_extractor: BTreeMap<String, Counts>,
    pub resolution_rate: Option<f64>,
    pub cross_system_resolution_rate: Option<f64>,
}

/// One run of the check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub version: u32,
    pub diagnostics: Vec<Diagnostic>,
    pub metrics: Metrics,
}

/// An object's (file, line) from somewhere other than its properties.
/// The file and line an evidence record cites.
pub type Locate<'a> = dyn Fn(&KirId) -> Option<(String, Option<u32>)> + 'a;

/// What the caller supplies beyond the objects.
#[derive(Default)]
pub struct Options<'a> {
    /// The files the last commit observed; a source whose file is not in it is skipped.
    pub live_files: Option<&'a BTreeSet<String>>,
    /// Location of an object that records none in its properties (a `JavaMethod`'s file is only in
    /// its evidence). Returns (file, line).
    pub locate: Option<&'a Locate<'a>>,
    /// Files that exist under an observed directory but were not observed (`[observe]
    /// ignore-patterns`): an import that lands in one is a coverage gap, not a defect.
    pub unobserved_files: Option<&'a BTreeSet<String>>,
    /// `[xref] entry-points`: globs over symbol names that count as reachability roots.
    pub entry_points: &'a [String],
    /// The ledger's relationships, for the checks that follow edges (imports, cycles, orphans).
    pub relationships: &'a [ekos_kir::KirRelationship],
    /// Per-kind severity overrides from `[xref.severity]`.
    pub severity: BTreeMap<DiagnosticKind, Severity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Resolved,
    Unresolved,
    Ambiguous,
}

/// One name some object refers to.
pub(crate) struct Reference<'a> {
    pub(crate) source: &'a KirObject,
    pub(crate) written: String,
    pub(crate) access: &'static str,
    pub(crate) extractor: &'static str,
    pub(crate) file: Option<String>,
    pub(crate) line: Option<u32>,
    pub(crate) cross_system: bool,
    /// The SQL text it came from was not wholly literal, or the routine runs dynamic SQL.
    pub(crate) uncertain: bool,
    /// dbt sources resolve against the other systems' relations only.
    pub(crate) against_foreign: bool,
    /// What kind of object the name should be.
    pub(crate) expects: Expect,
}

/// The kind of target a reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expect {
    Relation,
    /// A routine call; `dialect` picks the built-in function list.
    Routine {
        dialect: SqlDialect,
    },
    /// A module or package import (judged in `xref_imports`).
    Module,
}

/// Whose built-in functions a call may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SqlDialect {
    Postgres,
    Oracle,
}

/// The file that declares each object, from `File → object` `Contains` edges — the only place a
/// Perl package's or an Elixir module's file is recorded.
pub(crate) fn declaring_files(
    objects: &[KirObject],
    rels: &[ekos_kir::KirRelationship],
) -> HashMap<uuid::Uuid, String> {
    let files: HashMap<uuid::Uuid, &str> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::File)
        .map(|o| (o.id.0, o.name.as_str()))
        .collect();
    let mut out = HashMap::new();
    for r in rels
        .iter()
        .filter(|r| r.kind == ekos_kir::RelationshipKind::Contains)
    {
        if let Some(f) = files.get(&r.from.0) {
            out.entry(r.to.0).or_insert_with(|| (*f).to_string());
        }
    }
    out
}

/// Where an object's first evidence record points.
pub(crate) fn locate_object(opts: &Options<'_>, o: &KirObject) -> Option<(String, Option<u32>)> {
    let f = opts.locate?;
    f(o.evidence.first()?)
}

pub(crate) fn custom_kind(o: &KirObject) -> Option<&str> {
    match &o.kind {
        ObjectKind::Custom(k) => Some(k.as_str()),
        _ => None,
    }
}

fn is_dbt(o: &KirObject) -> bool {
    o.properties.contains_key("dbt_kind")
}

fn is_relation(o: &KirObject) -> bool {
    matches!(o.kind, ObjectKind::Table | ObjectKind::Dataset)
        || matches!(custom_kind(o), Some("View" | "ModelTable"))
}

pub(crate) fn str_prop(o: &KirObject, key: &str) -> Option<String> {
    o.properties
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

pub(crate) fn u32_prop(v: Option<&serde_json::Value>) -> Option<u32> {
    v.and_then(serde_json::Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

fn strings(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The last dotted part of a normalized name.
pub(crate) fn tail_of(n: &str) -> &str {
    n.rsplit('.').next().unwrap_or(n)
}

/// Lower-cased, quotes and brackets stripped from each dotted part.
pub fn normalize(name: &str) -> String {
    name.split('.')
        .map(|p| {
            p.trim()
                .trim_matches(|c| matches!(c, '"' | '`' | '[' | ']'))
        })
        .collect::<Vec<_>>()
        .join(".")
        .to_lowercase()
}

/// A relation of a database's own catalog, not of the estate.
pub(crate) fn is_system_relation(n: &str) -> bool {
    let (schema, tail) = match n.rsplit_once('.') {
        Some((s, t)) => (Some(s.rsplit('.').next().unwrap_or(s)), t),
        None => (None, n),
    };
    if matches!(
        schema,
        Some("pg_catalog" | "information_schema" | "sys" | "system" | "mysql")
    ) {
        return true;
    }
    tail == "dual"
        || BUILTIN_TABLE_FUNCTIONS.contains(&tail)
        || tail.starts_with("pg_")
        || tail.starts_with("v$")
        || tail.starts_with("gv$")
        || ["all_", "dba_", "user_"]
            .iter()
            .any(|p| tail.starts_with(p) && tail.len() > p.len() && is_oracle_dictionary(tail))
}

/// Set-returning built-ins a `FROM` names like a relation.
const BUILTIN_TABLE_FUNCTIONS: &[&str] = &[
    "generate_series",
    "unnest",
    "json_each",
    "jsonb_each",
    "json_each_text",
    "jsonb_each_text",
    "json_array_elements",
    "jsonb_array_elements",
    "json_array_elements_text",
    "jsonb_array_elements_text",
    "json_to_recordset",
    "jsonb_to_recordset",
    "json_populate_recordset",
    "jsonb_populate_recordset",
    "regexp_split_to_table",
    "regexp_matches",
    "string_to_table",
    "generate_subscripts",
    "numbers",
    "table",
];

/// `all_tables`, `user_tab_columns` — not any table that happens to start with `user_`.
fn is_oracle_dictionary(tail: &str) -> bool {
    let rest = tail.split_once('_').map(|x| x.1).unwrap_or("");
    [
        "tables",
        "tab_columns",
        "tab_cols",
        "objects",
        "views",
        "indexes",
        "ind_columns",
        "constraints",
        "cons_columns",
        "source",
        "sequences",
        "synonyms",
        "triggers",
        "procedures",
        "arguments",
        "users",
        "tab_comments",
        "col_comments",
        "dependencies",
        "errors",
        "tab_partitions",
        "ind_partitions",
        "ind_subpartitions",
        "part_tables",
        "part_key_columns",
        "tab_subpartitions",
        "segments",
        "mviews",
        "role_privs",
        "tab_privs",
        "sys_privs",
        "jobs",
        "scheduler_jobs",
    ]
    .contains(&rest)
}

/// A name that cannot be resolved by reading it.
fn is_dynamic(n: &str) -> bool {
    n.contains(JAVA_PLACEHOLDER) || ["[%", "{{", "${", "%s", "?"].iter().any(|m| n.contains(m))
}

/// A work table a routine makes for itself.
fn is_work_table(n: &str) -> bool {
    let tail = n.rsplit('.').next().unwrap_or(n);
    n.starts_with("pg_temp.")
        || tail.starts_with('#')
        || ["tmp_", "temp_", "tt_"].iter().any(|p| tail.starts_with(p))
        || tail.ends_with("_tmp")
        || tail.ends_with("_temp")
}

/// Schemas that mean "the default one", so `public.t` is as covered as `t`.
const DEFAULT_SCHEMAS: &[&str] = &["public", "dbo", "main", "default"];

/// The schema qualifier of a normalized name, unless it is a default schema.
fn namespace_of(n: &str) -> Option<String> {
    let (q, _) = n.rsplit_once('.')?;
    let s = q.rsplit('.').next().unwrap_or(q);
    (!DEFAULT_SCHEMAS.contains(&s)).then(|| s.to_string())
}

#[derive(Default)]
struct TableHistory {
    /// The latest table-level change (create / drop) dropped it.
    dropped: bool,
    at: Option<(String, u64)>,
    /// The script and line of that change.
    by: Option<String>,
}

/// A migration or upgrade script: it is written against the schema of its own time.
fn is_migration_path(f: &str, t: &Targets<'_>) -> bool {
    t.migration_files.contains(f) || looks_like_migration(f)
}

/// A path in a conventional migration / upgrade directory.
pub(crate) fn looks_like_migration(f: &str) -> bool {
    f.split('/').any(|seg| {
        matches!(
            seg.to_ascii_lowercase().as_str(),
            "migration" | "migrations" | "migrate" | "upgrade" | "upgrades" | "changes" | "patches"
        ) || seg.starts_with("migration-")
    })
}

/// Test code: a reference to a missing table there is often the point of the test.
pub(crate) fn is_test_path(f: &str) -> bool {
    let lower = f.to_ascii_lowercase();
    lower.split('/').any(|seg| {
        matches!(seg, "test" | "tests" | "testing" | "__tests__") || seg.ends_with(".test")
    }) || {
        // `FooTest.java`, `test_foo.py`, `foo_test.go` — by the file's own name, case kept, so
        // `Contest.java` is not a test.
        let file = f.rsplit('/').next().unwrap_or(f);
        file.ends_with("Test.java")
            || file.ends_with("Tests.java")
            || file.starts_with("test_")
            || file.ends_with("_test.py")
            || file.ends_with("_test.go")
    }
}

/// The relations references resolve against, and what is needed to explain a miss.
struct Targets<'a> {
    all: NameIndex,
    /// Everything except dbt's own declarations, for dbt `source()` tables.
    foreign: NameIndex,
    by_id: HashMap<KirId, &'a KirObject>,
    /// Schema qualifiers any compiled relation has.
    namespaces: BTreeSet<String>,
    foreign_count: usize,
    /// (normalized tail, object) for the closest-name candidates.
    tails: Vec<(String, &'a KirObject)>,
    /// RFC 0210 migration history per table: the latest table-level change, so a table the
    /// scripts alter but no compiled DDL creates is a coverage gap, and one they drop is gone.
    history: HashMap<String, TableHistory>,
    /// Files that are migration scripts: they describe the schema of their own time.
    migration_files: BTreeSet<String>,
    /// `ModelTable`s (RFC 0209): consulted only for a name no schema defines.
    claims: NameIndex,
    routine_count: usize,
    /// Schema / package qualifiers of compiled routines.
    routine_namespaces: BTreeSet<String>,
    routine_tails: Vec<(String, &'a KirObject)>,
    /// Routines a statement creates at run time (`EXECUTE 'CREATE FUNCTION …'`).
    created_routines: BTreeSet<String>,
    /// Routines: a SQL transformation's `FROM f()` names a table function, not a relation.
    routines: NameIndex,
    /// Relations a routine creates when it runs (`CREATE TEMP TABLE`, `EXECUTE 'CREATE VIEW …'`):
    /// no schema file defines them, and they are not broken.
    created: BTreeSet<String>,
}

impl<'a> Targets<'a> {
    fn build(objects: &'a [KirObject]) -> Self {
        let mut t = Targets {
            all: NameIndex::default(),
            foreign: NameIndex::default(),
            by_id: HashMap::new(),
            namespaces: BTreeSet::new(),
            foreign_count: 0,
            tails: Vec::new(),
            history: HashMap::new(),
            migration_files: BTreeSet::new(),
            claims: NameIndex::default(),
            routines: NameIndex::default(),
            routine_count: 0,
            routine_namespaces: BTreeSet::new(),
            routine_tails: Vec::new(),
            created_routines: BTreeSet::new(),
            created: BTreeSet::new(),
        };
        for o in objects {
            match custom_kind(o) {
                Some("Procedure") => {
                    t.routines.add_object(o);
                    t.routine_count += 1;
                    let n = normalize(&o.name);
                    if let Some(ns) = namespace_of(&n) {
                        t.routine_namespaces.insert(ns);
                    }
                    let tail = n.rsplit('.').next().unwrap_or(&n).to_string();
                    t.routine_tails.push((tail, o));
                }
                Some("SchemaChange") => t.add_change(o),
                Some("SchemaChangeSet") => {
                    if let Some(f) = str_prop(o, "source_path") {
                        t.migration_files.insert(f);
                    }
                }
                Some("ProcedureStatement") => {
                    // `sql` for a static statement, `expr` for a dynamic `EXECUTE`'s argument.
                    for key in ["sql", "expr"] {
                        if let Some(text) = o.properties.get(key).and_then(|v| v.as_str()) {
                            t.created.extend(created_relations(text));
                            t.created_routines.extend(created_routines(text));
                        }
                    }
                }
                _ => {}
            }
        }
        for o in objects.iter().filter(|o| is_relation(o)) {
            if custom_kind(o) == Some("ModelTable") {
                // RFC 0209: the application model's claim about a table, never a second definition.
                t.claims.add_object(o);
            } else {
                t.all.add_object(o);
            }
            if !is_dbt(o) {
                if custom_kind(o) != Some("ModelTable") {
                    t.foreign.add_object(o);
                }
                t.foreign_count += 1;
            }
            t.by_id.insert(o.id, o);
            let n = normalize(&o.name);
            if let Some(ns) = namespace_of(&n) {
                t.namespaces.insert(ns);
            }
            // A live object's schema is a property, not part of its name.
            if let Some(owner) = str_prop(o, "owner").or_else(|| str_prop(o, "schema")) {
                t.namespaces.insert(owner.to_lowercase());
            }
            let tail = n.rsplit('.').next().unwrap_or(&n).to_string();
            t.tails.push((tail, o));
        }
        t.tails
            .sort_by(|a, b| a.0.cmp(&b.0).then(a.1.id.0.cmp(&b.1.id.0)));
        t.routine_tails
            .sort_by(|a, b| a.0.cmp(&b.0).then(a.1.id.0.cmp(&b.1.id.0)));
        t
    }

    fn add_change(&mut self, o: &KirObject) {
        if let Some(f) = str_prop(o, "source_path") {
            self.migration_files.insert(f);
        }
        let Some(table) = str_prop(o, "table") else {
            return;
        };
        let op = str_prop(o, "operation").unwrap_or_default();
        let at = (
            str_prop(o, "applied_at").unwrap_or_default(),
            o.properties
                .get("ordinal")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        );
        let dropped = matches!(op.as_str(), "drop_table" | "drop_view");
        let table_level = dropped || matches!(op.as_str(), "create_table" | "create_view");
        let e = self.history.entry(normalize(&table)).or_default();
        if table_level && e.at.as_ref().is_none_or(|prev| at >= *prev) {
            e.dropped = dropped;
            e.at = Some(at);
            e.by = str_prop(o, "source_path").map(|f| match u32_prop(o.properties.get("line")) {
                Some(l) => format!("{f}:{l}"),
                None => f,
            });
        }
    }

    fn candidate(&self, id: KirId) -> Option<Candidate> {
        self.by_id.get(&id).map(|o| to_candidate(o))
    }

    /// Up to three relations whose name is within a small edit distance of `n`'s last part.
    fn closest(&self, n: &str) -> Vec<Candidate> {
        closest_in(&self.tails, n)
    }
}

/// Up to three objects whose name's last part is within a small edit distance of `n`'s.
fn closest_in(tails: &[(String, &KirObject)], n: &str) -> Vec<Candidate> {
    let tail = n.rsplit('.').next().unwrap_or(n);
    let limit = (tail.chars().count() / 4).clamp(1, 3);
    let mut scored: Vec<(usize, &str, &KirObject)> = tails
        .iter()
        .filter(|(t, _)| t != tail)
        .filter_map(|(t, o)| {
            let d = levenshtein(tail, t, limit)?;
            Some((d, t.as_str(), *o))
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(b.1))
            .then(a.2.id.0.cmp(&b.2.id.0))
    });
    let mut seen = BTreeSet::new();
    scored
        .into_iter()
        .filter(|(_, _, o)| seen.insert(o.name.to_lowercase()))
        .take(3)
        .map(|(_, _, o)| to_candidate(o))
        .collect()
}

pub(crate) fn to_candidate(o: &KirObject) -> Candidate {
    Candidate {
        id: o.id,
        kind: o.kind.to_string(),
        name: o.name.clone(),
        file: str_prop(o, "source_path").or_else(|| str_prop(o, "file")),
    }
}

/// Edit distance, or `None` once it exceeds `limit`.
fn levenshtein(a: &str, b: &str, limit: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > limit {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        if cur.iter().min().copied().unwrap_or(0) > limit {
            return None;
        }
        prev = cur;
    }
    let d = prev[b.len()];
    (d <= limit).then_some(d)
}

/// Relations a SQL text creates — `CREATE [OR REPLACE] [GLOBAL|LOCAL] [TEMP|TEMPORARY|UNLOGGED]
/// [MATERIALIZED] TABLE|VIEW [IF NOT EXISTS] <name>` — also inside string literals, which is how
/// a routine creates one with `EXECUTE` / `EXECUTE IMMEDIATE`. Normalized names.
pub fn created_relations(sql: &str) -> Vec<String> {
    let words: Vec<String> = sql
        .split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '"' | '$' | '#')))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if !words[i].eq_ignore_ascii_case("create") {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let skip = [
            "or",
            "replace",
            "global",
            "local",
            "temp",
            "temporary",
            "unlogged",
            "materialized",
        ];
        while j < words.len() && skip.iter().any(|w| words[j].eq_ignore_ascii_case(w)) {
            j += 1;
        }
        let is_relation = j < words.len()
            && (words[j].eq_ignore_ascii_case("table") || words[j].eq_ignore_ascii_case("view"));
        if is_relation {
            j += 1;
            if j + 2 < words.len()
                && words[j].eq_ignore_ascii_case("if")
                && words[j + 1].eq_ignore_ascii_case("not")
                && words[j + 2].eq_ignore_ascii_case("exists")
            {
                j += 3;
            }
            if let Some(name) = words.get(j) {
                out.push(normalize(name));
            }
        }
        i = j.max(i + 1);
    }
    out
}

/// Routines a SQL text creates, also inside string literals (`EXECUTE 'CREATE FUNCTION …'`).
pub fn created_routines(sql: &str) -> Vec<String> {
    let words: Vec<&str> = sql
        .split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '"' | '$')))
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    for (i, w) in words.iter().enumerate() {
        if !w.eq_ignore_ascii_case("create") {
            continue;
        }
        let mut j = i + 1;
        while j < words.len()
            && ["or", "replace"]
                .iter()
                .any(|s| words[j].eq_ignore_ascii_case(s))
        {
            j += 1;
        }
        let routine = words.get(j).is_some_and(|k| {
            ["function", "procedure", "aggregate"]
                .iter()
                .any(|s| k.eq_ignore_ascii_case(s))
        });
        if routine && let Some(name) = words.get(j + 1) {
            out.push(normalize(name));
        }
    }
    out
}

/// The first line of each (file, routine) statement that names each target.
type StatementLines = HashMap<(String, String), BTreeMap<String, u32>>;

fn statement_lines(objects: &[KirObject]) -> StatementLines {
    let mut out: StatementLines = HashMap::new();
    for o in objects {
        if custom_kind(o) != Some("ProcedureStatement") {
            continue;
        }
        let (Some(file), Some(line)) = (
            str_prop(o, "source_path"),
            u32_prop(o.properties.get("line")),
        ) else {
            continue;
        };
        let routine = str_prop(o, "procedure")
            .unwrap_or_else(|| o.name.split('#').next().unwrap_or(&o.name).to_string())
            .to_lowercase();
        let e = out.entry((file, routine)).or_default();
        for key in ["reads", "writes"] {
            for n in strings(o.properties.get(key)) {
                let slot = e.entry(normalize(&n)).or_insert(line);
                *slot = (*slot).min(line);
            }
        }
    }
    out
}

fn collect<'a>(objects: &'a [KirObject], opts: &Options<'_>) -> Vec<Reference<'a>> {
    let lines = statement_lines(objects);
    let mut refs = Vec::new();
    for o in objects {
        match custom_kind(o) {
            Some("Procedure" | "View") => footprint_refs(o, &lines, &mut refs),
            Some("TransformNode") => transform_ref(o, &mut refs),
            Some("JavaMethod") => java_refs(o, opts, &mut refs),
            _ if o.kind == ObjectKind::Table
                && o.properties.get("dbt_kind").and_then(|v| v.as_str()) == Some("source") =>
            {
                refs.push(Reference {
                    source: o,
                    written: o.name.clone(),
                    access: "declare",
                    extractor: "dbt-source",
                    file: str_prop(o, "source_path"),
                    line: None,
                    cross_system: true,
                    uncertain: false,
                    against_foreign: true,
                    expects: Expect::Relation,
                });
            }
            _ => {}
        }
    }
    // A SQL file's transformation steps repeat what its views' and routines' real AST footprints
    // already name; one missing table is one finding.
    let footprint: BTreeSet<(Option<String>, String, &str)> = refs
        .iter()
        .filter(|r| matches!(r.extractor, "procedure-footprint" | "view-footprint"))
        .map(|r| (r.file.clone(), normalize(&r.written), r.access))
        .collect();
    refs.retain(|r| {
        r.extractor != "sql-transform"
            || !footprint.contains(&(r.file.clone(), normalize(&r.written), r.access))
    });
    refs
}

fn footprint_refs<'a>(o: &'a KirObject, lines: &StatementLines, out: &mut Vec<Reference<'a>>) {
    let view = custom_kind(o) == Some("View");
    let file = str_prop(o, "source_path");
    let own_line = u32_prop(o.properties.get("line"));
    let dynamic = o
        .properties
        .get("dynamic_sql_sites")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|n| n > 0);
    let stmt_lines = file
        .as_ref()
        .and_then(|f| lines.get(&(f.clone(), o.name.to_lowercase())));
    for (key, access) in [("reads", "read"), ("writes", "write")] {
        for written in strings(o.properties.get(key)) {
            let line = stmt_lines
                .and_then(|m| m.get(&normalize(&written)).copied())
                .or(own_line);
            out.push(Reference {
                source: o,
                written,
                access,
                extractor: if view {
                    "view-footprint"
                } else {
                    "procedure-footprint"
                },
                file: file.clone(),
                line,
                cross_system: false,
                uncertain: dynamic,
                against_foreign: false,
                expects: Expect::Relation,
            });
        }
    }
    // Phase 2: the routines it calls. Built-ins are in the same list, told apart by dialect.
    let dialect = match str_prop(o, "language").as_deref() {
        Some("plsql") => SqlDialect::Oracle,
        _ if file.as_deref().is_some_and(|f| f.contains("/oracle/")) => SqlDialect::Oracle,
        _ => SqlDialect::Postgres,
    };
    for written in strings(o.properties.get("calls")) {
        out.push(Reference {
            source: o,
            written,
            access: "call",
            extractor: "routine-calls",
            file: file.clone(),
            line: own_line,
            cross_system: false,
            uncertain: dynamic,
            against_foreign: false,
            expects: Expect::Routine { dialect },
        });
    }
}

fn transform_ref<'a>(o: &'a KirObject, out: &mut Vec<Reference<'a>>) {
    let access = match o.properties.get("node_type").and_then(|v| v.as_str()) {
        Some("Source") => "read",
        Some("Sink") => "write",
        _ => return,
    };
    let Some(written) = str_prop(o, "object_name") else {
        return;
    };
    // `lower_to_kir` names a node `<origin>:<index>`; a SQL file's origin is `<path>#<statement>`.
    let file = o
        .name
        .rsplit_once(':')
        .map(|(f, _)| f.split('#').next().unwrap_or(f).to_string())
        .or_else(|| str_prop(o, "source_path"));
    let pentaho = file
        .as_deref()
        .is_some_and(|f| f.ends_with(".ktr") || f.ends_with(".kjb"));
    out.push(Reference {
        source: o,
        written,
        access,
        extractor: if pentaho { "pentaho" } else { "sql-transform" },
        file,
        line: None,
        cross_system: pentaho,
        uncertain: false,
        against_foreign: false,
        expects: Expect::Relation,
    });
}

fn java_refs<'a>(o: &'a KirObject, opts: &Options<'_>, out: &mut Vec<Reference<'a>>) {
    let Some(statements) = o.properties.get("sql").and_then(|v| v.as_array()) else {
        return;
    };
    let located = locate_object(opts, o);
    let file = str_prop(o, "source_path").or_else(|| located.as_ref().map(|l| l.0.clone()));
    for st in statements {
        let line = u32_prop(st.get("line")).or_else(|| located.as_ref().and_then(|l| l.1));
        let partial = st.get("status").and_then(|v| v.as_str()) != Some("parsed");
        for (key, access) in [
            ("reads", "read"),
            ("inserts", "write"),
            ("updates", "write"),
            ("deletes", "write"),
        ] {
            for written in strings(st.get(key)) {
                out.push(Reference {
                    source: o,
                    written,
                    access,
                    extractor: "java-sql",
                    file: file.clone(),
                    line,
                    cross_system: true,
                    uncertain: partial,
                    against_foreign: false,
                    expects: Expect::Relation,
                });
            }
        }
    }
}

/// The fingerprint's input: never ids or line numbers (RFC 0233 §3).
fn fingerprint(kind: DiagnosticKind, r: &Reference<'_>, normalized: &str) -> String {
    // A transformation step is named by its index in the file, which moves when a step is added.
    let name = match custom_kind(r.source) {
        Some("TransformNode") => String::new(),
        Some("JavaMethod") => format!(
            "{}.{}",
            str_prop(r.source, "owner").unwrap_or_default(),
            r.source.name
        ),
        _ => r.source.name.to_lowercase(),
    };
    let text = format!(
        "xref/v1|{}|{}|{}|{}|{}|{}",
        kind.as_str(),
        r.source.kind,
        r.file.as_deref().unwrap_or(""),
        name,
        r.access,
        normalized
    );
    ekos_common::ContentHash::of_str(&text).as_str().to_string()
}

/// RFC 0233 Phase 3 — what one name resolves to, for an agent about to write SQL or code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameCheck {
    pub name: String,
    pub normalized: String,
    /// `resolved`, `ambiguous`, `unresolved` or `built_in`.
    pub status: String,
    pub matches: Vec<Candidate>,
}

/// Resolve `name` against `candidates` (relations and routines; others are ignored) by the rule
/// the check uses. The caller narrows the ledger to objects that may match.
pub fn check_name(candidates: &[KirObject], name: &str) -> NameCheck {
    let normalized = normalize(name);
    let mut index = NameIndex::default();
    let mut by_id = HashMap::new();
    for o in candidates {
        if is_relation(o) || custom_kind(o) == Some("Procedure") {
            index.add_object(o);
            by_id.insert(o.id, o);
        }
    }
    let ids = index.candidates(&normalized);
    let built_in = is_system_relation(&normalized)
        || crate::xref_builtins::is_builtin(&normalized, SqlDialect::Postgres)
        || crate::xref_builtins::is_builtin(&normalized, SqlDialect::Oracle);
    // A routine's overloads, or one relation defined in several files, are one object to a caller.
    let distinct: BTreeSet<(String, String)> = ids
        .iter()
        .filter_map(|id| by_id.get(id))
        .map(|o| (normalize(&o.name), o.kind.to_string()))
        .collect();
    let status = match distinct.len() {
        0 if built_in => "built_in",
        0 => "unresolved",
        1 => "resolved",
        _ => "ambiguous",
    };
    NameCheck {
        name: name.to_string(),
        normalized,
        status: status.to_string(),
        matches: ids
            .iter()
            .filter_map(|id| by_id.get(id))
            .map(|o| to_candidate(o))
            .collect(),
    }
}

/// Run the check over every object of a compiled ledger.
pub fn check(objects: &[KirObject], opts: &Options<'_>) -> Report {
    let targets = Targets::build(objects);
    let mut metrics = Metrics::default();
    let mut found: BTreeMap<String, Diagnostic> = BTreeMap::new();
    // One reference per (source, target, access): a routine reading a table in ten statements
    // is one reference.
    let mut seen: BTreeSet<(uuid::Uuid, String, &str)> = BTreeSet::new();

    // Every relation name some reference uses, resolved or not: a table named this way is used.
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for r in collect(objects, opts) {
        let normalized = normalize(&r.written);
        if !seen.insert((r.source.id.0, normalized.clone(), r.access)) {
            continue;
        }
        if r.expects == Expect::Relation {
            referenced.insert(tail_of(&normalized).to_string());
        }
        if let (Some(live), Some(file)) = (opts.live_files, r.file.as_ref())
            && !is_live(r.source)
            && !live.contains(file)
        {
            metrics.stale_sources += 1;
            continue;
        }
        if is_dynamic(&normalized) {
            metrics.dynamic += 1;
            continue;
        }
        let builtin = match r.expects {
            Expect::Relation => is_system_relation(&normalized),
            Expect::Routine { dialect } => crate::xref_builtins::is_builtin(&normalized, dialect),
            Expect::Module => false,
        };
        if builtin {
            metrics.system += 1;
            continue;
        }
        let (outcome, diagnostic) = match r.expects {
            Expect::Relation => judge(&r, &normalized, &targets, opts),
            Expect::Routine { .. } => judge_call(&r, &normalized, &targets),
            // `collect` gathers no imports; `xref_imports` judges them below.
            Expect::Module => continue,
        };
        metrics.all.add(outcome);
        if r.cross_system {
            metrics.cross_system.add(outcome);
        }
        metrics
            .by_extractor
            .entry(r.extractor.to_string())
            .or_default()
            .add(outcome);
        if let Some(mut d) = diagnostic {
            if let Some(s) = opts.severity.get(&d.kind) {
                d.severity = *s;
            }
            found.entry(d.fingerprint.clone()).or_insert(d);
        }
    }

    let imports = crate::xref_imports::check(objects, opts);
    metrics.external += imports.external;
    for res in imports.results {
        metrics.all.add(res.outcome);
        metrics
            .by_extractor
            .entry(res.reference.extractor.to_string())
            .or_default()
            .add(res.outcome);
        if let Some(f) = res.finding {
            let mut d = diagnostic(&res.reference, &res.normalized, f);
            if let Some(s) = opts.severity.get(&d.kind) {
                d.severity = *s;
            }
            found.entry(d.fingerprint.clone()).or_insert(d);
        }
    }

    let graph = crate::xref_graph::check(objects, opts, &referenced);
    metrics.cycles = graph.cycles;
    metrics.orphans = graph.orphans;
    for mut d in graph.diagnostics {
        if let Some(s) = opts.severity.get(&d.kind) {
            d.severity = *s;
        }
        found.entry(d.fingerprint.clone()).or_insert(d);
    }

    // Without relationships there is no call graph to traverse.
    let reach = if opts.relationships.is_empty() {
        crate::xref_reach::Reach::default()
    } else {
        crate::xref_reach::check(objects, opts)
    };
    metrics.reachability = reach.families;
    for mut d in reach.diagnostics {
        if let Some(s) = opts.severity.get(&d.kind) {
            d.severity = *s;
        }
        found.entry(d.fingerprint.clone()).or_insert(d);
    }

    metrics.resolution_rate = metrics.all.resolution_rate();
    metrics.cross_system_resolution_rate = metrics.cross_system.resolution_rate();
    let mut diagnostics: Vec<Diagnostic> = found.into_values().collect();
    diagnostics.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.source.file.cmp(&b.source.file))
            .then_with(|| a.source.line.cmp(&b.source.line))
            .then_with(|| a.target_spec.normalized.cmp(&b.target_spec.normalized))
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
    });
    Report {
        version: XREF_VERSION,
        diagnostics,
        metrics,
    }
}

/// Every candidate has the same full name and kind: one relation defined several times — an
/// engine's twin script (`db/oracle/views`, `db/postgresql/views`), a migration re-creating it —
/// not two relations a name cannot choose between.
fn one_relation_redefined(normalized: &str, index: &NameIndex, t: &Targets<'_>) -> bool {
    let shapes: BTreeSet<(String, String)> = index
        .candidates(normalized)
        .into_iter()
        .filter_map(|id| t.by_id.get(&id))
        .map(|o| (normalize(&o.name), o.kind.to_string()))
        .collect();
    shapes.len() == 1
}

/// Exactly one of an ambiguous name's candidates is defined in the reference's own file.
fn defined_in_own_file(
    r: &Reference<'_>,
    normalized: &str,
    index: &NameIndex,
    t: &Targets<'_>,
    opts: &Options<'_>,
) -> bool {
    let Some(file) = &r.file else {
        return false;
    };
    let same = index
        .candidates(normalized)
        .into_iter()
        .filter_map(|id| t.by_id.get(&id))
        .filter(|o| {
            let f = str_prop(o, "source_path").or_else(|| locate_object(opts, o).map(|(f, _)| f));
            f.as_deref() == Some(file.as_str())
        })
        .count();
    same == 1
}

/// Resolve one reference; a miss becomes a diagnostic with RFC 0233 §5's severity.
fn judge(
    r: &Reference<'_>,
    normalized: &str,
    t: &Targets<'_>,
    opts: &Options<'_>,
) -> (Outcome, Option<Diagnostic>) {
    let index = if r.against_foreign {
        &t.foreign
    } else {
        &t.all
    };
    let (kind, outcome) = match index.resolve(normalized) {
        Ok(_) => return (Outcome::Resolved, None),
        // Two files define it (a migration and the module that reloads it); the reference's own
        // file defining one of them is the definition it means.
        Err(true)
            if one_relation_redefined(normalized, index, t)
                || defined_in_own_file(r, normalized, index, t, opts) =>
        {
            return (Outcome::Resolved, None);
        }
        Err(true) => (DiagnosticKind::AmbiguousReference, Outcome::Ambiguous),
        // Known only from the application model's generated classes.
        Err(false) if !t.claims.candidates(normalized).is_empty() => {
            return (Outcome::Resolved, None);
        }
        Err(false) if !r.against_foreign && t.created.contains(normalized) => {
            return (Outcome::Resolved, None);
        }
        // A SQL transformation's `FROM f()` source is a table function (perhaps overloaded).
        Err(false)
            if r.extractor == "sql-transform" && !t.routines.candidates(normalized).is_empty() =>
        {
            return (Outcome::Resolved, None);
        }
        Err(false) => (DiagnosticKind::UnresolvedReference, Outcome::Unresolved),
    };
    let namespace = namespace_of(normalized);
    let nothing_compiled = if r.against_foreign {
        t.foreign_count == 0
    } else {
        t.by_id.is_empty()
    };
    let unknown_namespace = namespace
        .as_ref()
        .is_some_and(|ns| !t.namespaces.contains(ns));
    let mut coverage_gap = false;
    let (severity, confidence, reason, candidates) = if kind == DiagnosticKind::AmbiguousReference {
        let c: Vec<Candidate> = index
            .candidates(normalized)
            .into_iter()
            .filter_map(|id| t.candidate(id))
            .collect();
        (
            Severity::Warning,
            0.6,
            format!("{} compiled relations match this name", c.len()),
            c,
        )
    } else if nothing_compiled {
        coverage_gap = true;
        let what = if r.against_foreign {
            "no database schema outside dbt is compiled"
        } else {
            "no relations are compiled in this workspace"
        };
        (Severity::Info, 0.3, what.to_string(), Vec::new())
    } else if unknown_namespace {
        coverage_gap = true;
        (
            Severity::Info,
            0.3,
            format!(
                "no compiled relation is in schema `{}`",
                namespace.as_deref().unwrap_or_default()
            ),
            Vec::new(),
        )
    } else if r.file.as_ref().is_some_and(|f| is_migration_path(f, t)) {
        (
            Severity::Info,
            0.3,
            "a migration script describes the schema of its own time".to_string(),
            t.closest(normalized),
        )
    } else if let Some(h) = t.history.get(normalized).filter(|h| h.dropped) {
        (
            Severity::Error,
            0.9,
            format!(
                "dropped by {}",
                h.by.as_deref().unwrap_or("a migration script")
            ),
            t.closest(normalized),
        )
    } else if t.history.contains_key(normalized) {
        coverage_gap = true;
        (
            Severity::Info,
            0.3,
            "the migration scripts change this table, but no compiled DDL or model defines it"
                .to_string(),
            Vec::new(),
        )
    } else if r.file.as_deref().is_some_and(is_test_path) {
        (
            Severity::Info,
            0.3,
            "test code; a missing table may be what the test checks".to_string(),
            t.closest(normalized),
        )
    } else if is_work_table(normalized) {
        (
            Severity::Info,
            0.3,
            "named like a work table the code creates for itself".to_string(),
            t.closest(normalized),
        )
    } else if r.uncertain {
        let why = if r.extractor == "java-sql" {
            "the SQL text was not wholly literal"
        } else {
            "the routine also runs dynamic SQL"
        };
        (
            Severity::Warning,
            0.6,
            format!("no compiled relation has this name; {why}"),
            t.closest(normalized),
        )
    } else {
        (
            Severity::Error,
            0.9,
            "no compiled relation has this name".to_string(),
            t.closest(normalized),
        )
    };
    let found = Finding {
        kind,
        severity,
        confidence,
        reason,
        candidates,
        coverage_gap,
        expected: "relation",
        namespace,
    };
    (outcome, Some(diagnostic(r, normalized, found)))
}

/// What a check decided about one reference, before it becomes a [`Diagnostic`].
pub(crate) struct Finding {
    pub(crate) kind: DiagnosticKind,
    pub(crate) severity: Severity,
    pub(crate) confidence: f64,
    pub(crate) reason: String,
    pub(crate) candidates: Vec<Candidate>,
    pub(crate) coverage_gap: bool,
    pub(crate) expected: &'static str,
    pub(crate) namespace: Option<String>,
}

pub(crate) fn diagnostic(r: &Reference<'_>, normalized: &str, f: Finding) -> Diagnostic {
    Diagnostic {
        kind: f.kind,
        severity: f.severity,
        confidence: f.confidence,
        source: SourceRef {
            object_id: r.source.id,
            object_kind: r.source.kind.to_string(),
            object_name: r.source.name.clone(),
            file: r.file.clone(),
            line: r.line,
            extractor: r.extractor.to_string(),
            access: r.access.to_string(),
        },
        target_spec: TargetSpec {
            written: r.written.clone(),
            normalized: normalized.to_string(),
            expected: f.expected.to_string(),
            namespace: f.namespace,
        },
        candidates: f.candidates,
        evidence: r.source.evidence.clone(),
        coverage_gap: f.coverage_gap,
        cross_system: r.cross_system,
        reason: f.reason,
        fingerprint: fingerprint(f.kind, r, normalized),
    }
}

/// Phase 2: a routine call. Overloads are one routine; built-ins never reach here.
fn judge_call(
    r: &Reference<'_>,
    normalized: &str,
    t: &Targets<'_>,
) -> (Outcome, Option<Diagnostic>) {
    if !t.routines.candidates(normalized).is_empty() || t.created_routines.contains(normalized) {
        return (Outcome::Resolved, None);
    }
    let namespace = namespace_of(normalized);
    let mut coverage_gap = false;
    let (severity, confidence, reason) = if t.routine_count == 0 {
        coverage_gap = true;
        (
            Severity::Info,
            0.3,
            "no routines are compiled in this workspace".to_string(),
        )
    } else if namespace
        .as_ref()
        .is_some_and(|ns| !t.routine_namespaces.contains(ns))
    {
        coverage_gap = true;
        (
            Severity::Info,
            0.3,
            format!(
                "no compiled routine is in schema or package `{}`",
                namespace.as_deref().unwrap_or_default()
            ),
        )
    } else if r.file.as_deref().is_some_and(|f| is_migration_path(f, t)) {
        (
            Severity::Info,
            0.3,
            "a migration script describes the schema of its own time".to_string(),
        )
    } else if r.uncertain {
        (
            Severity::Warning,
            0.5,
            "no compiled routine or built-in has this name; the routine also runs dynamic SQL"
                .to_string(),
        )
    } else if matches!(
        r.expects,
        Expect::Routine {
            dialect: SqlDialect::Oracle
        }
    ) {
        // `CREATE TYPE` constructors and synonyms are callable and not compiled.
        (
            Severity::Warning,
            0.5,
            "no compiled routine or built-in has this name; Oracle object types and synonyms are \
             not compiled, so it may be one"
                .to_string(),
        )
    } else {
        (
            Severity::Error,
            0.8,
            "no compiled routine or built-in function has this name".to_string(),
        )
    };
    let found = Finding {
        kind: DiagnosticKind::UnresolvedReference,
        severity,
        confidence,
        reason,
        candidates: closest_in(&t.routine_tails, normalized),
        coverage_gap,
        expected: "routine",
        namespace,
    };
    (Outcome::Unresolved, Some(diagnostic(r, normalized, found)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn table(name: &str) -> KirObject {
        KirObject::new(name, ObjectKind::Table)
    }

    fn routine(name: &str, file: &str, reads: &[&str], writes: &[&str]) -> KirObject {
        KirObject::new(name, ObjectKind::Custom("Procedure".into()))
            .with_property("source_path", json!(file))
            .with_property("line", json!(10))
            .with_property("reads", json!(reads))
            .with_property("writes", json!(writes))
    }

    fn statement(routine: &str, n: u32, file: &str, line: u32, reads: &[&str]) -> KirObject {
        KirObject::new(
            format!("{routine}#{n}"),
            ObjectKind::Custom("ProcedureStatement".into()),
        )
        .with_property("source_path", json!(file))
        .with_property("line", json!(line))
        .with_property("reads", json!(reads))
    }

    fn java(name: &str, sql: serde_json::Value) -> KirObject {
        KirObject::new(name, ObjectKind::Custom("JavaMethod".into()))
            .with_property("owner", json!("org.compiere.process.Foo"))
            .with_property("sql", sql)
    }

    fn run(objects: &[KirObject]) -> Report {
        check(objects, &Options::default())
    }

    #[test]
    fn a_resolved_name_is_counted_and_not_reported() {
        let r = run(&[
            table("account"),
            routine("post", "sql/a.sql", &["account"], &[]),
        ]);
        assert!(r.diagnostics.is_empty());
        assert_eq!(r.metrics.all.resolved, 1);
        assert_eq!(r.metrics.resolution_rate, Some(1.0));
    }

    #[test]
    fn a_missing_table_is_an_error_with_the_statement_line_and_close_names() {
        let r = run(&[
            table("account"),
            table("acc_trans"),
            routine("post", "sql/a.sql", &["acc_tran"], &[]),
            statement("post", 3, "sql/a.sql", 42, &["acc_tran"]),
        ]);
        let [d] = r.diagnostics.as_slice() else {
            panic!("{:?}", r.diagnostics)
        };
        assert_eq!(d.kind, DiagnosticKind::UnresolvedReference);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.source.line, Some(42));
        assert_eq!(d.source.file.as_deref(), Some("sql/a.sql"));
        assert_eq!(d.candidates[0].name, "acc_trans");
        assert!(!d.cross_system);
    }

    #[test]
    fn an_ambiguous_name_lists_every_candidate() {
        let r = run(&[
            table("a.customers"),
            table("b.customers"),
            routine("r", "x.sql", &["customers"], &[]),
        ]);
        let [d] = r.diagnostics.as_slice() else {
            panic!()
        };
        assert_eq!(d.kind, DiagnosticKind::AmbiguousReference);
        assert_eq!(d.candidates.len(), 2);
        assert_eq!(r.metrics.all.ambiguous, 1);
    }

    #[test]
    fn java_sql_is_cross_system_and_a_partial_statement_is_only_a_warning() {
        let r = run(&[
            table("C_Invoice"),
            java(
                "doIt",
                json!([
                    {"line": 5, "status": "parsed", "reads": ["C_Invoice", "C_Missing"]},
                    {"line": 9, "status": "partial", "updates": ["C_Other"]},
                    {"line": 12, "status": "partial", "reads": ["ekos_expr"]}
                ]),
            ),
        ]);
        let by: BTreeMap<&str, &Diagnostic> = r
            .diagnostics
            .iter()
            .map(|d| (d.target_spec.written.as_str(), d))
            .collect();
        assert_eq!(by["C_Missing"].severity, Severity::Error);
        assert_eq!(by["C_Missing"].source.line, Some(5));
        assert!(by["C_Missing"].cross_system);
        assert_eq!(by["C_Other"].severity, Severity::Warning);
        assert_eq!(r.metrics.dynamic, 1);
        assert_eq!(r.metrics.cross_system.resolved, 1);
    }

    #[test]
    fn system_catalogs_unknown_schemas_and_work_tables_are_not_errors() {
        let r = run(&[
            table("account"),
            routine(
                "r",
                "x.sql",
                &[
                    "pg_catalog.pg_class",
                    "dual",
                    "all_tables",
                    "generate_series",
                    "other_db.ledger",
                    "tmp_lines",
                ],
                &[],
            ),
        ]);
        assert_eq!(r.metrics.system, 4);
        for d in &r.diagnostics {
            assert_eq!(d.severity, Severity::Info, "{d:?}");
        }
        let gap = r
            .diagnostics
            .iter()
            .find(|d| d.target_spec.written == "other_db.ledger")
            .unwrap();
        assert!(gap.coverage_gap);
        // `user_` alone is not the Oracle dictionary.
        assert!(!is_system_relation("user_preferences"));
    }

    #[test]
    fn a_workspace_without_any_schema_reports_only_coverage_gaps() {
        let r = run(&[java(
            "m",
            json!([{"line": 1, "status": "parsed", "reads": ["C_Order"]}]),
        )]);
        assert!(r.diagnostics.iter().all(|d| d.coverage_gap));
    }

    #[test]
    fn a_dbt_source_resolves_only_against_other_systems() {
        let source = table("raw_orders").with_property("dbt_kind", json!("source"));
        let r = run(&[source.clone(), table("customers")]);
        let [d] = r.diagnostics.as_slice() else {
            panic!()
        };
        assert_eq!(d.source.extractor, "dbt-source");
        assert_eq!(d.severity, Severity::Error);
        let r = run(&[source, table("raw_orders")]);
        assert!(r.diagnostics.is_empty());
    }

    #[test]
    fn a_sql_transform_step_does_not_repeat_its_files_view() {
        let view = KirObject::new("v", ObjectKind::Custom("View".into()))
            .with_property("source_path", json!("v.sql"))
            .with_property("reads", json!(["gone"]));
        let step = KirObject::new("v.sql#2:0", ObjectKind::Custom("TransformNode".into()))
            .with_property("node_type", json!("Source"))
            .with_property("object_name", json!("gone"));
        let r = run(&[table("t"), view, step]);
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.diagnostics[0].source.extractor, "view-footprint");
    }

    #[test]
    fn a_pentaho_step_is_cross_system() {
        let step = KirObject::new("etl/load.ktr:3", ObjectKind::Custom("TransformNode".into()))
            .with_property("node_type", json!("Sink"))
            .with_property("object_name", json!("dim_x"));
        let r = run(&[table("t"), step]);
        let [d] = r.diagnostics.as_slice() else {
            panic!()
        };
        assert_eq!(d.source.extractor, "pentaho");
        assert_eq!(d.source.access, "write");
        assert_eq!(d.source.file.as_deref(), Some("etl/load.ktr"));
        assert!(d.cross_system);
    }

    #[test]
    fn a_source_whose_file_is_gone_is_skipped() {
        let live: BTreeSet<String> = ["kept.sql".to_string()].into();
        let objects = [
            table("t"),
            routine("old", "removed.sql", &["gone"], &[]),
            routine("new", "kept.sql", &["gone"], &[]),
        ];
        let r = check(
            &objects,
            &Options {
                live_files: Some(&live),
                ..Default::default()
            },
        );
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.metrics.stale_sources, 1);
    }

    #[test]
    fn fingerprints_ignore_ids_lines_and_input_order() {
        let objects = vec![
            table("account"),
            routine("post", "sql/a.sql", &["missing"], &["also_missing"]),
            statement("post", 1, "sql/a.sql", 7, &["missing"]),
        ];
        let a = run(&objects);
        // A recompile gives new ids, an edit above moves lines; the input arrives reordered.
        let mut moved: Vec<KirObject> = objects
            .iter()
            .cloned()
            .map(|mut o| {
                o.id = KirId::new();
                if o.properties.contains_key("line") {
                    o.properties.insert("line".into(), json!(99));
                }
                o
            })
            .collect();
        moved.reverse();
        let b = run(&moved);
        let fp = |r: &Report| -> BTreeSet<String> {
            r.diagnostics
                .iter()
                .map(|d| d.fingerprint.clone())
                .collect()
        };
        assert_eq!(fp(&a), fp(&b));
        assert_eq!(fp(&a).len(), 2);
        assert_eq!(
            serde_json::to_string(&run(&objects)).unwrap(),
            serde_json::to_string(&a).unwrap()
        );
    }

    #[test]
    fn relations_a_routine_creates_are_not_missing() {
        assert_eq!(
            created_relations("CREATE TEMPORARY TABLE bulk_in (id int)"),
            ["bulk_in"]
        );
        assert_eq!(
            created_relations("EXECUTE 'CREATE OR REPLACE VIEW file_links AS SELECT 1'"),
            ["file_links"]
        );
        assert_eq!(
            created_relations("create table if not exists \"Grants\" as select 1"),
            ["grants"]
        );
        assert!(created_relations("CREATE INDEX i ON t (c)").is_empty());
        let stmt = KirObject::new("r#1", ObjectKind::Custom("ProcedureStatement".into()))
            .with_property("procedure", json!("r"))
            .with_property("source_path", json!("x.sql"))
            .with_property("line", json!(5))
            .with_property("sql", json!("CREATE TEMP TABLE work_in (id int)"))
            .with_property("reads", json!(["work_in"]));
        let r = run(&[table("t"), routine("r", "x.sql", &["work_in"], &[]), stmt]);
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        assert_eq!(r.metrics.all.resolved, 1);
    }

    #[test]
    fn a_table_function_in_a_sql_transformation_is_a_routine() {
        let step = KirObject::new("a.sql#3:0", ObjectKind::Custom("TransformNode".into()))
            .with_property("node_type", json!("Source"))
            .with_property("object_name", json!("report__begin"));
        // Defined twice (an overload, or a module and a migration): still a routine.
        let f = KirObject::new("report__begin", ObjectKind::Custom("Procedure".into()));
        let g = KirObject::new("report__begin", ObjectKind::Custom("Procedure".into()));
        let r = run(&[table("t"), f, g, step]);
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    }

    #[test]
    fn an_ambiguous_name_defined_in_the_references_own_file_resolves() {
        // A table and a view of one name: different relations, so only the file can choose.
        let view = KirObject::new("cash_impact", ObjectKind::Custom("View".into()))
            .with_property("source_path", json!("modules/fin.sql"));
        let other = KirObject::new("cash_impact", ObjectKind::Table)
            .with_property("source_path", json!("changes/views.sql"));
        let objects = [
            view,
            other,
            routine("pnl", "modules/fin.sql", &["cash_impact"], &[]),
            routine("other", "modules/other.sql", &["cash_impact"], &[]),
        ];
        let r = run(&objects);
        let [d] = r.diagnostics.as_slice() else {
            panic!("{:?}", r.diagnostics)
        };
        assert_eq!(d.source.object_name, "other");
        assert_eq!(d.kind, DiagnosticKind::AmbiguousReference);
    }

    #[test]
    fn one_relation_defined_twice_or_claimed_by_the_model_is_not_ambiguous() {
        let v = |file: &str| {
            KirObject::new("rv_order", ObjectKind::Custom("View".into()))
                .with_property("source_path", json!(file))
        };
        let model = KirObject::new("C_Order", ObjectKind::Custom("ModelTable".into()));
        let only_model = KirObject::new("C_Trl", ObjectKind::Custom("ModelTable".into()));
        let r = run(&[
            v("db/oracle/views/rv_order.sql"),
            v("db/postgresql/views/rv_order.sql"),
            table("c_order"),
            model,
            only_model,
            routine("r", "x.sql", &["rv_order", "c_order", "c_trl"], &[]),
        ]);
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        assert_eq!(r.metrics.all.resolved, 3);
    }

    fn change(file: &str, line: u32, op: &str, table: &str, at: &str) -> KirObject {
        KirObject::new(
            format!("{op} {table}"),
            ObjectKind::Custom("SchemaChange".into()),
        )
        .with_property("source_path", json!(file))
        .with_property("line", json!(line))
        .with_property("operation", json!(op))
        .with_property("table", json!(table))
        .with_property("applied_at", json!(at))
        .with_property("ordinal", json!(0))
    }

    #[test]
    fn migration_history_explains_a_miss() {
        let r = run(&[
            table("t"),
            change("migration/a.sql", 3, "add_column", "AD_Field_Trl", "2020"),
            change("migration/b.sql", 9, "create_table", "W_Click", "2019"),
            change("migration/c.sql", 4, "drop_table", "W_Click", "2021"),
            routine("r", "src/r.sql", &["ad_field_trl", "w_click"], &[]),
            routine("old", "migration/b.sql", &["w_click", "nowhere"], &[]),
        ]);
        let by = |src: &str, tgt: &str| {
            r.diagnostics
                .iter()
                .find(|d| d.source.object_name == src && d.target_spec.normalized == tgt)
                .unwrap()
                .clone()
        };
        let trl = by("r", "ad_field_trl");
        assert_eq!(trl.severity, Severity::Info);
        assert!(trl.coverage_gap);
        let gone = by("r", "w_click");
        assert_eq!(gone.severity, Severity::Error);
        assert_eq!(gone.reason, "dropped by migration/c.sql:4");
        // A migration script's own references are history, whatever they name.
        assert_eq!(by("old", "nowhere").severity, Severity::Info);
    }

    #[test]
    fn test_code_is_not_an_error() {
        let m = java(
            "testIt",
            json!([{"line": 1, "status": "parsed", "reads": ["inexistent_table"]}]),
        )
        .with_property("source_path", json!("org.idempiere.test/src/DBTest.java"));
        let r = run(&[table("t"), m]);
        assert_eq!(r.diagnostics[0].severity, Severity::Info);
        assert!(is_test_path("a/src/test/java/X.java"));
        assert!(!is_test_path("org/compiere/process/Contest.java"));
    }

    #[test]
    fn a_single_name_resolves_by_the_checks_rule() {
        let objects = [
            table("public.orders"),
            table("a.customers"),
            table("b.customers"),
            KirObject::new("post", ObjectKind::Custom("Procedure".into())),
            KirObject::new("post", ObjectKind::Custom("Procedure".into())),
        ];
        assert_eq!(check_name(&objects, "ORDERS").status, "resolved");
        assert_eq!(check_name(&objects, "customers").status, "ambiguous");
        assert_eq!(check_name(&objects, "post").status, "resolved");
        assert_eq!(check_name(&objects, "coalesce").status, "built_in");
        assert_eq!(check_name(&objects, "nope").status, "unresolved");
    }

    #[test]
    fn severity_overrides_apply_per_kind() {
        let objects = [table("t"), routine("r", "x.sql", &["gone"], &[])];
        let r = check(
            &objects,
            &Options {
                severity: [(DiagnosticKind::UnresolvedReference, Severity::Warning)].into(),
                ..Default::default()
            },
        );
        assert_eq!(r.diagnostics[0].severity, Severity::Warning);
    }

    #[test]
    fn names_are_normalized_before_comparison() {
        assert_eq!(normalize("\"Public\".\"C_Invoice\""), "public.c_invoice");
        let r = run(&[
            table("c_invoice"),
            routine("r", "x.sql", &["\"C_INVOICE\""], &[]),
        ]);
        assert!(r.diagnostics.is_empty());
    }
}
