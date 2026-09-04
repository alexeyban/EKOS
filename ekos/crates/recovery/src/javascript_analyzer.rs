//! `JavaScriptAnalyzerPass` — real AST-based decomposition of JavaScript/TypeScript source (RFC
//! 0085, Phase 5 of the source-decomposition plan) into plain KIR objects and relationships,
//! replacing `plugins/file`'s crude declaration-prefix symbol scan for `.js`/`.jsx`/`.ts`/`.tsx`/
//! `.mjs`/`.cjs` files — the last major real-code language family that still got no relationships,
//! no module structure, nothing but bare symbol name strings.
//!
//! Real parser this time (unlike Elixir's hand-written scanner): [`oxc_parser`], evaluated against
//! `swc_ecma_parser` before committing (see RFC 0085) — MIT-licensed, native TypeScript/JSX/TSX
//! support in one crate with no separate syntax-config setup, the deciding factor for a bounded,
//! "read what's declared" analyzer matching `rust_analyzer.rs`'s/`python_analyzer.rs`'s own scope.
//!
//! Scope, deliberately narrow and honest, same shape as every prior language analyzer this
//! session shipped:
//! - `import ... from "specifier"` becomes a real `Custom("JsModule")` object (one per distinct
//!   specifier per file — a file with several `import` statements naming the same module produces
//!   one `DependsOn` edge, not one per statement) + a `DependsOn` edge from the owning `File`.
//!   Relative imports (`"./Dashboard"`) are **not** resolved to the real internal file/component
//!   they point at — same honestly-scoped limitation `package_json_analyzer.rs` (RFC 0082) already
//!   documented for npm workspace-internal packages; real internal import resolution is a
//!   separate, harder problem (extension/`index.*` resolution, bundler alias configs) left for a
//!   future increment, not silently faked here.
//! - `function foo() {}` / `class Foo {}` (top-level, or one level inside `export`/
//!   `export default`) become `Custom("JsSymbol")` objects (`kind`: `"function"`/`"class"`,
//!   `visibility`: `"exported"`/`"local"` — a real signal from the real `export` keyword, not
//!   guessed) + a `Contains` edge from the owning `File`.
//! - `const Foo = () => {...}` / `const foo = function() {...}` (top-level function-valued
//!   `const`/`let`/`var`) become the same `Custom("JsSymbol")` shape — the real, common React
//!   component/hook authoring pattern; a plain non-function-valued top-level `const` is not a
//!   symbol worth surfacing on its own (would just be data-constant noise, the same judgment call
//!   `python_analyzer.rs` already makes by only surfacing `def`/`class`, not every top-level
//!   assignment).
//! - Not a call graph, not a JSX component-tree walk (matches every prior language analyzer's own
//!   scope decision) — module/symbol/dependency structure is what an architecture diagram needs.
//! - A file that fails to parse (`ParserReturn::panicked`) contributes nothing and is reported as
//!   a real diagnostic warning, not silently dropped without a trace and not treated as fatal to
//!   the whole pass — matches `sql_analyzer.rs`'s own per-statement-failure discipline.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use oxc_allocator::Allocator;
use oxc_ast::CommentContent;
use oxc_ast::ast::{
    BindingPattern, Class, Declaration, ExportDefaultDeclarationKind, Expression, Function,
    Program, Statement, VariableDeclaration,
};
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct JsArtifactData {
    path: String,
    source: String,
    /// RFC 0079: present only in a multi-`[observe] paths` workspace. Qualifies id hashing only —
    /// `path` stays bare everywhere it's displayed.
    #[serde(default)]
    project: Option<String>,
}

/// Coverage counters from one run, mirroring `ElixirStats`/`PythonStats`.
#[derive(Debug, Clone, Copy, Default)]
pub struct JavaScriptStats {
    pub files_processed: usize,
    pub files_failed_to_parse: usize,
    pub modules_total: usize,
    pub symbols_total: usize,
}

pub struct JavaScriptAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<JavaScriptStats>>,
}

impl JavaScriptAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("javascript-analyzer:{}", workspace_name.into()),
            artifact_ids,
            stats: Arc::new(Mutex::new(JavaScriptStats::default())),
        }
    }

    pub fn stats_handle(&self) -> Arc<Mutex<JavaScriptStats>> {
        Arc::clone(&self.stats)
    }
}

#[async_trait]
impl CompilerPass for JavaScriptAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(|id| id.to_string()).collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut combined = KirGraph::new();
        let mut stats = JavaScriptStats::default();
        // Dedup module target objects across files within this one run — many files can import
        // the same module; mirrors `rust_analyzer.rs`/`python_analyzer.rs`/`elixir_analyzer.rs`'s
        // own module dedup discipline.
        let mut seen_modules: HashSet<KirId> = HashSet::new();

        for artifact_id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics
                        .lock()
                        .unwrap()
                        .warning("JS001", format!("cannot read artifact {artifact_id}: {e}"));
                    continue;
                }
            };
            let data: JsArtifactData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "JS002",
                        format!("malformed javascript payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            let id_path =
                ekos_common::project::project_qualify(&data.path, data.project.as_deref());
            let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, id_path.as_bytes()));

            let allocator = Allocator::default();
            let source_type = javascript_source_type(&data.path);
            let parsed = Parser::new(&allocator, &data.source, source_type).parse();
            if parsed.panicked {
                stats.files_failed_to_parse += 1;
                ctx.diagnostics.lock().unwrap().warning(
                    "JS003",
                    format!("cannot parse {} — unrecoverable syntax error", data.path),
                );
                continue;
            }

            let result = extract_javascript_file(&parsed.program, file_id, data.project.as_deref());

            stats.files_processed += 1;
            stats.modules_total += result.module_count;
            stats.symbols_total += result.symbol_count;

            for obj in result.objects {
                if seen_modules.insert(obj.id)
                    || !matches!(obj.kind, ObjectKind::Custom(ref k) if k == "JsModule")
                {
                    combined.add_object(obj);
                }
            }
            for rel in result.relationships {
                combined.add_relationship(rel);
            }
        }

        *self.stats.lock().unwrap() = stats;

        if combined.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], combined);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            files = stats.files_processed,
            failed = stats.files_failed_to_parse,
            modules = stats.modules_total,
            symbols = stats.symbols_total,
            "javascript-analyzer complete"
        );
        Ok(())
    }
}

/// `SourceType::from_path` alone only enables JSX for `.jsx`/`.tsx` — but real-world `.js` files
/// very often contain real JSX with no `.jsx` extension (confirmed live against the real
/// analytics project: 16 real `.js` files under `assets/js/dashboard/` use JSX directly, e.g.
/// `lazy-loader.js`'s `return (<div ref={ref} ...>...)`), and every real JS bundler/loader this
/// session has seen treats `.js` as JSX-permissive by default. Force JSX on for JavaScript
/// (`.js`/`.jsx`/`.mjs`/`.cjs`) — always a safe superset, a file with no JSX in it parses
/// identically either way. Left off for TypeScript (`.ts` stays extension-derived, `.tsx` already
/// gets it from `from_path`): unlike JavaScript, `.ts`'s old-style generic type assertion syntax
/// (`<T>expr`) is genuinely ambiguous with a JSX element — real TypeScript tooling deliberately
/// keeps `.ts` non-JSX for exactly this reason, so guessing JSX on for `.ts` risks the opposite
/// failure mode this fix targets.
fn javascript_source_type(path: &str) -> SourceType {
    let source_type = SourceType::from_path(path).unwrap_or_default();
    if source_type.is_javascript() {
        source_type.with_jsx(true)
    } else {
        source_type
    }
}

// ── Deterministic ids ────────────────────────────────────────────────────────

fn js_module_kir_id(qualified_specifier: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("js-module:{qualified_specifier}").as_bytes(),
    ))
}

fn js_symbol_kir_id(owner: KirId, qualified_name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("js-symbol:{owner}:{qualified_name}").as_bytes(),
    ))
}

// ── Extraction ───────────────────────────────────────────────────────────────

#[derive(Default)]
struct JsFileResult {
    objects: Vec<KirObject>,
    relationships: Vec<KirRelationship>,
    module_count: usize,
    symbol_count: usize,
}

struct FileCtx<'a> {
    file_id: KirId,
    project: Option<&'a str>,
    seen: HashSet<KirId>,
    imported_specifiers: HashSet<String>,
    /// Real `/** ... */` JSDoc text (Phase 1 of the "Real Descriptions, Purpose, and Links"
    /// plan), keyed by `Comment::attached_to` — the start offset of the token the leading comment
    /// precedes. Built once per file in [`extract_javascript_file`].
    docs_by_offset: HashMap<u32, String>,
}

/// Real JSDoc extraction — `oxc_parser` already classifies `program.comments` by
/// `CommentContent::Jsdoc` (a real `/** ... */` block, not just any comment), and gives each
/// comment's `attached_to` offset — the exact start of the token it precedes. No re-parsing of
/// comment syntax needed, just cleanup: strip the `/**`/`*/` delimiters and each line's leading
/// `*` (the real, near-universal JSDoc convention), joined into one real description.
fn extract_jsdoc_by_offset(program: &Program) -> HashMap<u32, String> {
    let mut docs = HashMap::new();
    for comment in &program.comments {
        if comment.content != CommentContent::Jsdoc {
            continue;
        }
        let raw = &program.source_text[comment.span.start as usize..comment.span.end as usize];
        let inner = raw
            .strip_prefix("/**")
            .unwrap_or(raw)
            .strip_suffix("*/")
            .unwrap_or(raw);
        let text = inner
            .lines()
            .map(|l| l.trim().trim_start_matches('*').trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !text.is_empty() {
            docs.insert(comment.attached_to, text);
        }
    }
    docs
}

fn extract_javascript_file(
    program: &Program,
    file_id: KirId,
    project: Option<&str>,
) -> JsFileResult {
    let mut result = JsFileResult::default();
    let mut fc = FileCtx {
        file_id,
        project,
        seen: HashSet::new(),
        imported_specifiers: HashSet::new(),
        docs_by_offset: extract_jsdoc_by_offset(program),
    };

    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(decl) => {
                handle_import(decl.source.value.as_str(), &mut fc, &mut result);
            }
            Statement::ExportNamedDeclaration(decl) => {
                if let Some(declaration) = &decl.declaration {
                    handle_declaration(declaration, true, decl.span.start, &mut fc, &mut result);
                }
            }
            Statement::ExportDefaultDeclaration(decl) => match &decl.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                    handle_function(f, true, decl.span.start, &mut fc, &mut result);
                }
                ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                    handle_class(c, true, decl.span.start, &mut fc, &mut result);
                }
                _ => {}
            },
            Statement::FunctionDeclaration(f) => {
                handle_function(f, false, f.span.start, &mut fc, &mut result);
            }
            Statement::ClassDeclaration(c) => {
                handle_class(c, false, c.span.start, &mut fc, &mut result);
            }
            Statement::VariableDeclaration(v) => {
                handle_variable_declaration(v, false, v.span.start, &mut fc, &mut result);
            }
            _ => {}
        }
    }

    result
}

fn handle_declaration(
    decl: &Declaration,
    exported: bool,
    doc_anchor: u32,
    fc: &mut FileCtx,
    result: &mut JsFileResult,
) {
    match decl {
        Declaration::FunctionDeclaration(f) => handle_function(f, exported, doc_anchor, fc, result),
        Declaration::ClassDeclaration(c) => handle_class(c, exported, doc_anchor, fc, result),
        Declaration::VariableDeclaration(v) => {
            handle_variable_declaration(v, exported, doc_anchor, fc, result)
        }
        _ => {}
    }
}

fn handle_import(specifier: &str, fc: &mut FileCtx, result: &mut JsFileResult) {
    if !fc.imported_specifiers.insert(specifier.to_string()) {
        return;
    }
    let qualified = ekos_common::project::project_qualify(specifier, fc.project);
    let target_id = js_module_kir_id(&qualified);
    if fc.seen.insert(target_id) {
        let mut obj = KirObject::new(
            specifier.to_string(),
            ObjectKind::Custom("JsModule".to_string()),
        );
        obj.id = target_id;
        result.objects.push(obj);
        result.module_count += 1;
    }
    result.relationships.push(KirRelationship::deterministic(
        RelationshipKind::DependsOn,
        fc.file_id,
        target_id,
        "",
    ));
}

fn emit_symbol(
    name: &str,
    kind: &str,
    exported: bool,
    doc_anchor: u32,
    fc: &mut FileCtx,
    result: &mut JsFileResult,
) {
    let qualified = ekos_common::project::project_qualify(name, fc.project);
    let sym_id = js_symbol_kir_id(fc.file_id, &qualified);
    if fc.seen.insert(sym_id) {
        let mut obj = KirObject::new(name.to_string(), ObjectKind::Custom("JsSymbol".to_string()))
            .with_property("kind", serde_json::json!(kind))
            .with_property(
                "visibility",
                serde_json::json!(if exported { "exported" } else { "local" }),
            );
        obj.id = sym_id;
        // Real, only when a real `/** ... */` JSDoc comment actually precedes this exact
        // declaration — never fabricated.
        if let Some(doc) = fc.docs_by_offset.get(&doc_anchor) {
            obj.properties
                .insert("description".into(), serde_json::json!(doc));
        }
        result.objects.push(obj);
        result.symbol_count += 1;
    }
    result.relationships.push(KirRelationship::deterministic(
        RelationshipKind::Contains,
        fc.file_id,
        sym_id,
        "",
    ));
}

fn handle_function(
    f: &Function,
    exported: bool,
    doc_anchor: u32,
    fc: &mut FileCtx,
    result: &mut JsFileResult,
) {
    if let Some(id) = &f.id {
        emit_symbol(
            id.name.as_str(),
            "function",
            exported,
            doc_anchor,
            fc,
            result,
        );
    }
}

fn handle_class(
    c: &Class,
    exported: bool,
    doc_anchor: u32,
    fc: &mut FileCtx,
    result: &mut JsFileResult,
) {
    if let Some(id) = &c.id {
        emit_symbol(id.name.as_str(), "class", exported, doc_anchor, fc, result);
    }
}

fn handle_variable_declaration(
    decl: &VariableDeclaration,
    exported: bool,
    doc_anchor: u32,
    fc: &mut FileCtx,
    result: &mut JsFileResult,
) {
    for declarator in &decl.declarations {
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            continue;
        };
        let is_function_valued = matches!(
            &declarator.init,
            Some(Expression::ArrowFunctionExpression(_)) | Some(Expression::FunctionExpression(_))
        );
        if is_function_valued {
            emit_symbol(
                binding.name.as_str(),
                "function",
                exported,
                doc_anchor,
                fc,
                result,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_span::SourceType;

    fn extract(source: &str, path: &str) -> JsFileResult {
        let allocator = Allocator::default();
        let source_type = javascript_source_type(path);
        let ret = Parser::new(&allocator, source, source_type).parse();
        assert!(
            !ret.panicked,
            "test fixture failed to parse: {:?}",
            ret.errors
        );
        let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, path.as_bytes()));
        extract_javascript_file(&ret.program, file_id, None)
    }

    #[test]
    fn recognizes_a_top_level_function_declaration() {
        let result = extract("function greet(name) {}\n", "a.js");
        assert_eq!(result.symbol_count, 1);
        let sym = &result.objects[0];
        assert_eq!(sym.name, "greet");
        assert_eq!(sym.properties["kind"], "function");
        assert_eq!(sym.properties["visibility"], "local");
    }

    #[test]
    fn a_real_jsdoc_comment_becomes_a_real_description() {
        let result = extract(
            "/**\n * Hashes a password using bcrypt.\n */\nfunction hash(pw) {}\n",
            "a.js",
        );
        assert_eq!(
            result.objects[0].properties["description"],
            "Hashes a password using bcrypt."
        );
    }

    #[test]
    fn a_jsdoc_comment_on_an_exported_function_still_attaches_correctly() {
        let result = extract(
            "/**\n * Renders the dashboard.\n */\nexport function Dashboard() {}\n",
            "a.js",
        );
        assert_eq!(
            result.objects[0].properties["description"],
            "Renders the dashboard."
        );
    }

    #[test]
    fn a_jsdoc_comment_on_an_arrow_function_component_attaches_to_the_const() {
        let result = extract(
            "/**\n * A lazy-loading wrapper.\n */\nexport const LazyLoader = () => null;\n",
            "a.jsx",
        );
        assert_eq!(
            result.objects[0].properties["description"],
            "A lazy-loading wrapper."
        );
    }

    #[test]
    fn a_plain_line_comment_is_not_mistaken_for_a_real_jsdoc_comment() {
        let result = extract("// just a regular comment\nfunction f() {}\n", "a.js");
        assert!(!result.objects[0].properties.contains_key("description"));
    }

    #[test]
    fn a_function_with_no_real_jsdoc_has_no_description_property_at_all() {
        let result = extract("function plain() {}\n", "a.js");
        assert!(!result.objects[0].properties.contains_key("description"));
    }

    #[test]
    fn an_exported_function_is_tagged_exported() {
        let result = extract("export function Greeting(name) {}\n", "a.js");
        assert_eq!(result.objects[0].properties["visibility"], "exported");
    }

    #[test]
    fn a_default_exported_function_is_tagged_exported() {
        let result = extract("export default function App() {}\n", "a.js");
        assert_eq!(result.objects[0].name, "App");
        assert_eq!(result.objects[0].properties["visibility"], "exported");
    }

    #[test]
    fn recognizes_a_class_declaration() {
        let result = extract("class Widget {}\n", "a.js");
        assert_eq!(result.objects[0].name, "Widget");
        assert_eq!(result.objects[0].properties["kind"], "class");
    }

    #[test]
    fn recognizes_an_arrow_function_component_assigned_to_const() {
        let result = extract("export const App = () => { return null; };\n", "a.jsx");
        assert_eq!(result.objects[0].name, "App");
        assert_eq!(result.objects[0].properties["kind"], "function");
        assert_eq!(result.objects[0].properties["visibility"], "exported");
    }

    #[test]
    fn a_plain_non_function_const_is_not_surfaced_as_a_symbol() {
        let result = extract("export const MAX_RETRIES = 3;\n", "a.js");
        assert_eq!(result.symbol_count, 0);
    }

    #[test]
    fn imports_become_real_depends_on_edges_deduped_per_specifier() {
        let result = extract(
            "import React from \"react\";\nimport { useState } from \"react\";\n",
            "a.jsx",
        );
        assert_eq!(
            result.module_count, 1,
            "one distinct specifier -> one module object"
        );
        assert_eq!(result.objects[0].name, "react");
        assert_eq!(
            result
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::DependsOn)
                .count(),
            1
        );
    }

    #[test]
    fn typescript_syntax_parses_via_extension_detected_source_type() {
        let result = extract(
            "interface Props { name: string }\nexport function Greeting(props: Props) {}\n",
            "a.ts",
        );
        assert_eq!(result.objects[0].name, "Greeting");
    }

    /// Real, found-in-production regression: `SourceType::from_path` alone only enables JSX for
    /// `.jsx`/`.tsx`, but real `.js` files in the wild (confirmed against the real analytics
    /// project's `assets/js/dashboard/` — 16 real files) contain real JSX with no `.jsx`
    /// extension. Before `javascript_source_type`'s fix, this exact shape failed to parse.
    #[test]
    fn a_plain_js_file_containing_real_jsx_parses_successfully() {
        let result = extract(
            "export default function Widget(props) {\n  return (<div className=\"x\">{props.children}</div>);\n}\n",
            "widget.js",
        );
        assert_eq!(result.objects[0].name, "Widget");
    }

    #[test]
    fn typescript_files_stay_non_jsx_to_avoid_the_generic_assertion_ambiguity() {
        assert!(!javascript_source_type("a.ts").is_jsx());
        assert!(javascript_source_type("a.tsx").is_jsx());
        assert!(javascript_source_type("a.js").is_jsx());
        assert!(javascript_source_type("a.jsx").is_jsx());
    }

    #[test]
    fn a_project_field_qualifies_ids_but_not_displayed_names() {
        let allocator = Allocator::default();
        let source_type = SourceType::from_path("a.js").unwrap_or_default();
        let ret = Parser::new(
            &allocator,
            "import x from \"shared\";\nfunction f() {}\n",
            source_type,
        )
        .parse();
        let file_a = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, b"proj-a:a.js"));
        let file_b = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, b"proj-b:a.js"));
        let a = extract_javascript_file(&ret.program, file_a, Some("proj-a"));
        let b = extract_javascript_file(&ret.program, file_b, Some("proj-b"));
        let module_a = a.objects.iter().find(|o| o.name == "shared").unwrap();
        let module_b = b.objects.iter().find(|o| o.name == "shared").unwrap();
        assert_eq!(module_a.name, "shared");
        assert_ne!(module_a.id, module_b.id);
    }

    #[test]
    fn a_malformed_file_does_not_panic_the_caller() {
        // oxc's parser has real error recovery for most malformed input rather than a hard
        // panic; this test documents that `extract_javascript_file` itself never panics given
        // whatever partial AST the parser recovers, real or degenerate.
        let allocator = Allocator::default();
        let source_type = SourceType::from_path("a.js").unwrap_or_default();
        let ret = Parser::new(&allocator, "function (", source_type).parse();
        let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, b"a.js"));
        let _ = extract_javascript_file(&ret.program, file_id, None);
    }
}
