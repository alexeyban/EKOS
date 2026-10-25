//! Human-only review paths stay out of `commands/mcp.rs` (issue #14).
//!
//! The per-RFC guards in `semantics.rs`, `session.rs`, `migrate.rs` and `conflicts.rs` look for a
//! literal path such as `conflicts::resolve` in the text of `mcp.rs`. They catch an omission but
//! not an alias: `use super::conflicts::{resolve as settle};` or `use ekos_semantic::conflicts as
//! c;` followed by `c::resolve(..)` reach the same function without the literal text. This guard
//! parses `mcp.rs`, resolves every `use` (renames, groups, `self`, globs) and every path in the
//! code, macro arguments included, back to a full path before matching. Comments and string
//! literals are not code and are never matched.
//!
//! A rename would make a literal scan pass forever, so each entry point is also checked to still
//! exist in the source that defines it: renaming one fails here until the list is updated.
//!
//! Extension MCP tools (RFC 0149 / 0191) are outside this check; RFC 0149 states that an
//! extension tool must not reach a review entry point either.

use std::collections::HashMap;

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

/// One human-only entry point: the path segments that reach it (matched as a contiguous run
/// anywhere in a resolved path), and where it is defined, with text that must still be there.
struct HumanOnly {
    path: &'static [&'static str],
    defined_in: &'static str,
    definition: &'static str,
    why: &'static str,
}

const HUMAN_ONLY: &[HumanOnly] = &[
    HumanOnly {
        path: &["review", "decide"],
        defined_in: include_str!("review.rs"),
        definition: "pub fn decide(",
        why: "RFC 0213: a review decision is a human decision",
    },
    HumanOnly {
        path: &["review", "approve"],
        defined_in: include_str!("review.rs"),
        definition: "pub fn approve(",
        why: "RFC 0213: signing the source model off is a human decision",
    },
    HumanOnly {
        path: &["review", "revoke"],
        defined_in: include_str!("review.rs"),
        definition: "pub fn revoke(",
        why: "RFC 0213: withdrawing a sign-off is a human decision",
    },
    HumanOnly {
        path: &["review", "decide_identity"],
        defined_in: include_str!("review.rs"),
        definition: "pub fn decide_identity(",
        why: "RFC 0213: confirming an identity match from the review is a human decision",
    },
    HumanOnly {
        path: &["review", "decision_object"],
        defined_in: include_str!("../../../semantic/src/review.rs"),
        definition: "pub fn decision_object(",
        why: "RFC 0213: a review decision is a human decision",
    },
    HumanOnly {
        path: &["review", "signoff_object"],
        defined_in: include_str!("../../../semantic/src/review.rs"),
        definition: "pub fn signoff_object(",
        why: "RFC 0213: signing the source model off is a human decision",
    },
    HumanOnly {
        path: &["semantics_review"],
        defined_in: include_str!("../../../semantic/src/semantics_review.rs"),
        definition: "pub fn apply_review(",
        why: "RFC 0170: confirming a business definition is a human decision",
    },
    HumanOnly {
        path: &["import", "linkml"],
        defined_in: include_str!("import.rs"),
        definition: "pub fn linkml(",
        why: "RFC 0170: a LinkML import records human review decisions",
    },
    HumanOnly {
        path: &["conflicts", "resolve"],
        defined_in: include_str!("conflicts.rs"),
        definition: "pub fn resolve(",
        why: "RFC 0172: settling a conflict is a human decision",
    },
    HumanOnly {
        path: &["conflicts", "resolve"],
        defined_in: include_str!("../../../semantic/src/conflicts.rs"),
        definition: "pub fn resolve(",
        why: "RFC 0172: settling a conflict is a human decision",
    },
    HumanOnly {
        path: &["Resolution"],
        defined_in: include_str!("../../../semantic/src/conflicts.rs"),
        definition: "pub enum Resolution",
        why: "RFC 0172: a resolution is only built to settle a conflict",
    },
    HumanOnly {
        path: &["ekos_session", "lifecycle"],
        defined_in: include_str!("../../../session/src/lifecycle.rs"),
        definition: "pub fn supersede(",
        why: "RFC 0151: promoting an agent note is a human decision",
    },
    HumanOnly {
        path: &["ekos_migrate", "lifecycle"],
        defined_in: include_str!("../../../migrate/src/lifecycle.rs"),
        definition: "pub enum Actor",
        why: "RFC 0154: moving a migration unit through its lifecycle is human-only",
    },
    HumanOnly {
        path: &["ekos_migrate_approval", "lifecycle"],
        defined_in: include_str!("../../../migrate-approval/src/lib.rs"),
        definition: "pub mod lifecycle;",
        why: "RFC 0161: approving a migration is human-only",
    },
    HumanOnly {
        path: &["ApprovalRequest"],
        defined_in: include_str!("../../../migrate-approval/src/request.rs"),
        definition: "pub struct ApprovalRequest",
        why: "RFC 0161: an approval request is only built to approve",
    },
    HumanOnly {
        path: &["review_authorization"],
        defined_in: include_str!("treasury.rs"),
        definition: "pub fn review_authorization(",
        why: "RFC 0032 / issue #16: confirming a payment authorization is a human decision",
    },
    HumanOnly {
        path: &["Actor", "Human"],
        defined_in: include_str!("../../../migrate/src/lifecycle.rs"),
        definition: "    Human,",
        why: "RFC 0154/0151: acting as a human is what MCP must never do",
    },
];

/// `use` declarations of a file, flattened: each name it brings into scope with the full path
/// it stands for, plus every glob's path.
#[derive(Default)]
struct Imports {
    names: HashMap<String, Vec<String>>,
    globs: Vec<Vec<String>>,
}

impl Imports {
    fn add(&mut self, prefix: &[String], tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(p) => {
                let mut next = prefix.to_vec();
                next.push(p.ident.to_string());
                self.add(&next, &p.tree);
            }
            syn::UseTree::Name(n) => {
                let name = n.ident.to_string();
                let mut full = prefix.to_vec();
                if name != "self" {
                    full.push(name.clone());
                }
                let key = if name == "self" {
                    prefix.last().cloned().unwrap_or_default()
                } else {
                    name
                };
                self.names.insert(key, full);
            }
            syn::UseTree::Rename(r) => {
                let mut full = prefix.to_vec();
                if r.ident != "self" {
                    full.push(r.ident.to_string());
                }
                self.names.insert(r.rename.to_string(), full);
            }
            syn::UseTree::Glob(_) => self.globs.push(prefix.to_vec()),
            syn::UseTree::Group(g) => {
                for t in &g.items {
                    self.add(prefix, t);
                }
            }
        }
    }

    /// `path` with its first segment replaced by what it was imported as, repeatedly.
    fn resolve(&self, path: &[String]) -> Vec<String> {
        let mut out = path.to_vec();
        for _ in 0..8 {
            match out.first().and_then(|head| self.names.get(head)) {
                Some(full) if full.as_slice() != &out[..1] => {
                    let mut next = full.clone();
                    next.extend_from_slice(&out[1..]);
                    out = next;
                }
                _ => break,
            }
        }
        out
    }
}

impl<'ast> Visit<'ast> for Imports {
    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        self.add(&[], &u.tree);
    }
}

/// Every `a::b::c` path in a token stream, macro bodies included.
fn paths(tokens: TokenStream, out: &mut Vec<Vec<String>>) {
    let mut current: Vec<String> = Vec::new();
    let mut colons = 0;
    let flush = |current: &mut Vec<String>, out: &mut Vec<Vec<String>>| {
        if !current.is_empty() {
            out.push(std::mem::take(current));
        }
    };
    for tt in tokens {
        match tt {
            TokenTree::Ident(i) => {
                if colons != 2 {
                    flush(&mut current, out);
                }
                current.push(i.to_string());
                colons = 0;
            }
            TokenTree::Punct(p) if p.as_char() == ':' => colons += 1,
            TokenTree::Group(g) => {
                flush(&mut current, out);
                colons = 0;
                paths(g.stream(), out);
            }
            _ => {
                flush(&mut current, out);
                colons = 0;
            }
        }
    }
    flush(&mut current, out);
}

fn contains_run(path: &[String], run: &[&str]) -> bool {
    path.windows(run.len())
        .any(|w| w.iter().zip(run).all(|(a, b)| a == b))
}

/// A path as its segments.
type Path = Vec<String>;

/// `source` parsed, with its `#[cfg(test)]` items dropped, and its imports.
fn parse_production(source: &str) -> (syn::File, Imports) {
    let mut file = syn::parse_file(source).expect("parses");
    file.items.retain(|item| {
        let attrs = match item {
            syn::Item::Mod(m) => &m.attrs,
            syn::Item::Fn(f) => &f.attrs,
            _ => return true,
        };
        !attrs.iter().any(|a| {
            a.path().is_ident("cfg") && a.parse_args::<syn::Ident>().is_ok_and(|i| i == "test")
        })
    });
    let mut imports = Imports::default();
    imports.visit_file(&file);
    (file, imports)
}

/// The functions in `source` (test modules excluded; free functions and methods) whose bodies
/// contain a path that, resolved through the file's imports, satisfies `reaches`. Method calls
/// count: `x.append_event(..)` is the one-segment path `append_event`. Used by `mcp.rs`'s
/// `only_listed_functions_can_write_the_ledger` (#11), so an alias cannot hide a writer.
pub(crate) fn functions_reaching(
    source: &str,
    reaches: impl Fn(&[String]) -> bool,
) -> std::collections::BTreeSet<String> {
    struct Fns<'a, F> {
        imports: &'a Imports,
        reaches: F,
        found: std::collections::BTreeSet<String>,
    }
    impl<F: Fn(&[String]) -> bool> Fns<'_, F> {
        fn check(&mut self, name: &syn::Ident, block: &syn::Block) {
            let mut all = Vec::new();
            paths(quote::ToTokens::to_token_stream(block), &mut all);
            if all.iter().any(|p| (self.reaches)(&self.imports.resolve(p))) {
                self.found.insert(name.to_string());
            }
        }
    }
    impl<'ast, F: Fn(&[String]) -> bool> Visit<'ast> for Fns<'_, F> {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            self.check(&f.sig.ident, &f.block);
            syn::visit::visit_item_fn(self, f);
        }
        fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
            self.check(&f.sig.ident, &f.block);
            syn::visit::visit_impl_item_fn(self, f);
        }
    }
    let (file, imports) = parse_production(source);
    let mut fns = Fns {
        imports: &imports,
        reaches,
        found: Default::default(),
    };
    fns.visit_file(&file);
    fns.found
}

/// Every path in `source` (test modules excluded), resolved through its imports, and the globs.
fn resolved_paths(source: &str) -> (Vec<(Path, Path)>, Vec<Path>) {
    let (file, imports) = parse_production(source);
    let mut all = Vec::new();
    for item in &file.items {
        paths(quote::ToTokens::to_token_stream(item), &mut all);
    }
    let resolved = all
        .into_iter()
        .map(|p| {
            let r = imports.resolve(&p);
            (p, r)
        })
        .collect();
    (resolved, imports.globs)
}

/// What in `source` (a Rust file, test modules excluded) reaches a human-only entry point.
fn violations(source: &str) -> Vec<String> {
    let (resolved, globs) = resolved_paths(source);
    let mut found = Vec::new();
    for g in &globs {
        for h in HUMAN_ONLY {
            // A glob of the module that holds an entry point brings it into scope unnamed.
            let module = &h.path[..h.path.len().saturating_sub(1)];
            if !module.is_empty()
                && contains_run(g, module)
                && g.ends_with(&[module.last().unwrap().to_string()])
            {
                found.push(format!("glob import of {} ({})", g.join("::"), h.why));
            }
        }
    }
    for (p, resolved) in resolved {
        for h in HUMAN_ONLY {
            if contains_run(&resolved, h.path) {
                found.push(format!(
                    "{} -> {} ({})",
                    p.join("::"),
                    resolved.join("::"),
                    h.why
                ));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

#[test]
fn mcp_cannot_reach_a_human_only_entry_point() {
    let src = include_str!("mcp.rs");
    // The guard really reads the file: it sees mcp.rs's legitimate read of the conflicts module.
    let (resolved, _) = resolved_paths(src);
    assert!(
        resolved
            .iter()
            .any(|(_, r)| contains_run(r, &["conflicts", "open_for"])),
        "mcp.rs was not read"
    );
    let found = violations(src);
    assert!(
        found.is_empty(),
        "human-only paths reachable from mcp.rs: {found:#?}"
    );
}

/// A rename must fail here rather than leave the guard matching a name nothing has any more.
#[test]
fn every_guarded_entry_point_still_exists() {
    for h in HUMAN_ONLY {
        assert!(
            h.defined_in.contains(h.definition),
            "`{}` is gone from its source: update HUMAN_ONLY ({})",
            h.definition,
            h.why
        );
    }
}

/// The guard sees through the evasions the literal scans miss.
#[test]
fn aliases_groups_globs_and_macros_are_resolved() {
    let caught = |src: &str| !violations(src).is_empty();
    assert!(caught(
        "use super::conflicts::{resolve as settle};\nfn f() { settle(); }"
    ));
    assert!(caught(
        "use ekos_semantic::conflicts as c;\nfn f() { c::resolve(); }"
    ));
    assert!(caught(
        "use super::conflicts::{self as k};\nfn f() { k::resolve(); }"
    ));
    assert!(caught("use super::conflicts::*;\nfn f() {}"));
    assert!(caught(
        "use ekos_semantic as s;\nfn f() { s::semantics_review::apply_review(); }"
    ));
    assert!(caught(
        "fn f() { format!(\"{}\", ekos_migrate::lifecycle::abandon()); }"
    ));
    assert!(caught(
        "use ekos_migrate::lifecycle::Actor as A;\nfn f() { let _ = A::Human; }"
    ));
    // Not code: comments, strings, and test modules.
    assert!(!caught(
        "// conflicts::resolve\nfn f() { let _ = \"conflicts::resolve\"; }"
    ));
    assert!(!caught(
        "#[cfg(test)]\nmod tests { fn f() { super::conflicts::resolve(); } }"
    ));
    // A legitimate read of the same module is fine.
    assert!(!caught(
        "use super::conflicts;\nfn f() { conflicts::open_for(); }"
    ));
}

/// The writer scan behind `only_listed_functions_can_write_the_ledger` sees aliases, method calls
/// and macro arguments, and ignores comments and strings.
#[test]
fn writer_scan_resolves_aliases() {
    let writes = |p: &[String]| {
        p.last()
            .is_some_and(|l| l.starts_with("append_") || l == "open_store")
    };
    let src = "use super::store::open_store as os;\n\
               fn a() { let l = os(); }\n\
               fn b(l: &L) { l.append_event(&e); }\n\
               fn c() { format!(\"{}\", x.append_object(o)); }\n\
               fn d() { /* open_store( */ let _ = \"append_object\"; }\n\
               impl T { fn e(&self) { self.s.append_relationship(r); } }\n";
    let found: Vec<String> = functions_reaching(src, writes).into_iter().collect();
    assert_eq!(found, ["a", "b", "c", "e"]);
}
