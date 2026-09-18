//! `PerlAnalyzerPass` — structural extraction of Perl source (RFC 0147) into plain KIR objects
//! and relationships, replacing `plugins/file`'s crude declaration-prefix symbol scan for
//! `.pl`/`.pm`/`.t`/`.psgi`/`.cgi` files — bare name strings with no package, no relationships,
//! no spans, no descriptions.
//!
//! Perl is the motivating case for RFC 0147 because it is the language of the estate this
//! compiler is actually pointed at: LedgerSMB (a Perl/PostgreSQL ERP) had its 158 tables and 214
//! foreign keys compiled by RFC 0146, while the ~700 Perl files that read and write them stayed
//! invisible.
//!
//! **No mature Perl-grammar Rust crate exists, and none can.** Perl is not statically parseable —
//! `BEGIN` blocks and prototypes change the grammar mid-parse, which is why the only complete
//! Perl parser is `perl` itself. Shelling out to `perl -MO=Deparse`/`PPI` was rejected in the RFC:
//! it would require a Perl toolchain wherever `ekos recover` runs, break reproducible builds, and
//! execute `BEGIN` blocks from observed code. So this is a real, bounded, hand-written structural
//! scanner — the same "read what's declared, don't build a resolver" spirit `elixir_analyzer.rs`
//! states for its own language.
//!
//! Scope, deliberately narrow and honest rather than a claim of full parsing:
//! - `package Foo::Bar;` and `package Foo::Bar { ... }` become `Custom("PerlPackage")` + a
//!   `Contains` edge from the owning `File`.
//! - `sub name { ... }` becomes `Custom("PerlSymbol")` (`symbol_kind: "sub"`) + a `Contains` edge
//!   from the owning package — or from the `File` directly when the file declares no package, the
//!   very common plain-script shape for `.pl`/`.t`. `visibility` is `"private"` for a leading `_`
//!   and `"public"` otherwise: that naming convention is the only visibility signal Perl actually
//!   has, since the language has no such keyword.
//! - `use Foo::Bar;`/`require Foo::Bar;` become real `DependsOn` edges to a `PerlPackage` built
//!   with the *same* deterministic id the declaration uses, so a package declared in one file and
//!   used in another resolves onto one real object rather than two disconnected ones.
//!   **Pragmas are excluded**: Perl's own convention reserves all-lowercase module names for
//!   pragmas (`strict`, `warnings`, `utf8`, `constant`, …), and a bare `use v5.36;` is a version
//!   assertion. Emitting those would bury every real edge under `strict`/`warnings` noise on
//!   every single file.
//! - `use parent`/`use base`/`our @ISA = (...)`/`push @ISA, ...` become real
//!   `RelationshipKind::Extends` edges — `parent` and `base` are the two lowercase `use` targets
//!   deliberately exempted from the pragma filter, because they carry a real structural
//!   relationship rather than a directive. Unlike RFC 0092's same-file-only Python inheritance,
//!   the shared package-id scheme resolves these across files.
//! - Real POD becomes a real `description`, never a fabricated one: a `=head1 DESCRIPTION`/`NAME`
//!   block describes the file's first package, and a `=head2`/`=head3`/`=item` heading describes
//!   the sub it names — matched either by sitting immediately above it (interleaved POD) or by
//!   naming it unambiguously (the trailing `=head1 METHODS` convention). See [`extract_pod`].
//! - Not a call graph. Perl's dispatch is fully dynamic (`$obj->$method()`, `AUTOLOAD`, symbol
//!   table manipulation, string `eval`), so a static call graph would be confidently wrong on
//!   real code — the same scope decision `elixir_analyzer.rs`/`python_analyzer.rs`/
//!   `javascript_analyzer.rs` each made for their own languages.
//!
//! **Documented limitations.** Comments are stripped by a quote-aware scan, and POD blocks plus
//! `__END__`/`__DATA__` sections are skipped before structural scanning. The scanner is *not*
//! aware of heredocs, `q{}`/`qq{}` bracket-quoting with embedded braces, regex literals
//! containing braces, or string `eval`. A brace inside one of those can desynchronize depth
//! tracking for the rest of that one file — an accepted, documented tradeoff matching
//! `elixir_analyzer.rs`'s own `do`/`end` note. It degrades one file's spans; it never fails the
//! pass.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct PerlArtifactData {
    path: String,
    source: String,
    /// RFC 0079: present only in a multi-`[observe] paths` workspace (`build.rs`'s own choke
    /// point). Qualifies id hashing only — `path` stays bare everywhere it's displayed.
    #[serde(default)]
    project: Option<String>,
}

/// Coverage counters from one run, mirroring `ElixirStats`/`JavaScriptStats`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PerlStats {
    pub files_processed: usize,
    pub packages_total: usize,
    pub symbols_total: usize,
}

pub struct PerlAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<PerlStats>>,
}

impl PerlAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("perl-analyzer:{}", workspace_name.into()),
            artifact_ids,
            stats: Arc::new(Mutex::new(PerlStats::default())),
        }
    }

    pub fn stats_handle(&self) -> Arc<Mutex<PerlStats>> {
        Arc::clone(&self.stats)
    }
}

#[async_trait]
impl CompilerPass for PerlAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    /// Bump on any change to this pass's output shape — see `rust_analyzer`'s `version` for why
    /// `cache_inputs` alone cannot catch a logic change, and what it cost when it didn't.
    fn version(&self) -> &str {
        "v1"
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(|id| id.to_string()).collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut combined = KirGraph::new();
        let mut stats = PerlStats::default();
        // Dedup package target objects across files within this one run — many files `use` the
        // same package, and the package's own declaring file contributes a third occurrence.
        // Unlike `elixir_analyzer.rs`'s straight "first one wins", these occurrences do *not* all
        // have the identical shape: only the declaring file's own object can carry a POD
        // `description`, and `use`d references are always thin. Whichever arrives first therefore
        // has to absorb what the others know, or a real module description is lost purely to
        // artifact iteration order.
        let mut package_at: HashMap<KirId, usize> = HashMap::new();

        for artifact_id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "PERL001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let data: PerlArtifactData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "PERL002",
                        format!("malformed perl payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            let id_path =
                ekos_common::project::project_qualify(&data.path, data.project.as_deref());
            let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, id_path.as_bytes()));
            let result = parse_perl_file(&data.source, file_id, data.project.as_deref());

            stats.files_processed += 1;
            stats.packages_total += result.package_count;
            stats.symbols_total += result.symbol_count;

            for mut obj in result.objects {
                // RFC 0140 §1 — a symbol's span is useless without the file it belongs to.
                crate::source_evidence::attach(&mut obj, &data.source, &data.path, &mut combined);
                let is_package = matches!(&obj.kind, ObjectKind::Custom(k) if k == "PerlPackage");
                if !is_package {
                    combined.add_object(obj);
                    continue;
                }
                match package_at.get(&obj.id) {
                    Some(&idx) => absorb_package(&mut combined.objects[idx], obj),
                    None => {
                        package_at.insert(obj.id, combined.objects.len());
                        combined.add_object(obj);
                    }
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
            packages = stats.packages_total,
            symbols = stats.symbols_total,
            "perl-analyzer complete"
        );
        Ok(())
    }
}

/// Fold a second occurrence of one package into the one already kept, so the declaring file's
/// real POD description survives regardless of which file this run happened to read first.
/// Properties and evidence already present are never overwritten — only gaps are filled.
fn absorb_package(kept: &mut KirObject, other: KirObject) {
    for (k, v) in other.properties {
        kept.properties.entry(k).or_insert(v);
    }
    for ev in other.evidence {
        if !kept.evidence.contains(&ev) {
            kept.evidence.push(ev);
        }
    }
}

// ── Deterministic ids ────────────────────────────────────────────────────────

fn perl_package_kir_id(qualified_name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("perl-package:{qualified_name}").as_bytes(),
    ))
}

fn perl_symbol_kir_id(owner: KirId, qualified_name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("perl-symbol:{owner}:{qualified_name}").as_bytes(),
    ))
}

// ── POD ──────────────────────────────────────────────────────────────────────

/// Real POD text recovered from one file.
#[derive(Default)]
struct PodDocs {
    /// `=head1 DESCRIPTION` (preferred) or `=head1 NAME` — describes the file's first package.
    package_doc: Option<String>,
    /// A block's trailing `=head2`/`=head3`/`=item` body, keyed by the 0-indexed line of the
    /// first code line the block sits immediately above. The interleaved-POD convention.
    adjacent: HashMap<usize, String>,
    /// Every `=head2`/`=head3`/`=item` body keyed by the sub name its heading normalizes to.
    /// `None` marks a name documented by more than one heading — ambiguous, so unused rather
    /// than guessed at. Covers the trailing `=head1 METHODS` convention, where the POD is at the
    /// bottom of the file and adjacent to nothing.
    by_name: HashMap<String, Option<String>>,
}

/// Whether a line opens a POD block. Per `perlpod`, a POD directive must start at column 0 — an
/// indented `=` is ordinary code (`$x =~ ...` inside a block, say), not documentation.
fn is_pod_start(line: &str) -> bool {
    line.starts_with('=') && line[1..].starts_with(|c: char| c.is_ascii_alphabetic())
}

/// A heading's sub name: POD formatting codes unwrapped (`=item B<new>`), then the leading
/// identifier alone (`=head2 new($class, %args)` → `new`). Empty for a heading that doesn't name
/// anything (`=item *` bullets, `=head2 Constructors`) — those simply never match a sub.
fn heading_name(text: &str) -> String {
    let mut t = text.trim();
    // `B<new>` / `C<new>` / `I<new>` — the three formatting codes real Perl POD wraps method
    // names in. Only the outermost wrapper is unwound; nesting these around a method name is not
    // a convention that occurs.
    for code in ['B', 'C', 'I'] {
        if let Some(rest) = t.strip_prefix(&format!("{code}<"))
            && let Some(inner) = rest.strip_suffix('>')
        {
            t = inner.trim();
            break;
        }
    }
    t.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Real POD extraction, pre-scanned separately from the main structural loop because a POD block
/// spans many lines and the main loop has no lookahead.
///
/// Two matching rules, because real Perl uses two conventions and honouring only one would leave
/// the other's modules looking undocumented:
/// - **Adjacent** — a `=head2 foo ... =cut` block sitting directly above `sub foo`. Recorded by
///   line, so it attaches to whatever declaration actually follows it.
/// - **By name** — the `=head1 METHODS` block at the bottom of a module, documenting every method
///   in one place, adjacent to no code at all. Used only when exactly one heading in the file
///   normalizes to that sub's name; a name documented twice is left undocumented rather than
///   resolved by guessing which heading was meant.
///
/// Adjacent wins where both exist: it is the more specific signal.
fn extract_pod(lines: &[&str]) -> PodDocs {
    let mut docs = PodDocs::default();
    // `=head1 NAME`'s body is the one-line "Module - blurb" summary; `=head1 DESCRIPTION`'s is the
    // real prose. DESCRIPTION therefore wins — held separately rather than resolved in place,
    // because nothing guarantees a file puts its NAME block first.
    let mut name_doc: Option<String> = None;
    let mut i = 0;
    while i < lines.len() {
        if !is_pod_start(lines[i]) {
            i += 1;
            continue;
        }
        // Collect this block's (directive, body) pairs until `=cut` or EOF.
        let mut headings: Vec<(String, String)> = Vec::new();
        let mut current: Option<(String, Vec<&str>)> = None;
        while i < lines.len() {
            let line = lines[i];
            if line.starts_with("=cut") {
                i += 1;
                break;
            }
            if is_pod_start(line) {
                if let Some((d, body)) = current.take() {
                    headings.push((d, join_pod_body(&body)));
                }
                current = Some((line.to_string(), Vec::new()));
            } else if let Some((_, body)) = current.as_mut() {
                body.push(line);
            }
            i += 1;
        }
        if let Some((d, body)) = current.take() {
            headings.push((d, join_pod_body(&body)));
        }

        let mut last_member: Option<String> = None;
        for (directive, body) in &headings {
            let Some((tag, text)) = directive.split_once(char::is_whitespace) else {
                continue;
            };
            let text = text.trim();
            match tag {
                "=head1" if !body.is_empty() => match text.to_ascii_uppercase().as_str() {
                    "DESCRIPTION" => docs.package_doc = Some(body.clone()),
                    "NAME" => name_doc = Some(body.clone()),
                    _ => {}
                },
                "=head2" | "=head3" | "=item" => {
                    let name = heading_name(text);
                    if name.is_empty() || body.is_empty() {
                        last_member = None;
                        continue;
                    }
                    // Second sighting of a name makes it ambiguous, permanently.
                    docs.by_name
                        .entry(name.clone())
                        .and_modify(|slot| *slot = None)
                        .or_insert_with(|| Some(body.clone()));
                    last_member = Some(body.clone());
                }
                _ => {}
            }
        }

        // The first real code line below this block — what an interleaved `=head2 foo ... =cut`
        // is documenting.
        if let Some(body) = last_member {
            let mut j = i;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            if j < lines.len() {
                docs.adjacent.insert(j, body);
            }
        }
    }
    docs.package_doc = docs.package_doc.or(name_doc);
    docs
}

fn join_pod_body(body: &[&str]) -> String {
    body.iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

// ── Parsing ──────────────────────────────────────────────────────────────────

#[derive(Default)]
struct PerlFileResult {
    objects: Vec<KirObject>,
    relationships: Vec<KirRelationship>,
    package_count: usize,
    symbol_count: usize,
}

/// One open brace on the depth stack. `Package` carries the id so later `sub`/`use` lines can
/// find the innermost block-form package regardless of unrelated nesting in between; `Sub` marks
/// a sub body so its closing brace can complete a real `source_span`.
enum Block {
    Package(KirId),
    Sub(KirId),
    Other,
}

/// The package a declaration at this point belongs to: the innermost open `package NAME { ... }`
/// block, else the most recent statement-form `package NAME;` (whose scope in practice runs to
/// the end of the file), else nothing — a plain script, where the `File` itself is the owner.
fn current_package(stack: &[Block], statement_package: Option<KirId>) -> Option<KirId> {
    stack
        .iter()
        .rev()
        .find_map(|b| match b {
            Block::Package(id) => Some(*id),
            Block::Sub(_) | Block::Other => None,
        })
        .or(statement_package)
}

fn parse_perl_file(source: &str, file_id: KirId, project: Option<&str>) -> PerlFileResult {
    let lines: Vec<&str> = source.lines().collect();
    let pod = extract_pod(&lines);

    let mut result = PerlFileResult::default();
    let mut stack: Vec<Block> = Vec::new();
    let mut seen: HashSet<KirId> = HashSet::new();
    let mut statement_package: Option<KirId> = None;
    let mut first_package_obj: Option<usize> = None;
    // Armed by a `package NAME {` / `sub NAME` line and consumed by the very next `{` the generic
    // brace scan sees, which may be on a later line (a signature or prototype can wrap). Arming
    // unconditionally overwrites any earlier unconsumed value, so a bodyless forward declaration
    // (`sub NAME;`) can never wrongly attach to some later, unrelated block.
    let mut pending: Option<Block> = None;
    let mut pending_sub_start: usize = 0;
    let mut symbol_spans: HashMap<KirId, (usize, usize)> = HashMap::new();
    let mut in_pod = false;

    for (line_idx, raw_line) in lines.iter().enumerate() {
        // POD blocks and everything past `__END__`/`__DATA__` are documentation and data, not
        // code — skipped wholesale so their prose can never be mistaken for a declaration or
        // desynchronize brace depth.
        if in_pod {
            if raw_line.starts_with("=cut") {
                in_pod = false;
            }
            continue;
        }
        if is_pod_start(raw_line) {
            in_pod = true;
            continue;
        }
        let trimmed_raw = raw_line.trim();
        if trimmed_raw == "__END__" || trimmed_raw == "__DATA__" {
            break;
        }

        let line = strip_comment(raw_line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("package ") {
            if let Some(name) = extract_package_name(rest) {
                let qualified = ekos_common::project::project_qualify(&name, project);
                let pkg_id = perl_package_kir_id(&qualified);
                if seen.insert(pkg_id) {
                    let mut obj =
                        KirObject::new(name, ObjectKind::Custom("PerlPackage".to_string()));
                    obj.id = pkg_id;
                    if first_package_obj.is_none() {
                        first_package_obj = Some(result.objects.len());
                    }
                    result.objects.push(obj);
                    result.package_count += 1;
                }
                result.relationships.push(KirRelationship::deterministic(
                    RelationshipKind::Contains,
                    file_id,
                    pkg_id,
                    "",
                ));
                // Block form (`package Foo { ... }`) is brace-scoped; statement form
                // (`package Foo;`) runs on until the next `package` statement.
                if trimmed.contains('{') {
                    pending = Some(Block::Package(pkg_id));
                } else {
                    statement_package = Some(pkg_id);
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("sub ") {
            if let Some(name) = extract_sub_name(rest) {
                let owner = current_package(&stack, statement_package).unwrap_or(file_id);
                let qualified = ekos_common::project::project_qualify(&name, project);
                let sym_id = perl_symbol_kir_id(owner, &qualified);
                if seen.insert(sym_id) {
                    let mut obj =
                        KirObject::new(name.clone(), ObjectKind::Custom("PerlSymbol".to_string()))
                            .with_property("symbol_kind", serde_json::json!("sub"))
                            .with_property(
                                "visibility",
                                // Perl has no visibility keyword at all; a leading underscore is the
                                // language's real, near-universal "internal" convention and the only
                                // signal there is to read.
                                serde_json::json!(if name.starts_with('_') {
                                    "private"
                                } else {
                                    "public"
                                }),
                            )
                            .with_property("signature", serde_json::json!(perl_signature(trimmed)));
                    obj.id = sym_id;
                    if let Some(doc) = pod
                        .adjacent
                        .get(&line_idx)
                        .or_else(|| pod.by_name.get(&name).and_then(|slot| slot.as_ref()))
                    {
                        obj.properties
                            .insert("description".into(), serde_json::json!(doc));
                    }
                    result.objects.push(obj);
                    result.symbol_count += 1;
                }
                result.relationships.push(KirRelationship::deterministic(
                    RelationshipKind::Contains,
                    owner,
                    sym_id,
                    "",
                ));
                pending = Some(Block::Sub(sym_id));
                pending_sub_start = line_idx + 1;
            }
        } else if let Some(targets) = extract_dependency_targets(trimmed) {
            let owner = current_package(&stack, statement_package).unwrap_or(file_id);
            for (name, kind) in targets {
                let qualified = ekos_common::project::project_qualify(&name, project);
                let target_id = perl_package_kir_id(&qualified);
                if seen.insert(target_id) {
                    let mut obj =
                        KirObject::new(name, ObjectKind::Custom("PerlPackage".to_string()));
                    obj.id = target_id;
                    result.objects.push(obj);
                }
                result
                    .relationships
                    .push(KirRelationship::deterministic(kind, owner, target_id, ""));
            }
        }

        adjust_depth(
            &mut stack,
            trimmed,
            line_idx,
            pending_sub_start,
            &mut pending,
            &mut symbol_spans,
        );
    }

    // RFC 0088: apply the now-fully-resolved source spans onto their matching symbol objects —
    // deferred to the end (rather than written at push time) because a span's end line isn't
    // known until its closing brace is reached, many lines after the object was created.
    for obj in &mut result.objects {
        if let Some(&(start, end)) = symbol_spans.get(&obj.id) {
            obj.properties.insert(
                "source_span".into(),
                serde_json::json!({"start_line": start, "end_line": end}),
            );
        }
    }
    // A file-level `=head1 DESCRIPTION`/`NAME` describes the module the file declares — Perl's
    // universal one-package-per-`.pm` convention. Applied to the *first* package only: a file
    // declaring several has no way to say which one the header meant.
    if let (Some(doc), Some(idx)) = (pod.package_doc.as_ref(), first_package_obj) {
        result.objects[idx]
            .properties
            .insert("description".into(), serde_json::json!(doc));
    }

    result
}

/// Track `{`/`}` nesting, consuming an armed `pending` block on the next opening brace and
/// recording a real `source_span` when a `Sub` block closes.
fn adjust_depth(
    stack: &mut Vec<Block>,
    line: &str,
    line_idx: usize,
    pending_sub_start: usize,
    pending: &mut Option<Block>,
    spans: &mut HashMap<KirId, (usize, usize)>,
) {
    for c in unquoted_chars(line) {
        match c {
            '{' => stack.push(pending.take().unwrap_or(Block::Other)),
            '}' => {
                if let Some(Block::Sub(id)) = stack.pop() {
                    spans.insert(id, (pending_sub_start, line_idx + 1));
                }
            }
            _ => {}
        }
    }
}

/// The characters of `line` that sit outside a `'`/`"` string. Not heredoc-, `q{}`- or
/// regex-aware — see this module's own doc comment for why that limitation is accepted.
fn unquoted_chars(line: &str) -> impl Iterator<Item = char> + '_ {
    let mut in_string: Option<char> = None;
    let mut escaped = false;
    line.chars().filter(move |&c| {
        if let Some(q) = in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                in_string = None;
            }
            false
        } else if c == '"' || c == '\'' {
            in_string = Some(c);
            false
        } else {
            true
        }
    })
}

/// Strips a `#`-comment, but only outside a `"`/`'`-quoted string — and never when the `#` is
/// part of a `$#` sigil (`$#array`, `$#{$ref}`), which is real, common Perl for "last index of"
/// and would otherwise truncate the line at a meaningless point.
fn strip_comment(line: &str) -> &str {
    let mut in_string: Option<char> = None;
    let mut escaped = false;
    let mut prev = '\0';
    for (i, c) in line.char_indices() {
        if let Some(q) = in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                in_string = None;
            }
        } else if c == '"' || c == '\'' {
            in_string = Some(c);
        } else if c == '#' && prev != '$' {
            return &line[..i];
        }
        prev = c;
    }
    line
}

/// `Foo::Bar` from `Foo::Bar;`, `Foo::Bar {`, or `Foo::Bar 1.02;` (the `package NAME VERSION`
/// form). `None` for `package;` (a real, if rare, statement resetting to no package).
fn extract_package_name(rest: &str) -> Option<String> {
    let name: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
        .collect();
    let name = name.trim_end_matches(':').to_string();
    (!name.is_empty()).then_some(name)
}

/// `foo` from `foo {`, `foo ($x, $y) {`, `foo;`. `None` for an anonymous `sub { ... }`, which
/// names nothing and so is not a symbol anyone can refer to.
fn extract_sub_name(rest: &str) -> Option<String> {
    let name: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The declaration's own text as the symbol's `signature` (RFC 0141 §1) — a real Perl signature
/// or prototype when the code has one (`sub area($w, $h)`), just the name when it doesn't. The
/// trailing brace is dropped; nothing is invented to fill in what the source doesn't say.
fn perl_signature(line: &str) -> String {
    line.trim_end_matches(|c: char| c.is_whitespace())
        .trim_end_matches('{')
        .trim_end()
        .to_string()
}

/// Perl reserves all-lowercase module names for pragmas — compiler directives, not dependencies.
/// `parent` and `base` are the deliberate exceptions: they are lowercase by that same convention
/// but declare real inheritance, so they are handled before this test is ever reached.
fn is_pragma(name: &str) -> bool {
    !name.contains(':') && !name.chars().any(|c| c.is_ascii_uppercase())
}

/// A bare version assertion (`use v5.36;`, `use 5.010;`) rather than a module name.
fn is_version_literal(name: &str) -> bool {
    let body = name.strip_prefix('v').unwrap_or(name);
    !body.is_empty() && body.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// The dependency and inheritance edges one line declares, if any.
///
/// Returns `None` for a line that declares nothing, an empty vec for one that declares only
/// pragmas — a real distinction the caller doesn't need, but which keeps the "did this line
/// match?" test honest.
fn extract_dependency_targets(trimmed: &str) -> Option<Vec<(String, RelationshipKind)>> {
    // `use parent`/`use base`/`@ISA` — real inheritance, in all three forms Perl offers.
    for prefix in ["use parent", "use base"] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && rest.starts_with(|c: char| c.is_whitespace() || c == '(')
        {
            return Some(
                extract_module_list(rest)
                    .into_iter()
                    .map(|m| (m, RelationshipKind::Extends))
                    .collect(),
            );
        }
    }
    let isa_rest = trimmed
        .strip_prefix("our @ISA")
        .or_else(|| trimmed.strip_prefix("push @ISA"))
        .or_else(|| trimmed.strip_prefix("@ISA"));
    if let Some(rest) = isa_rest {
        return Some(
            extract_module_list(rest)
                .into_iter()
                .map(|m| (m, RelationshipKind::Extends))
                .collect(),
        );
    }

    for prefix in ["use ", "require "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':' || *c == '.')
                .collect();
            let name = name.trim_end_matches(':').trim_end_matches('.').to_string();
            if name.is_empty() || is_version_literal(&name) || is_pragma(&name) {
                return Some(Vec::new());
            }
            return Some(vec![(name, RelationshipKind::DependsOn)]);
        }
    }
    None
}

/// Module names from a `use parent`/`use base`/`@ISA` argument list — the `qw(...)` form and
/// plain quoted strings, both of which real code uses interchangeably. `-norequire` and any
/// other leading-dash option is dropped: it is a flag to `parent`, not a superclass.
fn extract_module_list(rest: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut remainder = rest;

    while let Some(pos) = remainder.find("qw") {
        let after = &remainder[pos + 2..];
        let Some(open) = after.chars().find(|c| !c.is_whitespace()) else {
            break;
        };
        let close = match open {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '<' => '>',
            c => c,
        };
        let body_start = after.find(open).map(|i| i + 1).unwrap_or(0);
        let body = &after[body_start..];
        let end = body.find(close).unwrap_or(body.len());
        out.extend(body[..end].split_whitespace().map(str::to_string));
        remainder = &body[end.min(body.len())..];
    }

    let mut chars = rest.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '\'' && c != '"' {
            continue;
        }
        let body = &rest[i + c.len_utf8()..];
        let Some(end) = body.find(c) else { break };
        out.push(body[..end].to_string());
        // Skip past the closing quote so its own character can't open a new "string".
        while let Some(&(j, _)) = chars.peek() {
            if j <= i + c.len_utf8() + end {
                chars.next();
            } else {
                break;
            }
        }
    }

    out.retain(|m| {
        !m.starts_with('-')
            && !m.is_empty()
            && m.chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
    });
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> PerlFileResult {
        let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, b"t.pm"));
        parse_perl_file(source, file_id, None)
    }

    fn find<'a>(r: &'a PerlFileResult, name: &str) -> &'a KirObject {
        r.objects
            .iter()
            .find(|o| o.name == name)
            .unwrap_or_else(|| panic!("no object named {name} in {:?}", names(r)))
    }

    fn names(r: &PerlFileResult) -> Vec<&str> {
        r.objects.iter().map(|o| o.name.as_str()).collect()
    }

    fn edges(r: &PerlFileResult, kind: RelationshipKind) -> Vec<KirId> {
        r.relationships
            .iter()
            .filter(|rel| rel.kind == kind)
            .map(|rel| rel.to)
            .collect()
    }

    // ── packages ────────────────────────────────────────────────────────────

    #[test]
    fn a_statement_form_package_becomes_a_real_object() {
        let r = parse("package LedgerSMB::Payment;\n1;\n");
        assert_eq!(r.package_count, 1);
        let p = find(&r, "LedgerSMB::Payment");
        assert!(matches!(&p.kind, ObjectKind::Custom(k) if k == "PerlPackage"));
    }

    #[test]
    fn a_block_form_package_becomes_a_real_object_and_owns_its_subs() {
        let r = parse("package Foo::Bar {\n    sub hello {\n        1;\n    }\n}\n");
        let pkg = find(&r, "Foo::Bar");
        let sub = find(&r, "hello");
        assert!(
            r.relationships
                .iter()
                .any(|rel| rel.kind == RelationshipKind::Contains
                    && rel.from == pkg.id
                    && rel.to == sub.id),
            "sub must be contained by its block-form package"
        );
    }

    #[test]
    fn a_package_with_a_version_still_parses_its_name() {
        let r = parse("package Foo::Bar 1.02;\n");
        assert_eq!(names(&r), vec!["Foo::Bar"]);
    }

    #[test]
    fn a_sub_after_a_statement_package_is_owned_by_that_package() {
        let r = parse("package Acct;\nsub post { 1 }\n");
        let pkg = find(&r, "Acct");
        let sub = find(&r, "post");
        assert!(
            r.relationships
                .iter()
                .any(|rel| rel.kind == RelationshipKind::Contains
                    && rel.from == pkg.id
                    && rel.to == sub.id)
        );
    }

    #[test]
    fn a_script_with_no_package_at_all_hangs_its_subs_off_the_file() {
        let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, b"t.pm"));
        let r = parse("use strict;\nsub main { 1 }\n");
        let sub = find(&r, "main");
        assert!(
            r.relationships
                .iter()
                .any(|rel| rel.kind == RelationshipKind::Contains
                    && rel.from == file_id
                    && rel.to == sub.id),
            "a plain .pl script's subs belong to the File itself"
        );
    }

    // ── subs ────────────────────────────────────────────────────────────────

    #[test]
    fn a_sub_carries_its_kind_visibility_and_real_signature() {
        let r = parse("sub area($w, $h) {\n    return $w * $h;\n}\n");
        let s = find(&r, "area");
        assert_eq!(s.properties["symbol_kind"], "sub");
        assert_eq!(s.properties["visibility"], "public");
        assert_eq!(s.properties["signature"], "sub area($w, $h)");
    }

    #[test]
    fn a_leading_underscore_is_the_only_visibility_signal_perl_has() {
        let r = parse("sub _internal { 1 }\n");
        assert_eq!(find(&r, "_internal").properties["visibility"], "private");
    }

    #[test]
    fn an_anonymous_sub_is_not_a_symbol() {
        let r = parse("my $cb = sub { 1 };\n");
        assert_eq!(r.symbol_count, 0);
    }

    #[test]
    fn a_multi_line_sub_body_gets_a_real_source_span() {
        let r = parse("package P;\n\nsub go {\n    my $x = 1;\n    return $x;\n}\n");
        assert_eq!(
            find(&r, "go").properties["source_span"],
            serde_json::json!({"start_line": 3, "end_line": 6})
        );
    }

    #[test]
    fn a_one_line_sub_gets_a_real_source_span_too() {
        let r = parse("sub go { 1 }\n");
        assert_eq!(
            find(&r, "go").properties["source_span"],
            serde_json::json!({"start_line": 1, "end_line": 1})
        );
    }

    #[test]
    fn a_bodyless_forward_declaration_does_not_steal_a_later_blocks_span() {
        // `sub go;` opens no block. Arming `pending` unconditionally means the next `{` seen
        // belongs to `real`, not to the forward declaration.
        let r = parse("sub go;\n\nsub real {\n    1;\n}\n");
        assert!(!find(&r, "go").properties.contains_key("source_span"));
        assert_eq!(
            find(&r, "real").properties["source_span"],
            serde_json::json!({"start_line": 3, "end_line": 5})
        );
    }

    // ── dependencies ────────────────────────────────────────────────────────

    #[test]
    fn a_use_of_a_real_module_becomes_a_depends_on_edge() {
        let r = parse("package P;\nuse LedgerSMB::Database;\n");
        let target = find(&r, "LedgerSMB::Database");
        assert!(edges(&r, RelationshipKind::DependsOn).contains(&target.id));
    }

    #[test]
    fn require_is_a_dependency_just_like_use() {
        let r = parse("require Foo::Bar;\n");
        assert!(names(&r).contains(&"Foo::Bar"));
    }

    #[test]
    fn pragmas_and_version_assertions_are_never_dependencies() {
        let r = parse(
            "use strict;\nuse warnings;\nuse utf8;\nuse constant PI => 3;\nuse v5.36;\nuse 5.010;\n",
        );
        assert!(
            r.objects.is_empty(),
            "pragmas would bury every real edge: {:?}",
            names(&r)
        );
    }

    #[test]
    fn a_used_package_and_its_own_declaration_share_one_identity() {
        // The whole point of the shared id scheme: a package declared in one file and used in
        // another must be one node, not two disconnected ones.
        let declared = parse("package Foo::Bar;\n");
        let used = parse("package Other;\nuse Foo::Bar;\n");
        assert_eq!(find(&declared, "Foo::Bar").id, find(&used, "Foo::Bar").id);
    }

    // ── inheritance ─────────────────────────────────────────────────────────

    #[test]
    fn use_parent_becomes_a_real_extends_edge() {
        let r = parse("package Child;\nuse parent 'Base::Class';\n");
        let base = find(&r, "Base::Class");
        assert!(edges(&r, RelationshipKind::Extends).contains(&base.id));
    }

    #[test]
    fn use_parent_norequire_drops_the_flag_and_keeps_the_superclass() {
        let r = parse("package Child;\nuse parent -norequire, 'Base::Class';\n");
        assert!(names(&r).contains(&"Base::Class"));
        assert!(
            !names(&r).iter().any(|n| n.starts_with('-')),
            "-norequire is a flag, not a superclass"
        );
    }

    #[test]
    fn use_base_with_qw_yields_one_edge_per_superclass() {
        let r = parse("package Child;\nuse base qw(Base::One Base::Two);\n");
        let extends = edges(&r, RelationshipKind::Extends);
        assert_eq!(extends.len(), 2);
        assert!(names(&r).contains(&"Base::One"));
        assert!(names(&r).contains(&"Base::Two"));
    }

    #[test]
    fn an_isa_assignment_becomes_a_real_extends_edge() {
        let r = parse("package Child;\nour @ISA = ('Base::Class');\n");
        let base = find(&r, "Base::Class");
        assert!(edges(&r, RelationshipKind::Extends).contains(&base.id));
    }

    #[test]
    fn pushing_onto_isa_becomes_a_real_extends_edge() {
        let r = parse("package Child;\npush @ISA, 'Base::Class';\n");
        assert!(names(&r).contains(&"Base::Class"));
    }

    // ── POD ─────────────────────────────────────────────────────────────────

    #[test]
    fn a_head1_description_describes_the_files_package() {
        let r = parse(
            "package LedgerSMB::Payment;\n\n=head1 DESCRIPTION\n\nHandles customer payments.\n\n=cut\n\n1;\n",
        );
        assert_eq!(
            find(&r, "LedgerSMB::Payment").properties["description"],
            "Handles customer payments."
        );
    }

    #[test]
    fn a_head1_name_is_used_when_there_is_no_description_block() {
        let r = parse("package Foo;\n\n=head1 NAME\n\nFoo - does a thing\n\n=cut\n");
        assert_eq!(
            find(&r, "Foo").properties["description"],
            "Foo - does a thing"
        );
    }

    #[test]
    fn a_description_block_outranks_a_name_block() {
        let r = parse(
            "package Foo;\n\n=head1 NAME\n\nFoo - blurb\n\n=head1 DESCRIPTION\n\nThe real prose.\n\n=cut\n",
        );
        assert_eq!(find(&r, "Foo").properties["description"], "The real prose.");
    }

    #[test]
    fn an_interleaved_head2_block_describes_the_sub_directly_below_it() {
        let r = parse(
            "package P;\n\n=head2 post\n\nPosts the transaction.\n\n=cut\n\nsub post { 1 }\n",
        );
        assert_eq!(
            find(&r, "post").properties["description"],
            "Posts the transaction."
        );
    }

    #[test]
    fn a_trailing_methods_block_describes_subs_by_name() {
        // The other real Perl convention: all POD at the bottom, adjacent to nothing.
        let r = parse(
            "package P;\n\nsub post { 1 }\n\nsub void { 1 }\n\n=head1 METHODS\n\n=head2 post\n\nPosts it.\n\n=head2 void\n\nVoids it.\n\n=cut\n",
        );
        assert_eq!(find(&r, "post").properties["description"], "Posts it.");
        assert_eq!(find(&r, "void").properties["description"], "Voids it.");
    }

    #[test]
    fn an_item_heading_with_formatting_codes_still_matches_its_sub() {
        let r = parse(
            "package P;\n\nsub new { 1 }\n\n=over\n\n=item B<new>\n\nConstructs one.\n\n=back\n\n=cut\n",
        );
        assert_eq!(find(&r, "new").properties["description"], "Constructs one.");
    }

    #[test]
    fn a_heading_naming_a_sub_with_arguments_still_matches_it() {
        let r =
            parse("package P;\n\nsub area { 1 }\n\n=head2 area($w, $h)\n\nArea of it.\n\n=cut\n");
        assert_eq!(find(&r, "area").properties["description"], "Area of it.");
    }

    #[test]
    fn a_name_documented_twice_is_left_undocumented_rather_than_guessed_at() {
        let r = parse(
            "package P;\n\nsub go { 1 }\n\n=head1 METHODS\n\n=head2 go\n\nOne meaning.\n\n=head2 go\n\nAnother meaning.\n\n=cut\n",
        );
        assert!(
            !find(&r, "go").properties.contains_key("description"),
            "an ambiguous name must not be resolved by guessing"
        );
    }

    #[test]
    fn a_sub_with_no_pod_at_all_gets_no_description_property() {
        let r = parse("package P;\nsub plain { 1 }\n");
        assert!(!find(&r, "plain").properties.contains_key("description"));
    }

    // ── skipping non-code ───────────────────────────────────────────────────

    #[test]
    fn code_shaped_prose_inside_a_pod_block_is_not_mistaken_for_a_declaration() {
        let r = parse(
            "package Real;\n\n=head1 SYNOPSIS\n\npackage Imaginary;\nsub imaginary { }\n\n=cut\n\nsub real { 1 }\n",
        );
        assert!(!names(&r).contains(&"Imaginary"));
        assert!(!names(&r).contains(&"imaginary"));
        assert!(names(&r).contains(&"real"));
    }

    #[test]
    fn everything_after_end_is_data_not_code() {
        let r = parse("package P;\nsub real { 1 }\n__END__\nsub documentation_example { }\n");
        assert!(!names(&r).contains(&"documentation_example"));
    }

    #[test]
    fn an_indented_equals_sign_is_code_not_a_pod_directive() {
        // `perlpod` requires a directive at column 0; treating an indented `=` as POD would
        // swallow the rest of the file.
        let r = parse("package P;\nsub go {\n    my $x = 1;\n}\nsub after { 1 }\n");
        assert!(names(&r).contains(&"after"));
    }

    #[test]
    fn a_hash_in_a_last_index_sigil_does_not_truncate_the_line() {
        // `$#rows` is real, common Perl for "last index of @rows" — treating its `#` as a
        // comment would drop the rest of the line, including the closing brace.
        let r = parse("sub count {\n    my $n = $#rows;\n}\nsub after { 1 }\n");
        assert_eq!(
            find(&r, "count").properties["source_span"],
            serde_json::json!({"start_line": 1, "end_line": 3})
        );
        assert!(names(&r).contains(&"after"));
    }

    #[test]
    fn a_comment_is_stripped_but_a_hash_inside_a_string_is_not() {
        let r = parse("sub go {\n    my $s = \"a # b\";\n}\nsub after { 1 }\n");
        assert_eq!(
            find(&r, "go").properties["source_span"],
            serde_json::json!({"start_line": 1, "end_line": 3})
        );
        assert!(names(&r).contains(&"after"));
    }

    #[test]
    fn a_declaration_inside_a_comment_is_not_recovered() {
        let r = parse("# sub commented_out { }\nsub real { 1 }\n");
        assert!(!names(&r).contains(&"commented_out"));
    }

    // ── ids / robustness ────────────────────────────────────────────────────

    #[test]
    fn a_project_field_qualifies_ids_but_not_displayed_names() {
        let file_a = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, b"proj-a:t.pm"));
        let file_b = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, b"proj-b:t.pm"));
        let a = parse_perl_file("package Foo;\n", file_a, Some("proj-a"));
        let b = parse_perl_file("package Foo;\n", file_b, Some("proj-b"));
        assert_eq!(a.objects[0].name, "Foo");
        assert_eq!(b.objects[0].name, "Foo");
        assert_ne!(a.objects[0].id, b.objects[0].id);
    }

    #[test]
    fn parsing_is_deterministic_across_runs() {
        let src = "package P;\nuse Foo;\nsub go { 1 }\n";
        let a = parse(src);
        let b = parse(src);
        assert_eq!(
            a.objects.iter().map(|o| o.id).collect::<Vec<_>>(),
            b.objects.iter().map(|o| o.id).collect::<Vec<_>>()
        );
        assert_eq!(
            a.relationships.iter().map(|r| r.id).collect::<Vec<_>>(),
            b.relationships.iter().map(|r| r.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_malformed_file_does_not_panic() {
        for src in [
            "sub {",
            "package ;",
            "}}}}",
            "use ;",
            "=head2\n",
            "sub go { ' unterminated",
            "package",
        ] {
            let _ = parse(src);
        }
    }

    #[test]
    fn a_declaring_file_and_a_using_file_fold_into_one_object_keeping_the_description() {
        // Guards `absorb_package`: whichever file this run reads first, the real POD description
        // has to survive.
        let mut thin = KirObject::new("Foo", ObjectKind::Custom("PerlPackage".to_string()));
        thin.id = perl_package_kir_id("Foo");
        let rich = KirObject::new("Foo", ObjectKind::Custom("PerlPackage".to_string()))
            .with_property("description", serde_json::json!("The real prose."));

        let mut kept = thin;
        absorb_package(&mut kept, rich);
        assert_eq!(kept.properties["description"], "The real prose.");
    }
}
