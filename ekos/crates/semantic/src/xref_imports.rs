//! RFC 0233 Phase 2 — imports that name a module of this workspace that does not exist.
//!
//! Only languages whose toolchain does not already refuse a missing module are checked: Perl
//! (`use`/`require`), Elixir (`alias`/`import`/`use`), JavaScript relative specifiers and Python.
//! Rust and Java imports are left to `cargo` and `javac` (RFC 0233 non-goals).
//!
//! An import is *internal* only when its name falls under a namespace the workspace itself defines
//! (`LedgerSMB::…` beside `LedgerSMB::Report`; `./util` beside the importing file; a Python package
//! with files here). Everything else is a third-party dependency and is counted, not checked.

use crate::xref::{
    Candidate, DiagnosticKind, Expect, Finding, Options, Outcome, Reference, Severity, custom_kind,
    is_test_path, str_prop, to_candidate,
};
use ekos_kir::{KirId, KirObject, ObjectKind, RelationshipKind};
use std::collections::{BTreeSet, HashMap};

/// One checked import.
pub(crate) struct ImportResult<'a> {
    pub(crate) reference: Reference<'a>,
    pub(crate) normalized: String,
    pub(crate) outcome: Outcome,
    pub(crate) finding: Option<Finding>,
}

pub(crate) struct Imports<'a> {
    pub(crate) results: Vec<ImportResult<'a>>,
    /// Imports of something outside the workspace.
    pub(crate) external: usize,
}

/// Script and code extensions a relative JS specifier may leave off.
const JS_EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts", "vue", "svelte", "json",
];

/// Packages and modules share one id between their declaration and every `use` of them, so the
/// object alone does not say whether a file declares it: the declaring file's (or enclosing
/// module's) `Contains` edge does.
fn declared_ids(rels: &[ekos_kir::KirRelationship]) -> BTreeSet<uuid::Uuid> {
    rels.iter()
        .filter(|r| r.kind == RelationshipKind::Contains)
        .map(|r| r.to.0)
        .collect()
}

/// Perl `A::B`, Elixir `A.B` → `A`.
fn root(name: &str, sep: &str) -> String {
    name.split(sep).next().unwrap_or(name).to_string()
}

/// `a/b/../c/./d` → `a/c/d`; `None` when it climbs out of the workspace.
fn normalize_path(p: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            s => out.push(s),
        }
    }
    Some(out.join("/"))
}

/// What the Python files of the workspace make importable.
#[derive(Default)]
struct PythonModules {
    /// Every importable dotted name: modules and packages.
    names: BTreeSet<String>,
    /// Top-level names — an import under one of them is internal.
    roots: BTreeSet<String>,
}

impl PythonModules {
    /// A module's dotted name is its path from the top of its package chain: the highest
    /// directory of consecutive `__init__.py` directories, or the file's own directory.
    fn build(files: &BTreeSet<&str>) -> Self {
        let has_init = |dir: &str| {
            let init = if dir.is_empty() {
                "__init__.py".to_string()
            } else {
                format!("{dir}/__init__.py")
            };
            files.contains(init.as_str())
        };
        let mut m = PythonModules::default();
        for f in files.iter().filter(|f| f.ends_with(".py")) {
            let segs: Vec<&str> = f.trim_end_matches(".py").split('/').collect();
            // segs[..i] is the directory chain; walk up while each directory is a package.
            let mut top = segs.len() - 1;
            while top > 0 && has_init(&segs[..top].join("/")) {
                top -= 1;
            }
            let mut dotted: Vec<&str> = segs[top..].to_vec();
            if dotted.last() == Some(&"__init__") {
                dotted.pop();
            }
            if dotted.is_empty() {
                continue;
            }
            m.roots.insert(dotted[0].to_string());
            for i in 1..=dotted.len() {
                m.names.insert(dotted[..i].join("."));
            }
        }
        m
    }
}

pub(crate) fn check<'a>(objects: &'a [KirObject], opts: &Options<'_>) -> Imports<'a> {
    let by_id: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();
    let declared = declared_ids(opts.relationships);
    let declaring = crate::xref::declaring_files(objects, opts.relationships);
    let defined = |o: &KirObject| declared.contains(&o.id.0);
    let mut perl_roots = BTreeSet::new();
    let mut elixir_roots = BTreeSet::new();
    let mut perl_defined: Vec<&KirObject> = Vec::new();
    let mut elixir_defined: Vec<&KirObject> = Vec::new();
    let mut files: BTreeSet<&str> = BTreeSet::new();
    for o in objects {
        match (&o.kind, custom_kind(o)) {
            (_, Some("PerlPackage")) if defined(o) => {
                perl_roots.insert(root(&o.name, "::"));
                perl_defined.push(o);
            }
            (_, Some("ElixirModule")) if defined(o) => {
                elixir_roots.insert(root(&o.name, "."));
                elixir_defined.push(o);
            }
            (ObjectKind::File, _) => {
                files.insert(o.name.as_str());
            }
            _ => {}
        }
    }
    let python = PythonModules::build(&files);
    let excluded = Excluded::build(opts.unobserved_files);

    let mut out = Imports {
        results: Vec::new(),
        external: 0,
    };
    let mut seen: BTreeSet<(uuid::Uuid, uuid::Uuid)> = BTreeSet::new();
    for rel in opts.relationships {
        if rel.kind != RelationshipKind::DependsOn || !seen.insert((rel.from.0, rel.to.0)) {
            continue;
        }
        let (Some(from), Some(to)) = (by_id.get(&rel.from), by_id.get(&rel.to)) else {
            continue;
        };
        let located = rel
            .evidence
            .first()
            .and_then(|e| opts.locate.and_then(|f| f(e)));
        let file = located
            .as_ref()
            .map(|l| l.0.clone())
            .or_else(|| str_prop(from, "source_path"))
            .or_else(|| declaring.get(&from.id.0).cloned())
            .or_else(|| (from.kind == ObjectKind::File).then(|| from.name.clone()));
        let line = located.and_then(|l| l.1);
        let reference = |extractor: &'static str| Reference {
            source: from,
            written: to.name.clone(),
            access: "import",
            extractor,
            file: file.clone(),
            line,
            cross_system: false,
            uncertain: false,
            against_foreign: false,
            expects: Expect::Module,
        };
        let (extractor, outcome, finding) = match custom_kind(to) {
            Some("PerlPackage") => {
                if defined(to) {
                    ("perl-use", Outcome::Resolved, None)
                } else if let Some(path) = excluded.perl.get(&to.name) {
                    (
                        "perl-use",
                        Outcome::Unresolved,
                        Some(excluded_finding(path)),
                    )
                } else if perl_roots.contains(&root(&to.name, "::")) {
                    let f = missing(
                        &to.name,
                        format!(
                            "no file defines package `{}`, though `{}::` packages are defined here",
                            to.name,
                            root(&to.name, "::")
                        ),
                        &perl_defined,
                        file.as_deref(),
                    );
                    ("perl-use", Outcome::Unresolved, Some(f))
                } else {
                    out.external += 1;
                    continue;
                }
            }
            Some("ElixirModule") => {
                if defined(to) {
                    ("elixir-alias", Outcome::Resolved, None)
                } else if elixir_roots.contains(&root(&to.name, ".")) {
                    let f = missing(
                        &to.name,
                        format!(
                            "no file defines module `{}`, though `{}.` modules are defined here",
                            to.name,
                            root(&to.name, ".")
                        ),
                        &elixir_defined,
                        file.as_deref(),
                    );
                    ("elixir-alias", Outcome::Unresolved, Some(f))
                } else {
                    out.external += 1;
                    continue;
                }
            }
            Some("JsModule") => {
                let spec = to.name.as_str();
                let (Some(file), true) = (
                    file.as_deref(),
                    spec.starts_with("./") || spec.starts_with("../"),
                ) else {
                    out.external += 1;
                    continue;
                };
                let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
                let Some(base) = normalize_path(&format!("{dir}/{spec}")) else {
                    continue;
                };
                // A stylesheet or image import resolves through a loader EKOS does not observe.
                let ext = base
                    .rsplit('/')
                    .next()
                    .and_then(|f| f.rsplit_once('.'))
                    .map(|x| x.1);
                if ext.is_some_and(|e| !JS_EXTENSIONS.contains(&e)) {
                    out.external += 1;
                    continue;
                }
                let found = files.contains(base.as_str())
                    || JS_EXTENSIONS.iter().any(|e| {
                        files.contains(format!("{base}.{e}").as_str())
                            || files.contains(format!("{base}/index.{e}").as_str())
                    });
                let hidden = std::iter::once(base.clone())
                    .chain(
                        JS_EXTENSIONS
                            .iter()
                            .flat_map(|e| [format!("{base}.{e}"), format!("{base}/index.{e}")]),
                    )
                    .find(|p| excluded.files.is_some_and(|f| f.contains(p)));
                if found {
                    ("js-import", Outcome::Resolved, None)
                } else if let Some(path) = hidden {
                    (
                        "js-import",
                        Outcome::Unresolved,
                        Some(excluded_finding(&path)),
                    )
                } else {
                    let f = Finding {
                        kind: DiagnosticKind::UnresolvedReference,
                        severity: if is_test_path(file) {
                            Severity::Info
                        } else {
                            Severity::Error
                        },
                        confidence: if is_test_path(file) { 0.3 } else { 0.8 },
                        reason: format!(
                            "no observed file at `{base}` (with any script extension or `/index`)"
                        ),
                        candidates: Vec::new(),
                        coverage_gap: false,
                        expected: "module",
                        namespace: None,
                    };
                    ("js-import", Outcome::Unresolved, Some(f))
                }
            }
            Some("PythonModule") => {
                let name = to.name.as_str();
                let first = name.split('.').next().unwrap_or(name);
                if !python.roots.contains(first) {
                    out.external += 1;
                    continue;
                }
                // `from a.b import c` is recorded as `a.b.c`: `c` may be a module or a name in `a.b`.
                let parent = name.rsplit_once('.').map(|x| x.0);
                let hidden = excluded
                    .python
                    .get(name)
                    .or_else(|| parent.and_then(|p| excluded.python.get(p)));
                if python.names.contains(name) || parent.is_some_and(|p| python.names.contains(p)) {
                    ("python-import", Outcome::Resolved, None)
                } else if let Some(path) = hidden {
                    (
                        "python-import",
                        Outcome::Unresolved,
                        Some(excluded_finding(path)),
                    )
                } else {
                    let f = Finding {
                        kind: DiagnosticKind::UnresolvedReference,
                        // The analyzer drops a relative import's dots, so `from .x import y` reads
                        // as `x.y`: a warning, not an error.
                        severity: Severity::Warning,
                        confidence: 0.6,
                        reason: format!(
                            "`{first}` is a package of this workspace, but no file makes `{name}` \
                             or its parent importable"
                        ),
                        candidates: Vec::new(),
                        coverage_gap: false,
                        expected: "module",
                        namespace: None,
                    };
                    ("python-import", Outcome::Unresolved, Some(f))
                }
            }
            _ => continue,
        };
        out.results.push(ImportResult {
            reference: reference(extractor),
            normalized: to.name.to_lowercase(),
            outcome,
            finding,
        });
    }
    out
}

/// Modules defined in files the workspace does not observe, by every name they could be imported
/// as: `old/lib/LedgerSMB/Form.pm` is `Form`, `LedgerSMB::Form`, `lib::LedgerSMB::Form`, ….
#[derive(Default)]
struct Excluded<'a> {
    files: Option<&'a BTreeSet<String>>,
    perl: HashMap<String, String>,
    python: HashMap<String, String>,
}

impl<'a> Excluded<'a> {
    fn build(files: Option<&'a BTreeSet<String>>) -> Self {
        let mut e = Excluded {
            files,
            ..Default::default()
        };
        for f in files.into_iter().flatten() {
            let (stem, sep, map) = if let Some(s) = f.strip_suffix(".pm") {
                (s, "::", &mut e.perl)
            } else if let Some(s) = f.strip_suffix(".py") {
                (s.trim_end_matches("/__init__"), ".", &mut e.python)
            } else {
                continue;
            };
            let segs: Vec<&str> = stem.split('/').collect();
            for i in 0..segs.len() {
                map.entry(segs[i..].join(sep)).or_insert_with(|| f.clone());
            }
        }
        e
    }
}

fn excluded_finding(path: &str) -> Finding {
    Finding {
        kind: DiagnosticKind::UnresolvedReference,
        severity: Severity::Info,
        confidence: 0.3,
        reason: format!(
            "defined in `{path}`, which the workspace does not observe (`[observe] ignore-patterns`)"
        ),
        candidates: Vec::new(),
        coverage_gap: true,
        expected: "module",
        namespace: None,
    }
}

/// A missing Perl package / Elixir module, with the closest defined names.
fn missing(name: &str, reason: String, defined: &[&KirObject], file: Option<&str>) -> Finding {
    let test = file.is_some_and(is_test_path);
    let lower = name.to_lowercase();
    let mut close: Vec<(usize, &KirObject)> = defined
        .iter()
        .filter_map(|o| {
            let d = edit_distance(&lower, &o.name.to_lowercase());
            (d <= 3).then_some((d, *o))
        })
        .collect();
    close.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    let candidates: Vec<Candidate> = close
        .into_iter()
        .take(3)
        .map(|(_, o)| to_candidate(o))
        .collect();
    Finding {
        kind: DiagnosticKind::UnresolvedReference,
        severity: if test {
            Severity::Info
        } else {
            Severity::Error
        },
        confidence: if test { 0.3 } else { 0.8 },
        reason: if test {
            format!("{reason}; test code")
        } else {
            reason
        },
        candidates,
        coverage_gap: false,
        expected: "module",
        namespace: None,
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != *cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xref;
    use ekos_kir::KirRelationship;

    fn file(path: &str) -> KirObject {
        KirObject::new(path, ObjectKind::File)
    }

    fn pkg(kind: &str, name: &str) -> KirObject {
        KirObject::new(name, ObjectKind::Custom(kind.into()))
    }

    fn contains(from: &KirObject, to: &KirObject) -> KirRelationship {
        KirRelationship::deterministic(RelationshipKind::Contains, from.id, to.id, "t")
    }

    fn dep(from: &KirObject, to: &KirObject) -> KirRelationship {
        KirRelationship::deterministic(RelationshipKind::DependsOn, from.id, to.id, "t")
    }

    fn run(objects: &[KirObject], rels: &[KirRelationship]) -> xref::Report {
        xref::check(
            objects,
            &Options {
                relationships: rels,
                ..Default::default()
            },
        )
    }

    #[test]
    fn a_missing_package_under_an_own_namespace_is_an_error_and_cpan_is_not_checked() {
        let pm = file("lib/LedgerSMB/Report.pm");
        let report = pkg("PerlPackage", "LedgerSMB::Report");
        let missing = pkg("PerlPackage", "LedgerSMB::Reprot");
        let dbi = pkg("PerlPackage", "DBI");
        let rels = [
            contains(&pm, &report),
            dep(&report, &missing),
            dep(&report, &dbi),
        ];
        let r = run(&[pm, report, missing, dbi], &rels);
        let [d] = r.diagnostics.as_slice() else {
            panic!("{:?}", r.diagnostics)
        };
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.candidates[0].name, "LedgerSMB::Report");
        assert_eq!(r.metrics.external, 1);
    }

    #[test]
    fn relative_js_imports_resolve_to_observed_files() {
        let app = file("UI/js/app.js");
        let ok = KirObject::new("./util", ObjectKind::Custom("JsModule".into()));
        let idx = KirObject::new("../lib", ObjectKind::Custom("JsModule".into()));
        let gone = KirObject::new("./gone", ObjectKind::Custom("JsModule".into()));
        let css = KirObject::new("./app.css", ObjectKind::Custom("JsModule".into()));
        let npm = KirObject::new("vue", ObjectKind::Custom("JsModule".into()));
        let rels: Vec<_> = [&ok, &idx, &gone, &css, &npm]
            .iter()
            .map(|m| dep(&app, m))
            .collect();
        let r = run(
            &[
                app.clone(),
                file("UI/js/util.ts"),
                file("UI/lib/index.js"),
                ok,
                idx,
                gone,
                css,
                npm,
            ],
            &rels,
        );
        let [d] = r.diagnostics.as_slice() else {
            panic!("{:?}", r.diagnostics)
        };
        assert_eq!(d.target_spec.written, "./gone");
        assert_eq!(d.source.file.as_deref(), Some("UI/js/app.js"));
        assert_eq!(r.metrics.external, 2);
        assert_eq!(r.metrics.by_extractor["js-import"].resolved, 2);
    }

    #[test]
    fn python_imports_resolve_through_the_package_chain() {
        let files = [
            "src/shop/__init__.py",
            "src/shop/models.py",
            "src/shop/api/__init__.py",
            "src/shop/api/views.py",
            "scripts/run.py",
        ];
        let objects: Vec<KirObject> = files.iter().map(|f| file(f)).collect();
        let m = PythonModules::build(&objects.iter().map(|o| o.name.as_str()).collect());
        assert!(m.names.contains("shop.api.views"));
        assert!(m.names.contains("run"));
        assert!(m.roots.contains("shop"));
        assert!(!m.roots.contains("src"));

        let from = objects[4].clone();
        let imports = [
            "shop.models.Order",
            "shop.api.views",
            "shop.billing.Invoice",
            "requests",
        ]
        .map(|n| KirObject::new(n, ObjectKind::Custom("PythonModule".into())));
        let rels: Vec<_> = imports.iter().map(|m| dep(&from, m)).collect();
        let mut all = objects.clone();
        all.extend(imports);
        let r = run(&all, &rels);
        let [d] = r.diagnostics.as_slice() else {
            panic!("{:?}", r.diagnostics)
        };
        assert_eq!(d.target_spec.written, "shop.billing.Invoice");
        assert_eq!(d.severity, Severity::Warning);
    }

    #[test]
    fn an_import_into_an_excluded_directory_is_a_coverage_gap() {
        let pm = file("lib/LedgerSMB/Report.pm");
        let report = pkg("PerlPackage", "LedgerSMB::Report");
        let form = pkg("PerlPackage", "LedgerSMB::Form");
        let rels = [contains(&pm, &report), dep(&report, &form)];
        let unobserved: BTreeSet<String> = ["old/lib/LedgerSMB/Form.pm".to_string()].into();
        let r = xref::check(
            &[pm, report, form],
            &Options {
                relationships: &rels,
                unobserved_files: Some(&unobserved),
                ..Default::default()
            },
        );
        let [d] = r.diagnostics.as_slice() else {
            panic!()
        };
        assert!(d.coverage_gap);
        assert_eq!(d.severity, Severity::Info);
        assert!(d.reason.contains("old/lib/LedgerSMB/Form.pm"));
    }

    #[test]
    fn paths_normalize_and_never_climb_out() {
        assert_eq!(normalize_path("a/b/../c/./d").as_deref(), Some("a/c/d"));
        assert_eq!(normalize_path("../x"), None);
    }
}
