//! RFC 0233 Phase 3 — reachability: which symbols no entry point reaches.
//!
//! One engine, language specifics through the [`ReachabilityRules`] contract: a family names its
//! symbol kinds, says what counts as an entry point and which constructs make a reference
//! unprovable, or declares itself unsupported with the reason. The traversal follows `Calls`
//! edges across every family. A symbol nothing reaches is `UnreachableSymbol`; when the family's
//! dynamic rule fires for it, `DynamicReferenceUnknown` instead — dead-code conclusions are only
//! drawn where reachability can be proven, and the dynamic share says how far that is.
//!
//! What the recovered graph supports today (2026-10-30): **SQL routines** (`Calls` from routines,
//! views and triggers, RFC 0163). The other analyzers record no call graph across files, or no
//! visibility and entry points, so their families are declared unsupported with the reason.

use crate::xref::{
    Diagnostic, DiagnosticKind, Options, Severity, SourceRef, TargetSpec, custom_kind,
    locate_object, looks_like_migration, normalize, str_prop, tail_of,
};
use ekos_kir::{KirId, KirObject, RelationshipKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use uuid::Uuid;

/// Whether a family's reachability can be decided from what its analyzer records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Supported,
    /// Why not, in words.
    Unsupported(&'static str),
}

/// What the rules see: the whole estate, read once.
pub struct Estate<'a> {
    pub objects: &'a [KirObject],
    /// Symbol kinds present, by family — e.g. Perl code exists.
    pub families_present: BTreeSet<&'static str>,
    /// Lower-cased words of all SQL text Java methods build (RFC 0209 §7).
    pub java_sql_words: BTreeSet<String>,
    /// Some Java SQL statement had a part that is not a literal.
    pub java_sql_partial: bool,
    /// `[xref] entry-points`: globs over symbol names.
    pub configured_roots: Vec<String>,
    /// Objects that are the target of a `Calls` edge from a `Trigger`.
    pub trigger_targets: BTreeSet<Uuid>,
    /// The functions triggers name (`Trigger.function`), normalized — also when no edge links them.
    pub trigger_functions: BTreeSet<String>,
}

/// The contract every language family declares (RFC 0233 §Reachability).
pub trait ReachabilityRules: Sync {
    fn family(&self) -> &'static str;
    /// The object kinds that are this family's symbols.
    fn symbol_kinds(&self) -> &'static [&'static str];
    fn support(&self) -> Support;
    /// `Some(why)` when `o` is a root.
    fn entry_point(&self, o: &KirObject, estate: &Estate<'_>) -> Option<String>;
    /// `Some(why)` when an unreached `o` may still be called in a way the graph cannot show.
    fn dynamic(&self, o: &KirObject, estate: &Estate<'_>) -> Option<String>;
    /// Symbols left out of the check entirely (migration scripts run once and are done).
    fn excluded(&self, _o: &KirObject) -> bool {
        false
    }
}

/// Families whose code may call SQL routines by name in text EKOS does not read SQL from.
const NAME_CALLERS: &[&str] = &["perl", "python", "javascript", "elixir", "rust"];

struct Sql;

impl ReachabilityRules for Sql {
    fn family(&self) -> &'static str {
        "sql"
    }
    fn symbol_kinds(&self) -> &'static [&'static str] {
        &["Procedure"]
    }
    fn support(&self) -> Support {
        Support::Supported
    }
    fn excluded(&self, o: &KirObject) -> bool {
        crate::procedure_lineage::is_live(o)
            || str_prop(o, "source_path").is_some_and(|f| looks_like_migration(&f))
    }
    fn entry_point(&self, o: &KirObject, estate: &Estate<'_>) -> Option<String> {
        let name = normalize(&o.name);
        if estate.trigger_targets.contains(&o.id.0)
            || estate.trigger_functions.contains(tail_of(&name))
        {
            return Some("a trigger executes it".into());
        }
        if estate.java_sql_words.contains(tail_of(&name)) {
            return Some("Java code's SQL names it".into());
        }
        estate
            .configured_roots
            .iter()
            .find(|g| glob(&g.to_lowercase(), &name))
            .map(|g| format!("`[xref] entry-points` lists `{g}`"))
    }
    fn dynamic(&self, _o: &KirObject, estate: &Estate<'_>) -> Option<String> {
        if estate.java_sql_partial {
            return Some("Java code builds SQL at run time, so it may name this routine".into());
        }
        if let Some(f) = NAME_CALLERS
            .iter()
            .find(|f| estate.families_present.contains(*f))
        {
            return Some(format!(
                "{f} code in this workspace may call routines by name, and EKOS does not read SQL from it"
            ));
        }
        None
    }
}

/// A family whose analyzer does not record what reachability needs.
struct Unsupported {
    family: &'static str,
    kinds: &'static [&'static str],
    why: &'static str,
}

impl ReachabilityRules for Unsupported {
    fn family(&self) -> &'static str {
        self.family
    }
    fn symbol_kinds(&self) -> &'static [&'static str] {
        self.kinds
    }
    fn support(&self) -> Support {
        Support::Unsupported(self.why)
    }
    fn entry_point(&self, _: &KirObject, _: &Estate<'_>) -> Option<String> {
        None
    }
    fn dynamic(&self, _: &KirObject, _: &Estate<'_>) -> Option<String> {
        None
    }
}

/// Every family, supported or not. A new symbol kind must be claimed here (a test checks the
/// custom-kind registry).
pub static RULES: &[&dyn ReachabilityRules] = &[
    &Sql,
    &Unsupported {
        family: "rust",
        kinds: &["RustSymbol"],
        why: "calls are recovered only within one file, and neither visibility nor `main` / \
              `#[test]` entry points are recorded on symbols",
    },
    &Unsupported {
        family: "python",
        kinds: &["PythonSymbol"],
        why: "no call graph is recovered, and neither `__main__` nor package entry points are read",
    },
    &Unsupported {
        family: "javascript",
        kinds: &["JsSymbol"],
        why: "no call graph is recovered, and `package.json` `main` / `exports` are not read",
    },
    &Unsupported {
        family: "java",
        kinds: &["JavaType", "JavaMethod"],
        why: "no call graph is recovered; iDempiere-style dictionary entry points (`AD_Process`, \
              callouts) need the dictionary rows",
    },
    &Unsupported {
        family: "perl",
        kinds: &["PerlSymbol"],
        why: "no call graph is recovered: Perl dispatch is dynamic",
    },
    &Unsupported {
        family: "elixir",
        kinds: &["ElixirSymbol"],
        why: "no call graph is recovered",
    },
    &Unsupported {
        family: "binary",
        kinds: &["BinaryMethod"],
        why: "compiled .NET / JVM methods come from the private RFC 0148 extension; its call \
              graph is not read by the public check",
    },
];

/// Per-family reachability numbers (RFC 0233 §6).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FamilyReach {
    pub supported: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_because: Option<String>,
    pub symbols: usize,
    pub entry_points: usize,
    pub reachable: usize,
    pub unreachable: usize,
    pub dynamic: usize,
    /// Reachable / symbols.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reachability_coverage: Option<f64>,
    /// Dynamic / symbols — always published beside the coverage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_share: Option<f64>,
}

#[derive(Default)]
pub(crate) struct Reach {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) families: BTreeMap<String, FamilyReach>,
}

/// `*` matches any run of characters.
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !text.starts_with(first) || !text.ends_with(last) || text.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    true
}

fn family_of(o: &KirObject) -> Option<&'static dyn ReachabilityRules> {
    let k = custom_kind(o)?;
    RULES
        .iter()
        .copied()
        .find(|r| r.symbol_kinds().contains(&k))
}

fn estate<'a>(objects: &'a [KirObject], opts: &Options<'_>) -> Estate<'a> {
    let mut e = Estate {
        objects,
        families_present: BTreeSet::new(),
        java_sql_words: BTreeSet::new(),
        java_sql_partial: false,
        configured_roots: opts.entry_points.to_vec(),
        trigger_targets: BTreeSet::new(),
        trigger_functions: BTreeSet::new(),
    };
    let triggers: BTreeSet<Uuid> = objects
        .iter()
        .filter(|o| custom_kind(o) == Some("Trigger"))
        .map(|o| o.id.0)
        .collect();
    for r in opts.relationships {
        if r.kind == RelationshipKind::Calls && triggers.contains(&r.from.0) {
            e.trigger_targets.insert(r.to.0);
        }
    }
    for o in objects {
        if let Some(f) = family_of(o) {
            e.families_present.insert(f.family());
        }
        if custom_kind(o) == Some("Trigger")
            && let Some(f) = str_prop(o, "function")
        {
            let f = normalize(f.trim_end_matches("()"));
            e.trigger_functions.insert(tail_of(&f).to_string());
        }
        if custom_kind(o) == Some("JavaMethod")
            && let Some(sql) = o.properties.get("sql").and_then(|v| v.as_array())
        {
            for st in sql {
                if st.get("status").and_then(|v| v.as_str()) != Some("parsed") {
                    e.java_sql_partial = true;
                }
                if let Some(text) = st.get("text").and_then(|v| v.as_str()) {
                    e.java_sql_words.extend(
                        text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                            .filter(|w| !w.is_empty())
                            .map(str::to_lowercase),
                    );
                }
            }
        }
    }
    e
}

pub(crate) fn check(objects: &[KirObject], opts: &Options<'_>) -> Reach {
    let estate = estate(objects, opts);
    let by_id: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();
    let mut calls: BTreeMap<Uuid, BTreeSet<Uuid>> = BTreeMap::new();
    for r in opts.relationships {
        if r.kind == RelationshipKind::Calls && r.from != r.to {
            calls.entry(r.from.0).or_default().insert(r.to.0);
        }
    }

    // Roots: every family's entry points, plus every non-symbol caller (a view, a trigger, code
    // the check does not model) — they run whether or not something calls them.
    let mut roots: BTreeMap<Uuid, String> = BTreeMap::new();
    for o in objects {
        match family_of(o) {
            Some(f) if f.support() == Support::Supported && !f.excluded(o) => {
                if let Some(why) = f.entry_point(o, &estate) {
                    roots.insert(o.id.0, why);
                }
            }
            Some(_) => {}
            None if calls.contains_key(&o.id.0) => {
                roots.insert(o.id.0, format!("called from {} {}", o.kind, o.name));
            }
            None => {}
        }
    }
    // Excluded symbols (migration scripts) still run: what they call is reached.
    for o in objects {
        if let Some(f) = family_of(o)
            && f.excluded(o)
            && calls.contains_key(&o.id.0)
        {
            roots.insert(o.id.0, "called from a migration script".into());
        }
    }
    let mut reached: BTreeSet<Uuid> = roots.keys().copied().collect();
    let mut queue: VecDeque<Uuid> = roots.keys().copied().collect();
    while let Some(u) = queue.pop_front() {
        for v in calls.get(&u).into_iter().flatten() {
            if reached.insert(*v) {
                queue.push_back(*v);
            }
        }
    }

    let mut families: BTreeMap<String, FamilyReach> = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for o in objects {
        let Some(f) = family_of(o) else {
            continue;
        };
        let stats = families.entry(f.family().to_string()).or_insert_with(|| {
            let mut s = FamilyReach::default();
            match f.support() {
                Support::Supported => s.supported = true,
                Support::Unsupported(why) => s.unsupported_because = Some(why.to_string()),
            }
            s
        });
        if f.excluded(o) {
            continue;
        }
        stats.symbols += 1;
        if f.support() != Support::Supported {
            continue;
        }
        if roots.contains_key(&o.id.0) {
            stats.entry_points += 1;
        }
        if reached.contains(&o.id.0) {
            stats.reachable += 1;
            continue;
        }
        let (kind, severity, confidence, reason) = match f.dynamic(o, &estate) {
            Some(why) => {
                stats.dynamic += 1;
                (
                    DiagnosticKind::DynamicReferenceUnknown,
                    Severity::Info,
                    0.2,
                    format!("no entry point reaches it, but {why}"),
                )
            }
            None => {
                stats.unreachable += 1;
                (
                    DiagnosticKind::UnreachableSymbol,
                    Severity::Warning,
                    0.6,
                    "no entry point, view, trigger or caller reaches it".to_string(),
                )
            }
        };
        let located = locate_object(opts, o);
        let file = str_prop(o, "source_path").or_else(|| located.as_ref().map(|l| l.0.clone()));
        let key = format!(
            "{}|{}|{}",
            f.family(),
            file.as_deref().unwrap_or(""),
            o.name.to_lowercase()
        );
        diagnostics.push(Diagnostic {
            kind,
            severity,
            confidence,
            source: SourceRef {
                object_id: o.id,
                object_kind: o.kind.to_string(),
                object_name: o.name.clone(),
                file,
                line: crate::xref::u32_prop(o.properties.get("line"))
                    .or_else(|| located.and_then(|l| l.1)),
                extractor: format!("{}-reachability", f.family()),
                access: "reach".to_string(),
            },
            target_spec: TargetSpec {
                written: o.name.clone(),
                normalized: o.name.to_lowercase(),
                expected: "an entry point".to_string(),
                namespace: None,
            },
            candidates: Vec::new(),
            evidence: o.evidence.clone(),
            coverage_gap: false,
            cross_system: false,
            reason,
            fingerprint: ekos_common::ContentHash::of_str(&format!(
                "xref/v1|{}|{key}",
                kind.as_str()
            ))
            .as_str()
            .to_string(),
        });
    }
    for s in families.values_mut() {
        if s.supported && s.symbols > 0 {
            s.reachability_coverage = Some(s.reachable as f64 / s.symbols as f64);
            s.dynamic_share = Some(s.dynamic as f64 / s.symbols as f64);
        }
    }
    let _ = by_id;
    Reach {
        diagnostics,
        families,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xref;
    use ekos_kir::{KirRelationship, ObjectKind};
    use serde_json::json;

    fn routine(name: &str) -> KirObject {
        KirObject::new(name, ObjectKind::Custom("Procedure".into()))
            .with_property("source_path", json!("sql/r.sql"))
    }

    fn calls(a: &KirObject, b: &KirObject) -> KirRelationship {
        KirRelationship::deterministic(RelationshipKind::Calls, a.id, b.id, "t")
    }

    fn run(objects: &[KirObject], rels: &[KirRelationship], roots: &[String]) -> xref::Report {
        xref::check(
            objects,
            &Options {
                relationships: rels,
                entry_points: roots,
                ..Default::default()
            },
        )
    }

    fn kinds(r: &xref::Report, k: DiagnosticKind) -> Vec<String> {
        r.diagnostics
            .iter()
            .filter(|d| d.kind == k)
            .map(|d| d.source.object_name.clone())
            .collect()
    }

    #[test]
    fn routines_reached_from_a_view_a_trigger_java_or_config_are_live() {
        let view = KirObject::new("v", ObjectKind::Custom("View".into()));
        let trigger = KirObject::new("t", ObjectKind::Custom("Trigger".into()));
        let (by_view, by_trigger, by_java, by_config, callee, dead) = (
            routine("by_view"),
            routine("by_trigger"),
            routine("post_invoice"),
            routine("api_entry"),
            routine("helper"),
            routine("dead_one"),
        );
        let java = KirObject::new("doIt", ObjectKind::Custom("JavaMethod".into())).with_property(
            "sql",
            json!([{"line": 1, "status": "parsed", "text": "SELECT post_invoice(?)"}]),
        );
        let rels = [
            calls(&view, &by_view),
            calls(&trigger, &by_trigger),
            calls(&by_config, &callee),
        ];
        let r = run(
            &[
                view, trigger, by_view, by_trigger, by_java, by_config, callee, dead, java,
            ],
            &rels,
            &["api_*".to_string()],
        );
        assert_eq!(kinds(&r, DiagnosticKind::UnreachableSymbol), ["dead_one"]);
        let sql = &r.metrics.reachability["sql"];
        assert_eq!((sql.symbols, sql.reachable, sql.unreachable), (6, 5, 1));
        assert!(r.metrics.reachability["java"].unsupported_because.is_some());
    }

    #[test]
    fn perl_code_in_the_estate_makes_an_unreached_routine_unprovable() {
        let perl = KirObject::new("handler", ObjectKind::Custom("PerlSymbol".into()));
        let pkg = KirObject::new("Handler", ObjectKind::Custom("PerlPackage".into()));
        let owns = KirRelationship::deterministic(RelationshipKind::Contains, pkg.id, perl.id, "t");
        let r = run(&[routine("maybe_called"), perl, pkg], &[owns], &[]);
        assert_eq!(
            kinds(&r, DiagnosticKind::DynamicReferenceUnknown),
            ["maybe_called"]
        );
        assert_eq!(r.metrics.reachability["sql"].dynamic_share, Some(1.0));
    }

    #[test]
    fn migration_routines_are_not_symbols_but_what_they_call_is_reached() {
        let mig = KirObject::new("fix", ObjectKind::Custom("Procedure".into()))
            .with_property("source_path", json!("sql/changes/1.2/fix.sql"));
        let used = routine("used_by_fix");
        let rels = [calls(&mig, &used)];
        let r = run(&[mig, used], &rels, &[]);
        assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
        assert_eq!(r.metrics.reachability["sql"].symbols, 1);
    }

    #[test]
    fn a_trigger_names_its_function_even_without_an_edge() {
        let t = KirObject::new("t", ObjectKind::Custom("Trigger".into()))
            .with_property("function", json!("audit_fn()"));
        let f = routine("audit_fn");
        let other = routine("x");
        let rels = [calls(&other, &f)];
        let r = run(&[t, f, other.clone()], &rels, &[]);
        assert_eq!(kinds(&r, DiagnosticKind::UnreachableSymbol), ["x"]);
    }

    #[test]
    fn every_registered_symbol_kind_is_claimed_by_a_family() {
        let claimed: BTreeSet<&str> = RULES
            .iter()
            .flat_map(|r| r.symbol_kinds())
            .copied()
            .collect();
        for kind in ekos_kir::custom_kinds::REGISTRY.iter().map(|k| k.name) {
            let symbol_like =
                kind.ends_with("Symbol") || kind.ends_with("Method") || kind == "Procedure";
            assert!(
                !symbol_like || claimed.contains(kind),
                "`{kind}` is a symbol kind no reachability family declares — add it to \
                 `xref_reach::RULES`, as supported or unsupported with the reason"
            );
        }
    }
}
