//! RFC 0035 — deterministic Markdown rendering from the compiled ledger.
//!
//! Pure rendering over already-compiled ledger data: no LLM, no interpretation, no new
//! extraction. A `KirObject` plus the `KirRelationship`s that touch it plus the `KirEvidence`
//! those relationships and the object itself cite, in — a Markdown page, out. Every claim in
//! the page traces back to a real evidence id already in the ledger; nothing is invented here.
//!
//! Phase 2 generalizes Phase 1's `Table`-only renderer to every "significant" object kind
//! ([`is_significant`]) and adds [`render_index_page`] — the granularity decision RFC 0035 left
//! as an Open Question, resolved empirically by rendering a real recovered SQL schema and a real
//! git-observed file tree together (devlog_34/35): every kind gets a page except `Column`, which
//! stays embedded in its parent `Table`/`Dataset`'s properties rather than a page of its own —
//! one page per module/file/table/pipeline, not per symbol, matching repowise's own granularity
//! choice researched earlier in the same project.

use ekos_kir::{KirEvidence, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use std::collections::{BTreeMap, HashMap, HashSet};

mod layer_classification;
pub use layer_classification::{Layer, LayerOverride, classify_path};

/// One generated documentation page.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPage {
    /// Filesystem-safe file name (without directory), e.g. `table-customers.md`.
    pub file_name: String,
    pub content: String,
}

/// Whether `kind` gets its own generated page. `Column` is deliberately excluded — it's a
/// sub-part of its parent `Table`/`Dataset`, already rendered in that parent's properties table,
/// not a standalone documentation unit. Every other kind, including `Unknown` and `Custom(_)`,
/// gets a page: hiding a kind by default would silently drop real compiled facts, the same
/// mistake RFC 0035's relationship-grouping already avoids by not filtering to `ForeignKey` only.
pub fn is_significant(kind: &ObjectKind) -> bool {
    !matches!(kind, ObjectKind::Column)
}

// ── Phase 4 — format-agnostic page model ────────────────────────────────────
//
// `build_object_page_model` assembles everything an object's page needs to say, with no
// Markdown/HTML syntax in it. `render_markdown_object_page`/`render_html_object_page` each turn
// that model into one format — RFC 0035's explicit Phase 4 design: "`--format md` and `--format
// html` share the same underlying page-model data structure ... only the renderer differs."
// `render_object_page` (below) stays as the Markdown-producing convenience wrapper every earlier
// phase's tests already exercise, now implemented in terms of the model instead of duplicating
// the assembly logic.

/// Whether/how an inline relationship citation resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum RowEvidence {
    /// The relationship carries no evidence id at all.
    None,
    /// An evidence id is cited but wasn't found in the resolved evidence set passed in.
    Unavailable,
    /// Resolved to its evidence fragment text.
    Resolved(String),
}

/// One relationship row in a page's Relationships section.
#[derive(Debug, Clone, PartialEq)]
pub struct RelationshipRow {
    pub outgoing: bool,
    pub other_id: KirId,
    pub other_name: Option<String>,
    pub evidence: RowEvidence,
}

/// One row in a page's Evidence section.
#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceRow {
    pub id: KirId,
    /// `Some((fragment, confidence))` when resolved, `None` when the id wasn't in the resolved
    /// evidence set (rendered as "evidence unavailable" rather than dropped).
    pub resolved: Option<(String, f32)>,
}

/// Opt-in LLM-written overview for one object's page (RFC 0035 Phase 5). Populated by the CLI
/// after calling `AiRuntime::ask` — `docs-gen` itself never calls an LLM; it only knows how to
/// render one if the caller supplies it, keeping the deterministic tier's zero-LLM guarantee
/// intact regardless of what Phase 5 adds.
#[derive(Debug, Clone, PartialEq)]
pub struct ProseSection {
    pub text: String,
    /// Evidence ids the LLM's response cited, already filtered to only ids that exist in the
    /// object's own resolved evidence set — `AiRuntime::ask`'s citation-validation guarantee
    /// (RFC 0035 Phase 5's design note: reuse `ask`'s pipeline, don't reimplement it), so this
    /// list can never contain a fabricated id.
    pub cited_evidence: Vec<KirId>,
}

/// Format-agnostic content for one object's page (RFC 0035 Phase 4).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectPageModel {
    pub kind: ObjectKind,
    pub name: String,
    /// Real, already-written documentation (Phase 1 of the "Real Descriptions, Purpose, and
    /// Links" plan — a real `///`/docstring/`@doc`/JSDoc comment the analyzer captured into the
    /// object's own `"description"` property). `None` when the source has no real doc comment —
    /// rendered as an honest "not documented in source" placeholder, never fabricated at this
    /// deterministic layer (`prose`, below, is where an opt-in LLM may add real heuristic
    /// enrichment on top). Promoted out of `properties` below so it isn't shown twice.
    pub definition: Option<String>,
    pub properties: Vec<(String, String)>,
    /// Real relationships already compiled, regrouped by real structural meaning rather than raw
    /// relationship kind (Phase 2): `"Based on"` (the real `Contains` *parent* — where this is
    /// declared), `"Contains"` (real `Contains` children, unchanged from the prior grouping),
    /// `"Used in"` (every other real incoming edge — who calls/depends on/references this),
    /// `"Dependent on"` (every other real outgoing edge — what this itself relies on). A group
    /// with zero real rows is omitted entirely rather than shown empty.
    pub relationship_groups: Vec<(String, Vec<RelationshipRow>)>,
    /// `Some(fenced-Markdown Mermaid block)` from [`render_mermaid_graph`] when there's at least
    /// one relationship to diagram, `None` otherwise. Kept as the exact Markdown-fenced string
    /// (not the raw Mermaid body) so `render_markdown_object_page` can embed it unmodified; the
    /// HTML renderer strips the fence before embedding into a `<pre>` block.
    pub diagram_markdown: Option<String>,
    pub evidence: Vec<EvidenceRow>,
    /// `None` by default (the deterministic tier); set by the caller after an opt-in `--prose`
    /// LLM call. `build_object_page_model` always initializes this to `None` — prose is layered
    /// on top of the model, never produced by it.
    pub prose: Option<ProseSection>,
}

/// Assemble the format-agnostic model for `object`. See `render_object_page`'s original
/// documentation (still accurate) for what `relationships`/`evidence`/`object_names` mean.
pub fn build_object_page_model(
    object: &KirObject,
    relationships: &[KirRelationship],
    evidence: &[KirEvidence],
    object_names: &HashMap<KirId, String>,
) -> ObjectPageModel {
    let evidence_by_id: HashMap<KirId, &KirEvidence> = evidence.iter().map(|e| (e.id, e)).collect();

    // Phase 1/2 ("Real Descriptions, Purpose, and Links"): promote a real "description" property
    // (a real doc comment an analyzer captured) out of the generic table into its own `definition`
    // field — real analyzers write plain strings here, so `as_str` covers every real case; a
    // non-string value (never written by any real analyzer today) is left in the generic
    // properties table rather than silently dropped.
    let definition = object
        .properties
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut properties: Vec<(String, String)> = object
        .properties
        .iter()
        .filter(|(k, _)| definition.is_none() || k.as_str() != "description")
        .map(|(k, v)| (k.clone(), format_value(v)))
        .collect();
    properties.sort_by(|a, b| a.0.cmp(&b.0));

    // Phase 2: real relationships regrouped by real structural meaning rather than raw kind — see
    // `ObjectPageModel::relationship_groups`'s own doc comment for the four real buckets.
    let mut based_on: Vec<RelationshipRow> = Vec::new();
    let mut contains: Vec<RelationshipRow> = Vec::new();
    let mut used_in: Vec<RelationshipRow> = Vec::new();
    let mut dependent_on: Vec<RelationshipRow> = Vec::new();
    for rel in relationships {
        let outgoing = rel.from == object.id;
        let other_id = if outgoing { rel.to } else { rel.from };
        let row_evidence = match rel.evidence.first() {
            None => RowEvidence::None,
            Some(ev_id) => match evidence_by_id.get(ev_id) {
                Some(ev) => RowEvidence::Resolved(ev.fragment.clone()),
                None => RowEvidence::Unavailable,
            },
        };
        let row = RelationshipRow {
            outgoing,
            other_id,
            other_name: object_names.get(&other_id).cloned(),
            evidence: row_evidence,
        };
        let is_contains = rel.kind == RelationshipKind::Contains;
        match (is_contains, outgoing) {
            (true, false) => based_on.push(row),
            (true, true) => contains.push(row),
            (false, false) => used_in.push(row),
            (false, true) => dependent_on.push(row),
        }
    }
    let mut relationship_groups: Vec<(String, Vec<RelationshipRow>)> = Vec::new();
    for (label, rows) in [
        ("Based on", based_on),
        ("Contains", contains),
        ("Used in", used_in),
        ("Dependent on", dependent_on),
    ] {
        if !rows.is_empty() {
            relationship_groups.push((label.to_string(), rows));
        }
    }

    let diagram_markdown = if relationships.is_empty() {
        None
    } else {
        Some(render_mermaid_graph(object, relationships, object_names))
    };

    let mut seen: std::collections::HashSet<KirId> = std::collections::HashSet::new();
    let mut evidence_rows = Vec::new();
    for id in object.evidence.iter().copied().chain(
        relationships
            .iter()
            .flat_map(|r| r.evidence.iter().copied()),
    ) {
        if seen.insert(id) {
            evidence_rows.push(EvidenceRow {
                id,
                resolved: evidence_by_id
                    .get(&id)
                    .map(|ev| (ev.fragment.clone(), ev.confidence)),
            });
        }
    }

    ObjectPageModel {
        kind: object.kind.clone(),
        name: object.name.clone(),
        definition,
        properties,
        relationship_groups,
        diagram_markdown,
        evidence: evidence_rows,
        prose: None,
    }
}

/// Render one Markdown page for `object`. Thin wrapper: builds the model, then renders it.
///
/// `relationships` should be every `KirRelationship` touching `object.id` (both directions —
/// `relationships_for` on the ledger already returns both), grouped in the output by kind so no
/// relationship is silently dropped, matching the project's "Unmapped is a citizen, not a
/// failure" posture rather than only showing `ForeignKey` and hiding the rest.
///
/// `evidence` resolves every evidence id referenced by `object` or by any relationship in
/// `relationships`; a citation whose id isn't present in `evidence` renders as
/// `evidence unavailable` rather than panicking or being silently omitted — the page must never
/// claim more certainty than the data it was given.
///
/// `object_names` resolves the *other* endpoint of each relationship to a human-readable name
/// (e.g. `orders` instead of a raw id) when the caller has it available; an id missing from the
/// map falls back to rendering the raw id rather than guessing or omitting the edge.
pub fn render_object_page(
    object: &KirObject,
    relationships: &[KirRelationship],
    evidence: &[KirEvidence],
    object_names: &HashMap<KirId, String>,
) -> RenderedPage {
    let model = build_object_page_model(object, relationships, evidence, object_names);
    render_markdown_object_page(&model)
}

/// Render `model` as a Markdown page.
pub fn render_markdown_object_page(model: &ObjectPageModel) -> RenderedPage {
    let mut out = String::new();
    out.push_str(&format!("# {} ({})\n\n", model.name, model.kind));

    out.push_str("## Definition\n\n");
    match &model.definition {
        Some(text) => out.push_str(&format!("{}\n\n", text.trim())),
        None => out.push_str("_Not documented in source._\n\n"),
    }

    if let Some(prose) = &model.prose {
        out.push_str("## Overview\n\n");
        out.push_str(prose.text.trim());
        out.push_str("\n\n");
        if !prose.cited_evidence.is_empty() {
            let ids: Vec<String> = prose
                .cited_evidence
                .iter()
                .map(|id| format!("`{id}`"))
                .collect();
            out.push_str(&format!("_Cited evidence: {}_\n\n", ids.join(", ")));
        }
    }

    out.push_str("## Properties\n\n");
    if model.properties.is_empty() {
        out.push_str("_No compiled properties._\n\n");
    } else {
        out.push_str("| Key | Value |\n|---|---|\n");
        for (key, value) in &model.properties {
            out.push_str(&format!("| `{key}` | {value} |\n"));
        }
        out.push('\n');
    }

    out.push_str("## Relationships\n\n");
    if model.relationship_groups.is_empty() {
        out.push_str("_No compiled relationships touch this object._\n\n");
    } else {
        for (kind, rows) in &model.relationship_groups {
            out.push_str(&format!("### {kind}\n\n"));
            for row in rows {
                let arrow = if row.outgoing { "→" } else { "←" };
                let direction = match &row.other_name {
                    Some(name) => format!("{arrow} {name} (`{}`)", row.other_id),
                    None => format!("{arrow} `{}`", row.other_id),
                };
                out.push_str(&format!("- {direction}"));
                match &row.evidence {
                    RowEvidence::Resolved(fragment) => {
                        out.push_str(&format!(" — evidence: {fragment}"))
                    }
                    RowEvidence::Unavailable => out.push_str(" — evidence unavailable"),
                    RowEvidence::None => {}
                }
                out.push('\n');
            }
            out.push('\n');
        }
    }

    out.push_str("## Diagram\n\n");
    match &model.diagram_markdown {
        Some(diagram) => {
            out.push_str(diagram);
            out.push('\n');
        }
        None => out.push_str("_No relationships to diagram._\n\n"),
    }

    out.push_str("## Evidence\n\n");
    if model.evidence.is_empty() {
        out.push_str("_No evidence cited._\n");
    } else {
        for row in &model.evidence {
            match &row.resolved {
                Some((fragment, confidence)) => out.push_str(&format!(
                    "- `{}` — {fragment} (confidence: {confidence:.2})\n",
                    row.id
                )),
                None => out.push_str(&format!("- `{}` — evidence unavailable\n", row.id)),
            }
        }
    }

    RenderedPage {
        file_name: page_file_name(&model.kind, &model.name, "md"),
        content: out,
    }
}

/// Render `model` as a self-contained HTML page — embedded CSS, no external asset dependency
/// (RFC 0035's design note: reuse `docs/assets/theme.css`'s *visual pattern*, not a fragile
/// build-time path to this repo's own file, since `ekos docs generate` runs in arbitrary user
/// workspaces that don't have this repo's `docs/` directory available). The Mermaid diagram is
/// shown as its source inside a `<pre>` block, not live-rendered — rendering it would need
/// bundling or CDN-loading `mermaid.js`, and this generator's whole point is working fully
/// offline with zero external dependency; an honest limit, not an oversight (see RFC 0035 Open
/// Questions).
pub fn render_html_object_page(model: &ObjectPageModel) -> RenderedPage {
    let mut body = String::new();
    body.push_str(&format!(
        "<h1>{} <span class=\"kind\">({})</span></h1>\n",
        html_escape(&model.name),
        html_escape(&model.kind.to_string())
    ));

    body.push_str("<h2>Definition</h2>\n");
    match &model.definition {
        Some(text) => body.push_str(&format!("<p>{}</p>\n", html_escape(text.trim()))),
        None => body.push_str("<p class=\"empty\">Not documented in source.</p>\n"),
    }

    if let Some(prose) = &model.prose {
        body.push_str("<h2>Overview</h2>\n");
        body.push_str(&format!("<p>{}</p>\n", html_escape(prose.text.trim())));
        if !prose.cited_evidence.is_empty() {
            let ids: Vec<String> = prose
                .cited_evidence
                .iter()
                .map(|id| format!("<code>{id}</code>"))
                .collect();
            body.push_str(&format!(
                "<p class=\"empty\">Cited evidence: {}</p>\n",
                ids.join(", ")
            ));
        }
    }

    body.push_str("<h2>Properties</h2>\n");
    if model.properties.is_empty() {
        body.push_str("<p class=\"empty\">No compiled properties.</p>\n");
    } else {
        body.push_str("<table>\n<thead><tr><th>Key</th><th>Value</th></tr></thead>\n<tbody>\n");
        for (key, value) in &model.properties {
            body.push_str(&format!(
                "<tr><td><code>{}</code></td><td>{}</td></tr>\n",
                html_escape(key),
                html_escape(value)
            ));
        }
        body.push_str("</tbody>\n</table>\n");
    }

    body.push_str("<h2>Relationships</h2>\n");
    if model.relationship_groups.is_empty() {
        body.push_str("<p class=\"empty\">No compiled relationships touch this object.</p>\n");
    } else {
        for (kind, rows) in &model.relationship_groups {
            body.push_str(&format!("<h3>{}</h3>\n<ul>\n", html_escape(kind)));
            for row in rows {
                let arrow = if row.outgoing { "&rarr;" } else { "&larr;" };
                let label = match &row.other_name {
                    Some(name) => format!(
                        "{arrow} {} <code>{}</code>",
                        html_escape(name),
                        row.other_id
                    ),
                    None => format!("{arrow} <code>{}</code>", row.other_id),
                };
                let evidence_html = match &row.evidence {
                    RowEvidence::Resolved(fragment) => {
                        format!(" — evidence: {}", html_escape(fragment))
                    }
                    RowEvidence::Unavailable => " — evidence unavailable".to_string(),
                    RowEvidence::None => String::new(),
                };
                body.push_str(&format!("<li>{label}{evidence_html}</li>\n"));
            }
            body.push_str("</ul>\n");
        }
    }

    body.push_str("<h2>Diagram</h2>\n");
    match &model.diagram_markdown {
        Some(diagram) => {
            body.push_str("<pre class=\"mermaid-source\"><code>");
            body.push_str(&html_escape(strip_mermaid_fence(diagram)));
            body.push_str("</code></pre>\n");
        }
        None => body.push_str("<p class=\"empty\">No relationships to diagram.</p>\n"),
    }

    body.push_str("<h2>Evidence</h2>\n");
    if model.evidence.is_empty() {
        body.push_str("<p class=\"empty\">No evidence cited.</p>\n");
    } else {
        body.push_str("<ul>\n");
        for row in &model.evidence {
            match &row.resolved {
                Some((fragment, confidence)) => body.push_str(&format!(
                    "<li><code>{}</code> — {} (confidence: {confidence:.2})</li>\n",
                    row.id,
                    html_escape(fragment)
                )),
                None => body.push_str(&format!(
                    "<li><code>{}</code> — evidence unavailable</li>\n",
                    row.id
                )),
            }
        }
        body.push_str("</ul>\n");
    }

    RenderedPage {
        file_name: page_file_name(&model.kind, &model.name, "html"),
        content: html_document(&format!("{} — {}", model.name, model.kind), &body),
    }
}

/// Filesystem-safe file name for an object's page, e.g. `table-customers.md`,
/// `file-mainrs.html`, `transformnode-fact-sales.md`. Kind-prefixed so pages from different kinds
/// never collide even when two objects share a bare name (a `Table` and a `Pipeline` both named
/// `orders`, for instance).
fn page_file_name(kind: &ObjectKind, name: &str, ext: &str) -> String {
    format!("{}-{}.{ext}", slugify(&kind.to_string()), slugify(name))
}

/// Whether curated (`--layout curated`, RFC 0042) writes an individual detail page for `kind` —
/// the single source of truth both `generate_curated`'s page-writing loop and every renderer's
/// link-generation (`render_architecture`'s Components/Dependency-Graph-sample links, `render_api`)
/// must agree on, so a link is never emitted to a page that was never written (or vice versa: a
/// page written that nothing links to).
pub fn is_entity_page_kind(kind: &ObjectKind) -> bool {
    match kind {
        ObjectKind::Custom(s) => {
            matches!(
                s.as_str(),
                "Crate"
                    | "RustModule"
                    | "RustSymbol"
                    | "PythonModule"
                    | "PythonSymbol"
                    | "ElixirModule"
                    | "ElixirSymbol"
                    | "JsModule"
                    | "JsSymbol"
                    | "Technology"
                    | "Rollup"
            )
        }
        ObjectKind::Pipeline => true,
        _ => false,
    }
}

/// Collision-free, GitHub-browsable relative *path* for every object in `objects`, keyed by id —
/// `entities/<kind>/<2-char shard>/<name>.<ext>`. Two levels of directory nesting exist for a
/// concrete, hit-in-practice reason (RFC 0042): a flat `doc/` directory holding one page per
/// `RustSymbol` in a real ~1300-symbol codebase blows past GitHub's 1,000-entries-per-directory
/// listing cap — confirmed live after first shipping curated docs with everything flat in `doc/`.
/// Sharding by the first two characters of the slugified name keeps every directory small
/// regardless of how many objects of one kind exist, without needing a source-tree-aware grouping
/// key (which not every kind has — e.g. `Technology`/`Pipeline` have no natural "folder").
/// [`page_file_name`] alone also collides whenever two objects of the same kind share a bare name
/// — routine at program-entity scale: two different files can each declare a `fn new`. The first
/// occurrence (in a stable `id`-sorted order, so this never depends on caller iteration order)
/// keeps the plain name; every later collision gets an 8-hex-character id suffix appended so it
/// never overwrites the first.
pub fn unique_page_file_names(objects: &[KirObject], ext: &str) -> HashMap<KirId, String> {
    let mut sorted: Vec<&KirObject> = objects.iter().collect();
    sorted.sort_by_key(|o| o.id.0);

    let mut used: HashSet<String> = HashSet::new();
    let mut out = HashMap::with_capacity(objects.len());
    for o in sorted {
        let kind_slug = slugify(&o.kind.to_string());
        let name_slug = slugify(&o.name);
        let shard: String = if name_slug.is_empty() {
            "misc".to_string()
        } else {
            name_slug.chars().take(2).collect()
        };
        let base = format!("entities/{kind_slug}/{shard}/{name_slug}.{ext}");
        let name = if used.insert(base.clone()) {
            base
        } else {
            let suffix = &o.id.0.simple().to_string()[..8];
            format!("entities/{kind_slug}/{shard}/{name_slug}-{suffix}.{ext}")
        };
        used.insert(name.clone());
        out.insert(o.id, name);
    }
    out
}

// ── Phase 3 — Mermaid diagrams ──────────────────────────────────────────────
//
// One generic `graph TD` renderer (`render_mermaid_graph`) covers two of RFC 0035's three
// diagram families: the per-object dependency graph, and — because Transformation IR nodes
// (RFC 0027) are `KirObject`s connected by `Custom("FeedsInto")` relationships like any other
// object — the transformation DAG falls out of the same renderer for free when it's centered on
// a `Custom("TransformNode")` object; no separate function duplicating the same graph-drawing
// logic. The ER diagram genuinely needs different Mermaid syntax (`erDiagram`, not `graph TD`),
// so it's the one case with its own renderer, `render_er_diagram`.

/// Mermaid node id for `id`: hyphens aren't valid in a bare Mermaid identifier, so this strips
/// them rather than quoting — quoting is reserved for the human-readable label text instead.
fn mermaid_node_id(id: &KirId) -> String {
    format!("n{}", id.0.simple())
}

/// Mermaid label text must not contain an unescaped `"` or newline — both would break the
/// `id["label"]` syntax; replaced rather than rejected, since a label is display-only.
fn mermaid_escape_label(s: &str) -> String {
    s.replace('"', "'").replace(['\n', '\r'], " ")
}

/// `CoupledWith` is a derived/statistical signal (RFC 0020's git co-change coupling), not a hard
/// dependency like `ForeignKey`/`References`/`FeedsInto` — rendered as a dashed edge so the
/// diagram itself communicates that distinction instead of only the edge label.
fn mermaid_arrow(kind: &RelationshipKind) -> &'static str {
    match kind {
        RelationshipKind::CoupledWith => "-.->",
        _ => "-->",
    }
}

/// Render a Mermaid `graph TD` diagram of `object`'s 1-hop neighborhood: `object` plus every
/// object at the other end of a relationship in `relationships`, with edges labeled by
/// relationship kind and directed exactly as the underlying `KirRelationship` is (`from → to`).
/// `object_names` resolves neighbor labels the same way `render_object_page` does; an
/// unresolvable neighbor renders with its short id rather than being dropped from the diagram.
pub fn render_mermaid_graph(
    object: &KirObject,
    relationships: &[KirRelationship],
    object_names: &HashMap<KirId, String>,
) -> String {
    let mut out = String::from("```mermaid\ngraph TD\n");
    let center_node = mermaid_node_id(&object.id);
    out.push_str(&format!(
        "    {center_node}[\"{}\"]\n",
        mermaid_escape_label(&object.name)
    ));

    let mut labeled: std::collections::HashSet<KirId> = std::collections::HashSet::new();
    labeled.insert(object.id);

    for rel in relationships {
        let other_id = if rel.from == object.id {
            rel.to
        } else {
            rel.from
        };
        if labeled.insert(other_id) {
            let label = object_names
                .get(&other_id)
                .cloned()
                .unwrap_or_else(|| format!("{other_id}"));
            out.push_str(&format!(
                "    {}[\"{}\"]\n",
                mermaid_node_id(&other_id),
                mermaid_escape_label(&label)
            ));
        }
        let arrow = mermaid_arrow(&rel.kind);
        out.push_str(&format!(
            "    {} {arrow}|{}| {}\n",
            mermaid_node_id(&rel.from),
            rel.kind,
            mermaid_node_id(&rel.to)
        ));
    }

    out.push_str("```\n");
    out
}

/// Render a Mermaid `erDiagram` of every `ForeignKey` relationship strictly between two objects
/// in `tables` — a whole-workspace entity-relationship diagram, not a per-object one, since an ER
/// diagram's whole point is showing how multiple tables relate at once. `ForeignKey` edges to/from
/// an object not in `tables` are silently excluded (that object isn't a `Table`, or wasn't passed
/// in) rather than rendered as a dangling reference. Duplicate edges (both directions of the same
/// pair already emitted) are collapsed to one line.
pub fn render_er_diagram(tables: &[KirObject], relationships: &[KirRelationship]) -> String {
    use std::collections::HashSet;

    let table_ids: HashSet<KirId> = tables.iter().map(|t| t.id).collect();
    let name_by_id: HashMap<KirId, &str> = tables.iter().map(|t| (t.id, t.name.as_str())).collect();

    let mut out = String::from("```mermaid\nerDiagram\n");
    let mut emitted: HashSet<(KirId, KirId)> = HashSet::new();
    for rel in relationships {
        if !matches!(rel.kind, RelationshipKind::ForeignKey) {
            continue;
        }
        if !table_ids.contains(&rel.from) || !table_ids.contains(&rel.to) {
            continue;
        }
        if !emitted.insert((rel.from, rel.to)) {
            continue;
        }
        out.push_str(&format!(
            "    \"{}\" }}o--|| \"{}\" : references\n",
            mermaid_escape_label(name_by_id[&rel.from]),
            mermaid_escape_label(name_by_id[&rel.to])
        ));
    }
    if emitted.is_empty() {
        out.push_str("    %% no ForeignKey relationships among the given tables\n");
    }
    out.push_str("```\n");
    out
}

/// Render the Executive Overview (RFC 0068 §14). Only the fields this project has real compiled
/// signal for are populated with data: component/crate counts (`count_by_kind`, already this
/// file's own established pattern), the technologies with the most real compiled dependents, and
/// the Open Questions count (RFC 0065 §17). `Purpose`, `Architecture style`, `Major risks`, and
/// `Architecture confidence` are named by RFC 0068's own template but have no real EKOS source yet
/// — `Major risks` needs a `Risk` KIR kind this project doesn't have (RFC 0068 §62 Phase 2);
/// `Architecture confidence` needs `evaluate_architecture` wired through from `cli` the same way
/// RFC 0069 wired drift through, not yet done for the plain `docs generate` path (only
/// `ekos architecture investigate` computes it today); `Purpose`/`Architecture style` need either
/// an LLM read of real project documentation or human input, neither available to a zero-LLM
/// deterministic renderer. Each says so explicitly rather than being silently dropped or guessed.
fn render_architecture_summary(objects: &[KirObject], relationships: &[KirRelationship]) -> String {
    let mut out = String::new();

    let counts = count_by_kind(objects, is_significant);
    let component_total: usize = counts.iter().map(|(_, n)| n).sum();
    out.push_str(&format!(
        "**Components:** {component_total} compiled object(s) across {} kind(s)\n\n",
        counts.len()
    ));

    let crate_count = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Crate"))
        .count();
    out.push_str(&format!("**Containers (crates):** {crate_count}\n\n"));

    // Deduplicated by (from, to) pair before counting — the same non-deterministic relationship
    // id gap RFC 0070 found and fixed for the Technology Inventory view applies here too (found
    // live, verifying this exact section against this repo's own real, repeatedly-recommitted
    // ledger: raw counts read 132 "dependents" for a technology only ~33 real crates use).
    let unique_edges: HashSet<(KirId, KirId)> = relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::DependsOn)
        .map(|r| (r.from, r.to))
        .collect();
    let mut dependent_counts: HashMap<KirId, usize> = HashMap::new();
    for (_, to) in &unique_edges {
        *dependent_counts.entry(*to).or_insert(0) += 1;
    }
    let mut technologies: Vec<(&KirObject, usize)> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Technology"))
        .map(|o| (o, dependent_counts.get(&o.id).copied().unwrap_or(0)))
        .filter(|(_, n)| *n > 0)
        .collect();
    technologies.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    const TOP_N_TECHNOLOGIES: usize = 5;
    if technologies.is_empty() {
        out.push_str("**Primary technologies:** _none compiled_\n\n");
    } else {
        let top: Vec<String> = technologies
            .iter()
            .take(TOP_N_TECHNOLOGIES)
            .map(|(t, n)| format!("{} ({n} dependent(s))", t.name))
            .collect();
        out.push_str(&format!("**Primary technologies:** {}\n\n", top.join(", ")));
    }

    let open_questions = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "ArchitectureGap"))
        .count();
    out.push_str(&format!("**Open questions:** {open_questions}\n\n"));

    out.push_str(
        "**Purpose:** _not yet computed — no real EKOS source for a project's stated purpose \
         today (RFC 0068 §14)_\n\n\
         **Architecture style:** _not yet computed — requires reasoning EKOS doesn't perform yet_\n\n\
         **Major risks:** _not yet computed — no `Risk` KIR kind exists yet (RFC 0068 §29/§62)_\n\n\
         **Architecture confidence:** _not yet computed here — see `ekos architecture investigate`'s \
         own evaluation report (RFC 0065 Phase 3) for a real completeness/evidence-coverage score_\n\n",
    );

    out
}

/// `(id, label)` nodes and `(from_id, to_id)` edges — the interchange shape both
/// [`system_context_graph`] and [`render_graph_svg`] share.
type IdGraph = (Vec<(String, String)>, Vec<(String, String)>);

/// Shared node/edge extraction behind both [`render_system_context`] (Mermaid text) and
/// [`render_system_context_svg`] (standalone SVG, RFC 0073) — computed once so the two renderers
/// can never drift apart on which technologies actually qualify. `None` when there's no real data
/// to show (no crates, no technologies, or no crate has a real `DependsOn` edge to one) — the
/// same honest-empty-state condition both callers already had before this was factored out.
fn system_context_graph(
    objects: &[KirObject],
    relationships: &[KirRelationship],
) -> Option<IdGraph> {
    let crate_ids: HashSet<KirId> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Crate"))
        .map(|o| o.id)
        .collect();
    let technologies: HashMap<KirId, &str> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Technology"))
        .map(|o| (o.id, o.name.as_str()))
        .collect();

    if crate_ids.is_empty() || technologies.is_empty() {
        return None;
    }

    let mut used: HashSet<KirId> = HashSet::new();
    for rel in relationships {
        if rel.kind == RelationshipKind::DependsOn
            && crate_ids.contains(&rel.from)
            && technologies.contains_key(&rel.to)
        {
            used.insert(rel.to);
        }
    }

    if used.is_empty() {
        return None;
    }

    let system_node = "system_context_root".to_string();
    let mut nodes = vec![(system_node.clone(), "System".to_string())];
    let mut edges = Vec::new();
    let mut sorted: Vec<KirId> = used.into_iter().collect();
    sorted.sort_by_key(|id| technologies[id]);
    for tech_id in sorted {
        let node_id = mermaid_node_id(&tech_id);
        nodes.push((node_id.clone(), technologies[&tech_id].to_string()));
        edges.push((system_node.clone(), node_id));
    }
    Some((nodes, edges))
}

/// Render a C4 System Context diagram (RFC 0068 §15): the whole compiled workspace collapsed to
/// one "System" node, with an edge to every `Custom("Technology")` object that at least one
/// `Custom("Crate")` actually has a real `DependsOn` edge to — not every `Technology` object that
/// happens to exist, only ones a real compiled dependency connects to the system. One C4 level
/// broader than the Container-level `## Crate & Workspace Topology` view; deliberately has no new
/// extraction behind it, reusing exactly the Crate/Technology/DependsOn data that view already
/// has (RFC 0042).
fn render_system_context(objects: &[KirObject], relationships: &[KirRelationship]) -> String {
    let Some((nodes, edges)) = system_context_graph(objects, relationships) else {
        return "_No external technology dependencies compiled._\n\n".to_string();
    };

    let mut out = String::from("```mermaid\ngraph TD\n");
    for (id, label) in &nodes {
        out.push_str(&format!("    {id}[\"{}\"]\n", mermaid_escape_label(label)));
    }
    for (from, to) in &edges {
        out.push_str(&format!("    {from} -->|DependsOn| {to}\n"));
    }
    out.push_str("```\n");
    out
}

/// Render the System Context diagram (see [`render_system_context`]) as a standalone SVG file
/// (RFC 0068's remaining §61 MVP item: "current output is Mermaid-in-Markdown only ... isn't a
/// standalone SVG artifact"). `None` under the exact same honest-empty condition
/// [`render_system_context`] falls back to text for — no SVG file is worth writing to disk for a
/// diagram that has nothing real to show. Uses the same node/edge data as the Mermaid rendering
/// ([`system_context_graph`]), laid out and drawn by the generic [`render_graph_svg`] primitive.
pub fn render_system_context_svg(
    objects: &[KirObject],
    relationships: &[KirRelationship],
) -> Option<RenderedPage> {
    let (nodes, edges) = system_context_graph(objects, relationships)?;
    Some(RenderedPage {
        file_name: "system-context.svg".to_string(),
        content: render_graph_svg(&nodes, &edges),
    })
}

/// Real per-object layer membership feeding `## System Decomposition`: every compiled `File`
/// object classified via [`classify_path`], plus every real `ObjectKind::Table` — already
/// unambiguously `Layer::Database` by its own kind, no path heuristic needed (`classify_path`
/// itself never assigns `Layer::Database`).
fn layer_membership(objects: &[KirObject], overrides: &[LayerOverride]) -> HashMap<KirId, Layer> {
    let mut membership = HashMap::new();
    for obj in objects {
        match &obj.kind {
            ObjectKind::File => {
                if let Some(layer) = classify_path(&obj.name, overrides) {
                    membership.insert(obj.id, layer);
                }
            }
            ObjectKind::Table => {
                membership.insert(obj.id, Layer::Database);
            }
            _ => {}
        }
    }
    membership
}

/// Extends `membership`'s File/Table layer assignments with real `Contains`-based inheritance —
/// e.g. a real Ecto Repo `Custom("ElixirModule")` (RFC 0086 Phase 6) inherits its owning `File`'s
/// `Layer::Backend` so a `DependsOn` edge *from that module* can resolve to a real layer. Used
/// only for resolving a cross-tier edge's endpoints — never for the per-layer node *counts* in
/// [`system_decomposition_graph`], which stay exactly the real File/Table counts, not inflated by
/// every object that merely lives inside one.
fn layer_membership_for_edges(
    membership: &HashMap<KirId, Layer>,
    relationships: &[KirRelationship],
) -> HashMap<KirId, Layer> {
    let mut extended = membership.clone();
    for rel in relationships {
        if rel.kind == RelationshipKind::Contains
            && let Some(&layer) = membership.get(&rel.from)
            && !extended.contains_key(&rel.to)
        {
            extended.insert(rel.to, layer);
        }
    }
    extended
}

/// Builds the `## System Decomposition` graph (RFC 0068's C4 Container-level intent for a
/// non-Rust project, RFC 0083 Phase 3): one node per real, non-empty layer — Backend, Frontend,
/// SQL Database, ClickHouse Database (the two real `Table` `source_system` (RFC 0056) values kept
/// as distinct nodes rather than merged into one "Database" box, since a real project can and
/// does use both at once) — and one edge per pair of layers a real compiled `DependsOn`/
/// `ReadsFrom`/`WritesTo` relationship actually connects. Never a guessed line: matches RFC 0068
/// §22's own "don't fabricate" principle, already this crate's practice for Data Architecture's
/// Ownership/Lifecycle/Data Quality fields.
fn system_decomposition_graph(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    overrides: &[LayerOverride],
) -> Option<IdGraph> {
    let membership = layer_membership(objects, overrides);
    if membership.is_empty() {
        return None;
    }

    let clickhouse_ids: HashSet<KirId> = objects
        .iter()
        .filter(|o| {
            o.kind == ObjectKind::Table
                && o.properties.get("source_system").and_then(|v| v.as_str()) == Some("clickhouse")
        })
        .map(|o| o.id)
        .collect();

    // RFC 0086 (Phase 6): a real database-adapter `Custom("Technology")` object (e.g. from a real
    // Ecto Repo's `adapter: Ecto.Adapters.Postgres`/`ClickHouse` declaration) routes into the same
    // real SQL/ClickHouse bucket a `Table` object with matching `source_system` would — the reader
    // sees "Backend depends on ClickHouse Database" either way, regardless of which real analyzer
    // produced the evidence.
    let db_technology_bucket: HashMap<KirId, &'static str> = objects
        .iter()
        .filter(|o| {
            matches!(&o.kind, ObjectKind::Custom(s) if s == "Technology")
                && o.properties.get("ecosystem").and_then(|v| v.as_str()) == Some("database")
        })
        .map(|o| {
            let bucket = if o.name == "ClickHouse" {
                "layer_clickhouse"
            } else {
                "layer_sql"
            };
            (o.id, bucket)
        })
        .collect();

    let edge_membership = layer_membership_for_edges(&membership, relationships);
    let node_key = |id: &KirId| -> Option<&'static str> {
        if clickhouse_ids.contains(id) {
            return Some("layer_clickhouse");
        }
        if let Some(&bucket) = db_technology_bucket.get(id) {
            return Some(bucket);
        }
        edge_membership.get(id).map(|l| match l {
            Layer::Backend => "layer_backend",
            Layer::Frontend => "layer_frontend",
            Layer::Database => "layer_sql",
        })
    };

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for id in membership.keys() {
        if let Some(key) = node_key(id) {
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    if counts.is_empty() {
        return None;
    }

    let labels: [(&str, &str, &str); 4] = [
        ("layer_backend", "Backend", "file"),
        ("layer_frontend", "Frontend", "file"),
        ("layer_sql", "SQL Database", "table"),
        ("layer_clickhouse", "ClickHouse Database", "table"),
    ];
    // RFC 0086 (Phase 6): a real database-adapter Technology can name a real SQL/ClickHouse
    // dependency with zero real compiled `Table` rows behind it (e.g. Ecto configured, no schema
    // recovered yet) — the node must still exist so the edge to it isn't silently dropped
    // (`render_graph_svg` skips edges referencing an id absent from `nodes`), with an honest label
    // rather than a fabricated table count.
    let db_config_only_buckets: HashSet<&str> = db_technology_bucket.values().copied().collect();
    let mut nodes = Vec::new();
    for (key, label, unit) in labels {
        if let Some(&count) = counts.get(key) {
            let plural = if count == 1 { "" } else { "s" };
            nodes.push((key.to_string(), format!("{label} ({count} {unit}{plural})")));
        } else if db_config_only_buckets.contains(key) {
            nodes.push((
                key.to_string(),
                format!("{label} (config only, no {unit}s compiled)"),
            ));
        }
    }

    let mut edge_pairs: HashSet<(&str, &str)> = HashSet::new();
    for rel in relationships {
        let is_real_cross_tier_edge = matches!(rel.kind, RelationshipKind::DependsOn)
            || is_reads_from(&rel.kind)
            || is_writes_to(&rel.kind);
        if !is_real_cross_tier_edge {
            continue;
        }
        if let (Some(from_key), Some(to_key)) = (node_key(&rel.from), node_key(&rel.to))
            && from_key != to_key
        {
            edge_pairs.insert((from_key, to_key));
        }
    }
    let mut edges: Vec<(String, String)> = edge_pairs
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    edges.sort();

    Some((nodes, edges))
}

/// Render `## System Decomposition` (RFC 0068's C4 Container-level intent, RFC 0083 Phase 3):
/// real, evidence-backed Backend/Frontend/Database boxes — the "which components does it have and
/// how do they relate" answer for a non-Rust project where `Crate` doesn't exist. One level more
/// detailed than `## System Context` above, positioned right after it (same C4-adjacent spot).
fn render_system_decomposition(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    overrides: &[LayerOverride],
) -> String {
    let Some((nodes, edges)) = system_decomposition_graph(objects, relationships, overrides) else {
        return "_No Backend, Frontend, or Database layer data compiled yet._\n\n".to_string();
    };

    let mut out = String::from("```mermaid\ngraph TD\n");
    for (id, label) in &nodes {
        out.push_str(&format!("    {id}[\"{}\"]\n", mermaid_escape_label(label)));
    }
    if edges.is_empty() {
        out.push_str(
            "    %% No real compiled relationship yet connects these layers to each other.\n",
        );
    } else {
        for (from, to) in &edges {
            out.push_str(&format!("    {from} --> {to}\n"));
        }
    }
    out.push_str("```\n");
    out
}

/// Render `## System Decomposition` (see [`render_system_decomposition`]) as a standalone SVG
/// file, same reasoning as [`render_system_context_svg`].
pub fn render_system_decomposition_svg(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    overrides: &[LayerOverride],
) -> Option<RenderedPage> {
    let (nodes, edges) = system_decomposition_graph(objects, relationships, overrides)?;
    Some(RenderedPage {
        file_name: "system-decomposition.svg".to_string(),
        content: render_graph_svg(&nodes, &edges),
    })
}

/// Builds the `## Crate & Workspace Topology` graph (RFC 0065 §23's Container-level internal
/// dependency graph) as node/edge data for [`render_graph_svg`] — the RFC 0083 Phase 4 standalone
/// SVG counterpart to [`render_relationship_kind_graph`]'s existing Mermaid-in-Markdown rendering
/// of the same real `Crate`→`Crate` `DependsOn` edges. `None` under the same honest-empty
/// condition (`crates.is_empty()` or no internal path dependencies compiled) `render_architecture`
/// itself already checks before calling this.
fn crate_topology_graph(
    objects: &[KirObject],
    relationships: &[KirRelationship],
) -> Option<IdGraph> {
    let crates: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Crate"))
        .collect();
    let crate_ids: HashSet<KirId> = crates.iter().map(|c| c.id).collect();
    let crate_edges: Vec<&KirRelationship> = relationships
        .iter()
        .filter(|r| {
            matches!(r.kind, RelationshipKind::DependsOn)
                && crate_ids.contains(&r.from)
                && crate_ids.contains(&r.to)
        })
        .collect();
    if crate_edges.is_empty() {
        return None;
    }

    let name_by_id: HashMap<KirId, &str> = crates.iter().map(|c| (c.id, c.name.as_str())).collect();
    let mut seen: HashSet<KirId> = HashSet::new();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for rel in &crate_edges {
        for id in [rel.from, rel.to] {
            if seen.insert(id) {
                let label = name_by_id.get(&id).copied().unwrap_or("unknown");
                nodes.push((mermaid_node_id(&id), label.to_string()));
            }
        }
        edges.push((mermaid_node_id(&rel.from), mermaid_node_id(&rel.to)));
    }
    Some((nodes, edges))
}

/// Render `## Crate & Workspace Topology` (see [`crate_topology_graph`]) as a standalone SVG
/// file, same reasoning and same conditional-write contract as [`render_system_context_svg`].
pub fn render_crate_topology_svg(
    objects: &[KirObject],
    relationships: &[KirRelationship],
) -> Option<RenderedPage> {
    let (nodes, edges) = crate_topology_graph(objects, relationships)?;
    Some(RenderedPage {
        file_name: "crate-topology.svg".to_string(),
        content: render_graph_svg(&nodes, &edges),
    })
}

const SVG_NODE_WIDTH: f64 = 160.0;
const SVG_NODE_HEIGHT: f64 = 40.0;
const SVG_LAYER_GAP: f64 = 70.0;
const SVG_NODE_GAP: f64 = 24.0;
const SVG_MARGIN: f64 = 20.0;
/// Vertical gap between two *wrapped* rows of the same topological layer — deliberately smaller
/// than [`SVG_LAYER_GAP`] so a wrap reads as "more of the same row" rather than a new DAG layer.
const SVG_ROW_GAP: f64 = 16.0;
/// Maximum nodes drawn in one horizontal row before wrapping into a new row within the same
/// topological layer (RFC 0068 §61/RFC 0083 Phase 4's own tracked finding: a real System Context
/// diagram with 46 nodes in one layer rendered as one unreadable 8296px-wide row). Chosen to keep
/// a full row's width at this renderer's fixed `SVG_NODE_WIDTH` in the same rough range as a
/// typical rendered page width.
const MAX_NODES_PER_ROW: usize = 8;

/// Escape text for use inside SVG element content/attributes — the same five XML entities every
/// SVG (and HTML) text node needs; distinct from [`mermaid_escape_label`], which only needs to
/// avoid breaking Mermaid's `id["label"]` syntax, not full XML.
fn svg_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Assign each node to a layer via Kahn's algorithm (BFS topological levels): layer 0 is every
/// node with no incoming edge among `edges`, layer 1 is every node whose predecessors are all
/// already placed, and so on. Ties within a layer are broken by node id (lexicographic) so the
/// same graph always lays out identically — required for [`render_graph_svg`] to be a
/// deterministic, reproducible-build-compatible renderer (no LLM, no interpretation, matching
/// this whole crate's existing convention). A node that's part of a cycle never becomes "ready"
/// through the main loop; any such nodes are appended as one final sorted layer instead of being
/// dropped, so every node in `nodes` always appears exactly once in the result.
fn layer_nodes(nodes: &[(String, String)], edges: &[(String, String)]) -> Vec<Vec<usize>> {
    let id_to_idx: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.as_str(), i))
        .collect();
    let n = nodes.len();
    let mut indegree = vec![0usize; n];
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (from, to) in edges {
        if let (Some(&from_idx), Some(&to_idx)) =
            (id_to_idx.get(from.as_str()), id_to_idx.get(to.as_str()))
        {
            adjacency[from_idx].push(to_idx);
            indegree[to_idx] += 1;
        }
    }

    let mut placed = vec![false; n];
    let mut layers: Vec<Vec<usize>> = Vec::new();
    loop {
        let mut ready: Vec<usize> = (0..n).filter(|&i| !placed[i] && indegree[i] == 0).collect();
        if ready.is_empty() {
            break;
        }
        ready.sort_by(|&a, &b| nodes[a].0.cmp(&nodes[b].0));
        for &i in &ready {
            placed[i] = true;
        }
        for &i in &ready {
            for &j in &adjacency[i] {
                if !placed[j] {
                    indegree[j] = indegree[j].saturating_sub(1);
                }
            }
        }
        layers.push(ready);
    }

    let mut remaining: Vec<usize> = (0..n).filter(|&i| !placed[i]).collect();
    if !remaining.is_empty() {
        remaining.sort_by(|&a, &b| nodes[a].0.cmp(&nodes[b].0));
        layers.push(remaining);
    }
    layers
}

/// Splits one topological layer's node indices into one or more visual rows of at most
/// [`MAX_NODES_PER_ROW`], preserving [`layer_nodes`]'s existing deterministic order (chunking,
/// never re-sorting). Each returned row is paired with whether it's the layer's *first* row —
/// [`render_graph_svg`] uses that to pick [`SVG_LAYER_GAP`] (a new DAG layer) vs.
/// [`SVG_ROW_GAP`] (a continuation row of the same layer) above it.
fn wrap_layer_into_rows(layer: &[usize]) -> Vec<(Vec<usize>, bool)> {
    layer
        .chunks(MAX_NODES_PER_ROW.max(1))
        .enumerate()
        .map(|(i, chunk)| (chunk.to_vec(), i == 0))
        .collect()
}

/// Generic deterministic `(nodes, edges) -> SVG` renderer — no Mermaid parsing, no headless
/// browser, no Node.js dependency (all ruled out: this project's own reproducible-build and
/// zero-`unsafe`/pure-function conventions rule out shelling out to `mmdc`/puppeteer, and no
/// mature pure-Rust Mermaid renderer exists to depend on). Lays nodes out in layers via
/// [`layer_nodes`], wraps any layer over [`MAX_NODES_PER_ROW`] into multiple visual rows via
/// [`wrap_layer_into_rows`] (RFC 0083 Phase 4 — a real 46-node System Context layer used to
/// render as one unreadably wide row), centers each row horizontally, and draws a straight arrow
/// from the bottom of each source box to the top of each target box. `nodes` is `(id, label)`;
/// `edges` is `(from_id, to_id)` referencing those same ids — an edge referencing an id not in
/// `nodes` is silently skipped rather than drawn as a dangling line. Empty `nodes` renders an
/// empty string; callers that have an honest "nothing to show" case (like
/// [`render_system_context_svg`]) should check that themselves rather than writing an empty SVG
/// file to disk.
fn render_graph_svg(nodes: &[(String, String)], edges: &[(String, String)]) -> String {
    if nodes.is_empty() {
        return String::new();
    }

    let layers = layer_nodes(nodes, edges);
    let visual_rows: Vec<(Vec<usize>, bool)> = layers
        .iter()
        .flat_map(|layer| wrap_layer_into_rows(layer))
        .collect();
    let max_row_len = visual_rows
        .iter()
        .map(|(r, _)| r.len())
        .max()
        .unwrap_or(1)
        .max(1);
    let width = SVG_MARGIN * 2.0
        + max_row_len as f64 * SVG_NODE_WIDTH
        + (max_row_len - 1) as f64 * SVG_NODE_GAP;

    let mut row_tops: Vec<f64> = Vec::with_capacity(visual_rows.len());
    let mut cursor = SVG_MARGIN;
    for (i, (_, starts_new_layer)) in visual_rows.iter().enumerate() {
        if i > 0 {
            cursor += SVG_NODE_HEIGHT
                + if *starts_new_layer {
                    SVG_LAYER_GAP
                } else {
                    SVG_ROW_GAP
                };
        }
        row_tops.push(cursor);
    }
    let height = cursor + SVG_NODE_HEIGHT + SVG_MARGIN;

    let mut positions: HashMap<&str, (f64, f64)> = HashMap::new();
    let mut boxes = String::new();
    for (row_idx, (row, _)) in visual_rows.iter().enumerate() {
        let y = row_tops[row_idx];
        let row_width =
            row.len() as f64 * SVG_NODE_WIDTH + (row.len().saturating_sub(1)) as f64 * SVG_NODE_GAP;
        let start_x = (width - row_width) / 2.0;
        for (i, &node_idx) in row.iter().enumerate() {
            let x = start_x + i as f64 * (SVG_NODE_WIDTH + SVG_NODE_GAP);
            let (id, label) = &nodes[node_idx];
            let (cx, cy) = (x + SVG_NODE_WIDTH / 2.0, y + SVG_NODE_HEIGHT / 2.0);
            positions.insert(id.as_str(), (cx, cy));
            boxes.push_str(&format!(
                "  <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{SVG_NODE_WIDTH:.1}\" \
                 height=\"{SVG_NODE_HEIGHT:.1}\" rx=\"6\" fill=\"#eef2ff\" stroke=\"#3355bb\"/>\n"
            ));
            boxes.push_str(&format!(
                "  <text x=\"{cx:.1}\" y=\"{cy:.1}\" text-anchor=\"middle\" \
                 dominant-baseline=\"middle\" font-family=\"sans-serif\" font-size=\"12\">{}</text>\n",
                svg_escape(label)
            ));
        }
    }

    let mut lines = String::new();
    for (from, to) in edges {
        if let (Some(&(x1, y1)), Some(&(x2, y2))) =
            (positions.get(from.as_str()), positions.get(to.as_str()))
        {
            let y1 = y1 + SVG_NODE_HEIGHT / 2.0;
            let y2 = y2 - SVG_NODE_HEIGHT / 2.0;
            lines.push_str(&format!(
                "  <line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" \
                 stroke=\"#555555\" stroke-width=\"1.5\" marker-end=\"url(#arrow)\"/>\n"
            ));
        }
    }

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.1}\" height=\"{height:.1}\" \
         viewBox=\"0 0 {width:.1} {height:.1}\">\n\
         <defs><marker id=\"arrow\" markerWidth=\"10\" markerHeight=\"10\" refX=\"8\" refY=\"3\" \
         orient=\"auto\" markerUnits=\"strokeWidth\"><path d=\"M0,0 L0,6 L9,3 z\" fill=\"#555555\"/>\
         </marker></defs>\n{lines}{boxes}</svg>\n"
    )
}

/// Render a C4 Component view (RFC 0068 §18): for each `Crate` (Container), link through to the
/// `Rollup` (RFC 0044) whose group covers that exact directory, if one was compiled. Matches by
/// exact `rollup.name == crate.path` — both are already computed the same way (path relative to
/// wherever `ekos recover` was invoked from), confirmed against this repo's own real compiled
/// data before relying on it, not assumed. A crate with no matching rollup is a real, honest,
/// non-fatal outcome — RFC 0044's own `synthesize_rollups` only creates a `Rollup` for a group of
/// 2+ member files, so a crate with 0-1 files legitimately has none — but (RFC 0083 Phase 4) it is
/// still reported by name and count below the linked list, never silently vanishing with zero
/// trace the way it used to.
fn render_component_view(
    crates: &[&KirObject],
    objects: &[KirObject],
    page_names: &HashMap<KirId, String>,
) -> String {
    let rollups_by_name: HashMap<&str, &KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Rollup"))
        .map(|o| (o.name.as_str(), o))
        .collect();

    let mut sorted_crates: Vec<&&KirObject> = crates.iter().collect();
    sorted_crates.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = String::new();
    let mut linked = 0usize;
    let mut unmatched: Vec<&str> = Vec::new();
    for krate in sorted_crates {
        let rollup = krate
            .properties
            .get("path")
            .and_then(|v| v.as_str())
            .and_then(|path| rollups_by_name.get(path));
        let Some(rollup) = rollup else {
            unmatched.push(krate.name.as_str());
            continue;
        };
        let member_count = rollup
            .properties
            .get("member_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let rollup_label = match page_names.get(&rollup.id) {
            Some(f) => format!("[{} member file(s)]({f})", member_count),
            None => format!("{member_count} member file(s)"),
        };
        out.push_str(&format!("- **{}** — {rollup_label}\n", krate.name));
        linked += 1;
    }

    const UNMATCHED_SAMPLE: usize = 10;
    match (linked, unmatched.is_empty()) {
        (0, true) => out.push_str("_No crate directory matched a compiled subsystem rollup._\n\n"),
        (_, false) => {
            let sample = unmatched
                .iter()
                .take(UNMATCHED_SAMPLE)
                .copied()
                .collect::<Vec<_>>();
            let more = unmatched.len().saturating_sub(sample.len());
            let more_suffix = if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            };
            out.push_str(&format!(
                "_{} crate(s) have no matching subsystem rollup — fewer than RFC 0044's 2-member \
                 threshold, or no manifest `path` property compiled, not silently dropped: \
                 {}{more_suffix}._\n\n",
                unmatched.len(),
                sample.join(", ")
            ));
        }
        (_, true) => out.push('\n'),
    }
    out
}

/// Render the whole-workspace ER diagram as a standalone HTML page (`er-diagram.html`) — same
/// data as [`render_er_diagram`], wrapped in the same self-contained document
/// [`render_html_object_page`] uses, with the diagram source shown in a `<pre>` block rather than
/// live-rendered (see [`render_html_object_page`]'s doc comment for why).
pub fn render_html_er_diagram_page(
    tables: &[KirObject],
    relationships: &[KirRelationship],
) -> RenderedPage {
    let diagram = render_er_diagram(tables, relationships);
    let body = format!(
        "<h1>Entity-Relationship Diagram</h1>\n<pre class=\"mermaid-source\"><code>{}</code></pre>\n",
        html_escape(strip_mermaid_fence(&diagram))
    );
    RenderedPage {
        file_name: "er-diagram.html".to_string(),
        content: html_document("Entity-Relationship Diagram", &body),
    }
}

/// Render an index page linking to every generated object page, grouped by kind (alphabetical
/// within each group) — the same "list, don't hide" grouping `render_object_page` uses for
/// relationships, applied at the doc-set level.
/// `diagrams` are whole-workspace diagram pages (e.g. the ER diagram) that don't belong to any
/// single object, listed in their own `## Diagrams` section ahead of the per-kind object groups —
/// as `(title, file_name)` pairs, rendered only when non-empty rather than an always-present
/// empty section.
pub fn render_index_page(
    pages: &[(ObjectKind, String, String)],
    diagrams: &[(String, String)],
) -> RenderedPage {
    let mut out = String::new();
    out.push_str("# Generated Documentation\n\n");

    if !diagrams.is_empty() {
        out.push_str("## Diagrams\n\n");
        for (title, file_name) in diagrams {
            out.push_str(&format!("- [{title}]({file_name})\n"));
        }
        out.push('\n');
    }

    if pages.is_empty() {
        out.push_str(
            "_No documented objects yet — run `ekos build && ekos recover && \
            ekos resolve && ekos compile && ekos commit` first._\n",
        );
        return RenderedPage {
            file_name: "index.md".to_string(),
            content: out,
        };
    }

    let mut by_kind: HashMap<String, Vec<&(ObjectKind, String, String)>> = HashMap::new();
    for entry in pages {
        by_kind.entry(entry.0.to_string()).or_default().push(entry);
    }
    let mut kinds: Vec<&String> = by_kind.keys().collect();
    kinds.sort();

    for kind in kinds {
        let mut entries = by_kind[kind].clone();
        entries.sort_by(|a, b| a.1.cmp(&b.1));
        out.push_str(&format!("## {kind} ({})\n\n", entries.len()));
        for (_, name, file_name) in entries {
            out.push_str(&format!("- [{name}]({file_name})\n"));
        }
        out.push('\n');
    }

    RenderedPage {
        file_name: "index.md".to_string(),
        content: out,
    }
}

/// HTML counterpart of [`render_index_page`] — same grouping/ordering, `index.html` instead of
/// `index.md`, and pages/diagrams are expected to already carry `.html` file names (the CLI is
/// responsible for passing the format-appropriate file name list to whichever index renderer
/// matches the format being generated).
pub fn render_html_index_page(
    pages: &[(ObjectKind, String, String)],
    diagrams: &[(String, String)],
) -> RenderedPage {
    let mut body = String::new();

    if !diagrams.is_empty() {
        body.push_str("<h2>Diagrams</h2>\n<ul>\n");
        for (title, file_name) in diagrams {
            body.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>\n",
                html_escape(file_name),
                html_escape(title)
            ));
        }
        body.push_str("</ul>\n");
    }

    if pages.is_empty() {
        body.push_str(
            "<p class=\"empty\">No documented objects yet — run <code>ekos build &amp;&amp; \
            ekos recover &amp;&amp; ekos resolve &amp;&amp; ekos compile &amp;&amp; ekos commit\
            </code> first.</p>\n",
        );
        return RenderedPage {
            file_name: "index.html".to_string(),
            content: html_document("Generated Documentation", &body),
        };
    }

    let mut by_kind: HashMap<String, Vec<&(ObjectKind, String, String)>> = HashMap::new();
    for entry in pages {
        by_kind.entry(entry.0.to_string()).or_default().push(entry);
    }
    let mut kinds: Vec<&String> = by_kind.keys().collect();
    kinds.sort();

    for kind in kinds {
        let mut entries = by_kind[kind].clone();
        entries.sort_by(|a, b| a.1.cmp(&b.1));
        body.push_str(&format!(
            "<h2>{} ({})</h2>\n<ul>\n",
            html_escape(kind),
            entries.len()
        ));
        for (_, name, file_name) in entries {
            body.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>\n",
                html_escape(file_name),
                html_escape(name)
            ));
        }
        body.push_str("</ul>\n");
    }

    RenderedPage {
        file_name: "index.html".to_string(),
        content: html_document("Generated Documentation", &body),
    }
}

/// Escape text for safe embedding in HTML — object names, property values, and evidence
/// fragments all come from arbitrary source/SQL/document content and could contain `<`, `>`,
/// `&`, or `"`, any of which would otherwise break page structure.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Strip the ` ```mermaid ` / ` ``` ` fence `render_mermaid_graph`/`render_er_diagram` wrap their
/// output in, leaving just the raw Mermaid body — used when embedding into an HTML `<pre>` block,
/// which doesn't want Markdown code-fence syntax.
fn strip_mermaid_fence(fenced: &str) -> &str {
    fenced
        .strip_prefix("```mermaid\n")
        .unwrap_or(fenced)
        .strip_suffix("```\n")
        .or_else(|| fenced.strip_suffix("```"))
        .unwrap_or(fenced)
}

/// Compact, self-contained CSS for generated HTML pages — inspired by (not copied from)
/// `docs/assets/theme.css`'s dark neon/glass palette, kept independent so this crate has zero
/// build-time dependency on this repo's own `docs/` directory: `ekos docs generate` runs in
/// arbitrary user workspaces that won't have this repo's files available.
const EMBEDDED_CSS: &str = r#"
:root{ --bg:#f6f4fc; --ink:#171326; --ink-soft:#5c5578; --rule:#e3ddf5; --accent:#7c3aed; --code-bg:#130f22; --code-fg:#e9e4f7; }
@media (prefers-color-scheme: dark){
  :root{ --bg:#0b0a12; --ink:#f3f1fa; --ink-soft:#a7a2c4; --rule:rgba(255,255,255,0.12); --accent:#9945ff; --code-bg:#100d1c; --code-fg:#e9e4f7; }
}
body{ background:var(--bg); color:var(--ink); font:16px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; max-width:52rem; margin:0 auto; padding:2.5rem 1.5rem 6rem; }
h1,h2,h3{ line-height:1.25; }
h1{ font-size:1.9rem; } h2{ font-size:1.3rem; margin-top:2.2rem; border-bottom:1px solid var(--rule); padding-bottom:0.4rem; } h3{ font-size:1.05rem; color:var(--ink-soft); }
.kind{ color:var(--ink-soft); font-weight:400; }
a{ color:var(--accent); }
code{ background:var(--code-bg); color:var(--code-fg); padding:0.1em 0.4em; border-radius:4px; font-size:0.88em; }
pre{ background:var(--code-bg); color:var(--code-fg); padding:1rem; border-radius:8px; overflow-x:auto; }
pre code{ background:none; padding:0; }
table{ border-collapse:collapse; width:100%; }
th,td{ text-align:left; padding:0.4rem 0.6rem; border-bottom:1px solid var(--rule); vertical-align:top; }
ul{ padding-left:1.3rem; }
.empty{ color:var(--ink-soft); font-style:italic; }
"#;

/// Wrap `body` (already-built HTML fragments) in a complete, self-contained HTML document.
fn html_document(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
        <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
        <title>{}</title>\n<style>{EMBEDDED_CSS}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        html_escape(title)
    )
}

fn format_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Filesystem-safe slug: lowercase, non-alphanumeric runs collapse to `-`.
fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

// ── RFC 0037 — curated documentation set (README/Architecture/API/SequenceDiagrams) ─────────
//
// A second, curated output *shape* over the same compiled objects/relationships every renderer
// above already reads — `--layout curated` on the CLI side, alongside (never replacing) the
// per-object `--layout objects` default. Four fixed-name files instead of one page per object.

/// Whether `kind` is a Transformation IR data-flow edge (RFC 0027's `lower_to_kir`). Kept private
/// and duplicated from `ekos-dbt-gen`'s public `is_feeds_into` rather than depending on that
/// crate — `docs-gen` is the more fundamental, widely-depended-on crate; a dependency the other
/// direction would be backwards.
fn is_feeds_into(kind: &RelationshipKind) -> bool {
    matches!(kind, RelationshipKind::Custom(s) if s.as_str() == "FeedsInto")
}

/// RFC 0075: a `TransformNode` Source node's unambiguous, name-matched link to the real `Table`/
/// `Dataset` it reads (`ekos_semantic::data_lineage::link_transform_nodes_to_tables`).
fn is_reads_from(kind: &RelationshipKind) -> bool {
    matches!(kind, RelationshipKind::Custom(s) if s.as_str() == "ReadsFrom")
}

/// RFC 0075: the `Sink`-side counterpart to [`is_reads_from`].
fn is_writes_to(kind: &RelationshipKind) -> bool {
    matches!(kind, RelationshipKind::Custom(s) if s.as_str() == "WritesTo")
}

/// RFC 0075 Data Domains: groups compiled data stores by the schema/database qualifier already
/// present in their own `name` when the source DDL wrote one (e.g. `sales.orders` → domain
/// `sales`) — reusing structure the store's name already carries rather than adding a new
/// extractor. Unqualified names (the common case for the real fixtures this session tested
/// against — neither ships schema-qualified DDL) are counted and reported honestly, not silently
/// dropped or guessed into a domain.
fn data_domains_section(stores: &[&KirObject]) -> String {
    if stores.is_empty() {
        return "_not yet computed — no compiled data stores to derive a domain from._\n\n"
            .to_string();
    }

    let mut by_domain: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut unqualified = 0usize;
    for store in stores {
        match store.name.rsplit_once('.') {
            Some((domain, table)) => by_domain.entry(domain).or_default().push(table),
            None => unqualified += 1,
        }
    }

    if by_domain.is_empty() {
        return format!(
            "_not yet computed — none of the {unqualified} compiled table name(s) is \
             schema-qualified (e.g. `sales.orders`); EKOS derives a domain from that qualifier \
             when the source DDL provides one, not from a separate extractor or human curation. \
             No domain grouping is possible for unqualified names without one or the other \
             (RFC 0068 §22)._\n\n"
        );
    }

    let mut out = String::new();
    for (domain, mut tables) in by_domain {
        tables.sort_unstable();
        out.push_str(&format!("- **{domain}** — {}\n", tables.join(", ")));
    }
    if unqualified > 0 {
        out.push_str(&format!(
            "\n_{unqualified} compiled table name(s) have no schema qualifier and aren't \
             grouped above._\n"
        ));
    }
    out.push('\n');
    out
}

fn count_by_kind(
    objects: &[KirObject],
    include: impl Fn(&ObjectKind) -> bool,
) -> Vec<(String, usize)> {
    let mut by_kind: HashMap<String, usize> = HashMap::new();
    for o in objects {
        if include(&o.kind) {
            *by_kind.entry(o.kind.to_string()).or_default() += 1;
        }
    }
    let mut rows: Vec<(String, usize)> = by_kind.into_iter().collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// Render `README.md`: object-kind counts, real contributors (`Person` objects with a
/// `commit_count` property, from `git_analyzer.rs`), and links to the other three curated docs.
/// Every fact traces to compiled data; an empty ledger renders an honest placeholder, never a
/// fabricated summary.
pub fn render_readme(objects: &[KirObject]) -> RenderedPage {
    let mut out = String::from(
        "# Project Overview\n\n_Generated by `ekos docs generate --layout curated` — every fact \
         below traces to compiled ledger data, nothing invented._\n\n",
    );

    out.push_str("## Components\n\n");
    let counts = count_by_kind(objects, |_| true);
    if counts.is_empty() {
        out.push_str(
            "_No compiled objects yet — run `ekos build && ekos recover && ekos resolve && \
             ekos compile && ekos commit` first._\n\n",
        );
    } else {
        for (kind, count) in &counts {
            out.push_str(&format!("- **{kind}**: {count}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Contributors\n\n");
    let mut contributors: Vec<&KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Person)
        .collect();
    if contributors.is_empty() {
        out.push_str("_No contributor data compiled._\n\n");
    } else {
        contributors.sort_by(|a, b| {
            let count_of = |o: &KirObject| {
                o.properties
                    .get("commit_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
            };
            count_of(b)
                .cmp(&count_of(a))
                .then_with(|| a.name.cmp(&b.name))
        });
        for c in &contributors {
            match c.properties.get("commit_count").and_then(|v| v.as_i64()) {
                Some(n) => out.push_str(&format!("- {} ({n} commits)\n", c.name)),
                None => out.push_str(&format!("- {}\n", c.name)),
            }
        }
        out.push('\n');
    }

    out.push_str("## Documentation\n\n");
    out.push_str("- [Architecture](Architecture.md)\n");
    out.push_str("- [API](API.md)\n");
    out.push_str("- [Sequence Diagrams](SequenceDiagrams.md)\n");

    RenderedPage {
        file_name: "README.md".to_string(),
        content: out,
    }
}

/// Kinds documented in depth elsewhere in the curated set (individual detail pages written
/// alongside `Architecture.md`/`API.md` by `--layout curated`, RFC 0042) — the `## Components`
/// count for these links out to where the real per-entity listing/diagram already lives instead
/// of dumping a potentially thousand-line inline list into `Architecture.md` itself.
fn components_cross_reference(kind: &str) -> Option<&'static str> {
    match kind {
        "RustModule" | "RustSymbol" | "PythonModule" | "PythonSymbol" => Some("[API.md](API.md)"),
        "Crate" => Some("below, `## Crate & Workspace Topology`"),
        "Technology" => Some("below, `## Technology Inventory`"),
        "Pipeline" => Some("below, `## CI/CD Pipelines`"),
        "Rollup" => Some("below, `## Subsystems`"),
        _ => None,
    }
}

/// Render RFC 0068 §22 Data Architecture: real compiled `Table`/`Dataset` objects (data stores,
/// with each one's real foreign-key edge count) and real compiled Transformation IR data flows
/// (RFC 0027, `Custom("FeedsInto")` edges) — link-through to `SequenceDiagrams.md`'s existing
/// "Data-Flow Sequences" section rather than duplicating it, the same precedent
/// [`render_architecture`]'s Runtime View section immediately above already established.
/// `Table`/`Dataset` aren't [`is_entity_page_kind`] — no curated per-object page exists for them
/// today — so data stores are listed by name only here, not linked; linking would produce a
/// dangling reference under `--layout curated`. Domain grouping, ownership, lifecycle, and data
/// quality are RFC 0068 §22 dimensions with no compiled EKOS signal behind them yet, so each says
/// so explicitly rather than being invented — the same honest-gap convention
/// [`render_architecture_summary`] already established.
fn render_data_architecture(objects: &[KirObject], relationships: &[KirRelationship]) -> String {
    let mut out = String::new();

    let mut stores: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(o.kind, ObjectKind::Table | ObjectKind::Dataset))
        .collect();
    stores.sort_by(|a, b| a.name.cmp(&b.name));

    out.push_str("### Data Stores\n\n");
    if stores.is_empty() {
        out.push_str("_No compiled data stores (Tables/Datasets)._\n\n");
    } else {
        out.push_str(&format!(
            "{} compiled data store(s). Listed individually — no domain/system grouping is \
             extracted from source data today (see Data Domains below).\n\n",
            stores.len()
        ));
        for store in &stores {
            let fk_count = relationships
                .iter()
                .filter(|r| {
                    matches!(r.kind, RelationshipKind::ForeignKey)
                        && (r.from == store.id || r.to == store.id)
                })
                .count();
            let reads = relationships
                .iter()
                .filter(|r| is_reads_from(&r.kind) && r.to == store.id)
                .count();
            let writes = relationships
                .iter()
                .filter(|r| is_writes_to(&r.kind) && r.to == store.id)
                .count();
            out.push_str(&format!(
                "- **{}** — {fk_count} real foreign-key edge(s), read by {reads} \
                 transformation(s), written by {writes} transformation(s)\n",
                store.name
            ));
        }
        out.push('\n');
    }

    out.push_str("### Transformations & Lineage\n\n");
    // Not `is_feeds_into` alone: a workspace can compile a real, single `TransformNode` (a bare
    // `SELECT * FROM x` with no downstream step) with zero `FeedsInto` edges — a lone source/sink
    // is still a real compiled transformation, not "nothing compiled".
    let has_transform_nodes = objects
        .iter()
        .any(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "TransformNode"));
    let has_lineage_links = relationships
        .iter()
        .any(|r| is_reads_from(&r.kind) || is_writes_to(&r.kind));
    if has_transform_nodes {
        out.push_str(
            "See [SequenceDiagrams.md](SequenceDiagrams.md) for real compiled data-flow \
             sequences (RFC 0027 Transformation IR).",
        );
        if has_lineage_links {
            out.push_str(
                " `TransformNode` source/sink nodes are cross-referenced to the Data Stores \
                 above (RFC 0075) whenever their raw `object_name` matches exactly one compiled \
                 table — see each store's read/write counts. A name matching zero or more than \
                 one table (e.g. the same unqualified name in two different schemas) is \
                 deliberately left unlinked rather than guessed at.\n\n",
            );
        } else {
            out.push_str(
                " None of this workspace's `TransformNode` source/sink names matched exactly \
                 one compiled table (RFC 0075) — either no name overlaps a compiled `Table`, or \
                 every overlapping name is ambiguous across two or more tables, so nothing was \
                 linked rather than guessed at.\n\n",
            );
        }
    } else {
        out.push_str("_No transformations compiled._\n\n");
    }

    out.push_str("### Data Domains\n\n");
    out.push_str(&data_domains_section(&stores));

    out.push_str("### Ownership\n\n");
    out.push_str(
        "_not yet computed for data objects — `OwnedBy` edges are compiled from git history \
         (`git_analyzer.rs`), but only from a commit event to the contributor who authored it, \
         never onto a `File`/`Table`/`Dataset` object; there's no compiled per-file ownership \
         signal today for a data store to link to, even setting aside that `Table`/`Dataset` \
         objects also aren't yet linked to the `File` they were defined in. Two real gaps, not \
         one: (1) `git_analyzer.rs` would need to derive a per-file top-contributor \
         relationship, the way it already derives per-file `CoupledWith` coupling; (2) a data \
         store would need the same kind of name/evidence-path linkage RFC 0075 just built for \
         `TransformNode`s, but against `File` objects instead._\n\n",
    );

    out.push_str("### Lifecycle\n\n");
    out.push_str(
        "_not yet computed — blocked on the same missing `Table`\u{2192}`File` link Ownership \
         above is (a real last-modified/commit-recency signal already exists per file via git \
         history, RFC 0020's coupling analysis touches the same commit data, but nothing \
         connects a compiled data store to the file whose history that would be)._\n\n",
    );

    out.push_str("### Data Quality\n\n");
    out.push_str(
        "_not yet computed — no data-quality signal (completeness, freshness, validation-rule \
         pass/fail) is extractable from static DDL/transformation-logic recovery at all; this \
         needs runtime data profiling (row counts, null rates, constraint violations against \
         actual data), which is explicitly RFC 0068 §63 Phase 3 scope (runtime telemetry), not \
         yet built._\n\n",
    );

    out
}

/// Render `Architecture.md`: component counts (linked out to the section/page where a kind's
/// real detail lives, RFC 0042), the crate/workspace dependency topology and external-technology
/// list (`crate_topology_analyzer.rs`), CI/CD pipelines (`cicd_analyzer.rs`), the existing ER
/// diagram when `Table`/`ForeignKey` data exists, and one small Mermaid graph per *structural*
/// relationship kind. `Custom("FeedsInto")` edges are deliberately excluded here — pipeline-
/// internal step wiring belongs in `SequenceDiagrams.md`; a real Pentaho workspace has dozens of
/// `TransformNode`s, so inlining that here would make the diagram unreadable. Splitting by
/// relationship *purpose* is this RFC's answer to RFC 0035's still-open "diagram size" question,
/// for the curated layout.
pub fn render_architecture(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    layer_overrides: &[LayerOverride],
) -> RenderedPage {
    let mut out = String::from("# Architecture\n\n");
    let page_names = unique_page_file_names(objects, "md");
    let kind_by_id: HashMap<KirId, &ObjectKind> = objects.iter().map(|o| (o.id, &o.kind)).collect();

    out.push_str("## Architecture Summary\n\n");
    out.push_str(
        "_Executive Overview (RFC 0068 §14) — only fields EKOS can back with real compiled \
         evidence are populated; fields the standard names but nothing here computes yet say so \
         explicitly rather than being silently omitted or guessed at._\n\n",
    );
    out.push_str(&render_architecture_summary(objects, relationships));

    out.push_str("## System Context\n\n");
    out.push_str(
        "_C4 System Context (RFC 0068 §15) — the compiled workspace as one system, and the real \
         external technologies it depends on. One level broader than the Container view below; \
         only technologies with a real compiled dependency edge are shown, not every `Technology` \
         object that happens to exist._\n\n",
    );
    out.push_str(&render_system_context(objects, relationships));
    if system_context_graph(objects, relationships).is_some() {
        out.push_str("[System Context diagram (SVG)](system-context.svg)\n\n");
    } else {
        out.push('\n');
    }

    out.push_str("## System Decomposition\n\n");
    out.push_str(
        "_C4 Container-level decomposition (RFC 0068 §16/§68), one level inside System Context \
         above — real Backend/Frontend/Database layers, grouped from each compiled `File`/`Table` \
         object's own path or `source_system` (RFC 0056/0083), never guessed. A path can be routed \
         to a specific layer via `[[architecture.system-decomposition.overrides]]` in `ekos.toml` \
         when the convention gets a project's layout wrong._\n\n",
    );
    out.push_str(&render_system_decomposition(
        objects,
        relationships,
        layer_overrides,
    ));
    if system_decomposition_graph(objects, relationships, layer_overrides).is_some() {
        out.push_str("[System Decomposition diagram (SVG)](system-decomposition.svg)\n\n");
    } else {
        out.push('\n');
    }

    out.push_str("## Components\n\n");
    let counts = count_by_kind(objects, is_significant);
    if counts.is_empty() {
        out.push_str("_No compiled objects yet._\n\n");
    } else {
        for (kind, count) in &counts {
            match components_cross_reference(kind) {
                Some(link) => out.push_str(&format!("- **{kind}**: {count} — see {link}\n")),
                None => out.push_str(&format!("- **{kind}**: {count}\n")),
            }
        }
        out.push('\n');
    }

    out.push_str("## Subsystems\n\n");
    out.push_str(
        "_Deterministic rollups (RFC 0044) — one per directory/project group with ≥2 member \
         files, zero LLM. Each links to a detail page with real member counts and boundary \
         relationships, so a subsystem can be understood without walking every file inside it._\n\n",
    );
    let rollups: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Rollup"))
        .collect();
    if rollups.is_empty() {
        out.push_str("_No subsystem rollups compiled._\n\n");
    } else {
        let mut sorted_rollups = rollups;
        sorted_rollups.sort_by(|a, b| a.name.cmp(&b.name));
        for rollup in sorted_rollups {
            let member_count = rollup
                .properties
                .get("member_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let link = page_names.get(&rollup.id);
            let label = match link {
                Some(f) => format!("[{}]({f})", rollup.name),
                None => rollup.name.clone(),
            };
            out.push_str(&format!("- {label} — {member_count} member file(s)\n"));
        }
        out.push('\n');
    }

    out.push_str("## Crate & Workspace Topology\n\n");
    out.push_str(
        "_C4 mapping (RFC 0065 §23): each crate below is a C4 **Container** — the natural \
         deployable/buildable unit in a Rust workspace — and each entry under Technologies is a \
         C4 **External System** the workspace depends on but doesn't own._\n\n",
    );
    let crates: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Crate"))
        .collect();
    let crate_ids: HashSet<KirId> = crates.iter().map(|c| c.id).collect();
    let crate_name_by_id: HashMap<KirId, &str> =
        crates.iter().map(|c| (c.id, c.name.as_str())).collect();
    let crate_edges: Vec<&KirRelationship> = relationships
        .iter()
        .filter(|r| {
            matches!(r.kind, RelationshipKind::DependsOn)
                && crate_ids.contains(&r.from)
                && crate_ids.contains(&r.to)
        })
        .collect();
    if crates.is_empty() {
        out.push_str("_No crate/workspace manifests compiled._\n\n");
    } else if crate_edges.is_empty() {
        out.push_str(
            "_No internal (path) crate dependencies compiled among the discovered manifests._\n\n",
        );
    } else {
        out.push_str(&render_relationship_kind_graph(
            "DependsOn",
            &crate_edges,
            &crate_name_by_id,
        ));
        out.push_str("[Crate & Workspace Topology diagram (SVG)](crate-topology.svg)\n\n");
    }

    out.push_str("## Component View\n\n");
    out.push_str(
        "_C4 Component (RFC 0068 §18) — one level inside a Container. Each `Crate` below whose \
         manifest directory matches a compiled `Rollup` (RFC 0044) links through to that \
         subsystem's real member-file breakdown; a crate with no matching rollup either has too \
         few member files to summarize (RFC 0044's own ≥2-member threshold) or none were \
         compiled — not fabricated either way._\n\n",
    );
    out.push_str(&render_component_view(&crates, objects, &page_names));

    out.push_str("## Technology Inventory\n\n");
    out.push_str(
        "_C4 External System-level dependencies (RFC 0068 §61's Technology Inventory), each \
         linked to its own detail page where one was compiled._\n\n",
    );
    let name_by_id: HashMap<KirId, &str> =
        objects.iter().map(|o| (o.id, o.name.as_str())).collect();
    let technologies: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Technology"))
        .collect();
    if technologies.is_empty() {
        out.push_str("_No technology dependencies compiled._\n\n");
    } else {
        for tech in &technologies {
            // Deduplicated by name: `KirRelationship::new` mints a fresh random id every time
            // (unlike `KirObject`'s deterministic ids), so `append_relationship`'s `(id,
            // content_signature)` versioning never recognizes a logically-identical `DependsOn`
            // edge re-derived by a later `recover`/`commit` as "the same one" — real duplicates
            // accumulate in the ledger across repeated commits (found live verifying this exact
            // view; the underlying ledger-level gap is real and larger than this view, tracked
            // separately in TODO.md, not silently fixed everywhere here).
            let mut dependents: Vec<&str> = relationships
                .iter()
                .filter(|r| r.to == tech.id && matches!(r.kind, RelationshipKind::DependsOn))
                .filter_map(|r| name_by_id.get(&r.from).copied())
                .collect();
            dependents.sort_unstable();
            dependents.dedup();
            let used_by = if dependents.is_empty() {
                "_no linked files_".to_string()
            } else {
                dependents.join(", ")
            };
            let label = match page_names.get(&tech.id) {
                Some(f) => format!("[{}]({f})", tech.name),
                None => format!("**{}**", tech.name),
            };
            out.push_str(&format!("- {label} — used by: {used_by}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Runtime View\n\n");
    out.push_str(
        "_Basic Runtime View (RFC 0068 §20) — real behavior, not structure. `SequenceDiagrams.md` \
         (generated alongside this page) already renders every real compiled call/data-flow \
         sequence (RFC 0041's `Calls` graph, RFC 0027's Transformation IR); this section links \
         through rather than duplicating it. Naming *which* of those are the system's important \
         business scenarios (RFC 0068's own examples: \"Create Order\", \"Process Payment\") needs \
         either an LLM read of real intent or human curation — neither happens in this \
         deterministic view, so no scenario names are invented here._\n\n",
    );
    let has_call_or_flow_edges = relationships
        .iter()
        .any(|r| matches!(r.kind, RelationshipKind::Calls) || is_feeds_into(&r.kind));
    if has_call_or_flow_edges {
        out.push_str(
            "See [SequenceDiagrams.md](SequenceDiagrams.md) for the real compiled sequences.\n\n",
        );
    } else {
        out.push_str("_No call or data-flow sequences compiled._\n\n");
    }

    out.push_str("## Data Architecture\n\n");
    out.push_str(
        "_RFC 0068 §22 (\"A major EKOS capability\") — real compiled data stores and real \
         compiled transformations/lineage; domain grouping, ownership, lifecycle, and data \
         quality each say explicitly why they're not computed yet rather than being guessed \
         at._\n\n",
    );
    out.push_str(&render_data_architecture(objects, relationships));

    out.push_str("## Open Questions\n\n");
    out.push_str(
        "_Explicit knowledge gaps (RFC 0065 §17) — not errors, and not silently dropped: each \
         entry below is something a deterministic pass found it could not resolve, evidence-backed \
         like everything else on this page. Unless resolved, they stay here rather than being \
         guessed at._\n\n",
    );
    let gaps: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "ArchitectureGap"))
        .collect();
    if gaps.is_empty() {
        out.push_str("_No open architecture questions compiled._\n\n");
    } else {
        for gap in &gaps {
            let question = gap
                .properties
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or(&gap.name);
            let affected = gap
                .properties
                .get("affected_crate")
                .and_then(|v| v.as_str());
            match affected {
                Some(crate_name) => {
                    out.push_str(&format!("- {question} (affects `{crate_name}`)\n"))
                }
                None => out.push_str(&format!("- {question}\n")),
            }
        }
        out.push('\n');
    }

    out.push_str("## CI/CD Pipelines\n\n");
    let pipelines: Vec<&KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Pipeline)
        .collect();
    if pipelines.is_empty() {
        out.push_str("_No CI/CD pipeline definitions compiled._\n\n");
    } else {
        for pipeline in &pipelines {
            out.push_str(&format!("### {}\n\n", pipeline.name));
            if let Some(triggers) = pipeline
                .properties
                .get("triggers")
                .and_then(|v| v.as_array())
            {
                let names: Vec<&str> = triggers.iter().filter_map(|v| v.as_str()).collect();
                if !names.is_empty() {
                    out.push_str(&format!("Triggers: `{}`\n\n", names.join("`, `")));
                }
            }
            if let Some(jobs) = pipeline.properties.get("jobs").and_then(|v| v.as_array()) {
                for job in jobs {
                    let job_name = job.get("name").and_then(|v| v.as_str()).unwrap_or("job");
                    out.push_str(&format!("- **{job_name}**\n"));
                    if let Some(steps) = job.get("steps").and_then(|v| v.as_array()) {
                        for step in steps {
                            if let Some(s) = step.as_str() {
                                out.push_str(&format!("  - {s}\n"));
                            }
                        }
                    }
                }
            }
            out.push('\n');
        }
    }

    out.push_str("## Entity Relationships\n\n");
    let tables: Vec<KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .cloned()
        .collect();
    let has_foreign_key = relationships
        .iter()
        .any(|r| matches!(r.kind, RelationshipKind::ForeignKey));
    if tables.is_empty() || !has_foreign_key {
        out.push_str("_No table foreign-key relationships compiled._\n\n");
    } else {
        out.push_str(&render_er_diagram(&tables, relationships));
        out.push('\n');
    }

    out.push_str("## Dependency Graph\n\n");
    let mut by_kind: HashMap<String, Vec<&KirRelationship>> = HashMap::new();
    for rel in relationships {
        if is_feeds_into(&rel.kind) {
            continue;
        }
        by_kind.entry(rel.kind.to_string()).or_default().push(rel);
    }
    if by_kind.is_empty() {
        out.push_str("_No structural relationships compiled._\n\n");
    } else {
        let mut kinds: Vec<&String> = by_kind.keys().collect();
        kinds.sort();
        for kind in kinds {
            out.push_str(&format!("### {kind}\n\n"));
            let rels = &by_kind[kind];
            // A single relationship kind can itself be too large to render usefully — found by
            // running this against a real Pentaho+PDF workspace, where `Contains` alone (PDF
            // pages/sections) produced 74 edges. Excluding `FeedsInto` wasn't enough; the cap
            // applies per kind, not just to the one kind known in advance to be large.
            const MAX_GRAPH_EDGES: usize = 20;
            // A page-count budget, not a hard cap: for oversized kinds this still prints a real
            // linked sample instead of only a sentence pointing at a different CLI invocation —
            // each endpoint links to that object's own detail page (written alongside this file
            // by `--layout curated`, RFC 0042), which shows its full relationship list.
            const SAMPLE_EDGES: usize = 15;
            if rels.len() > MAX_GRAPH_EDGES {
                out.push_str(&format!(
                    "_{} `{kind}` relationships compiled — diagram omitted, too large to render \
                     usefully. First {} shown below; every object's own detail page (linked) \
                     lists its full relationship set._\n\n",
                    rels.len(),
                    rels.len().min(SAMPLE_EDGES)
                ));
                for rel in rels.iter().take(SAMPLE_EDGES) {
                    let from_label = name_by_id.get(&rel.from).copied().unwrap_or("unknown");
                    let to_label = name_by_id.get(&rel.to).copied().unwrap_or("unknown");
                    // Only link when curated actually writes a page for that endpoint's kind
                    // (`is_entity_page_kind`) — e.g. `File`/`Person` endpoints never get one, so
                    // linking them here would point at a file that was never written.
                    let from_link = kind_by_id
                        .get(&rel.from)
                        .filter(|k| is_entity_page_kind(k))
                        .and_then(|_| page_names.get(&rel.from));
                    let to_link = kind_by_id
                        .get(&rel.to)
                        .filter(|k| is_entity_page_kind(k))
                        .and_then(|_| page_names.get(&rel.to));
                    let from_md = match from_link {
                        Some(f) => format!("[{from_label}]({f})"),
                        None => from_label.to_string(),
                    };
                    let to_md = match to_link {
                        Some(f) => format!("[{to_label}]({f})"),
                        None => to_label.to_string(),
                    };
                    out.push_str(&format!("- {from_md} → {to_md}\n"));
                }
                out.push('\n');
            } else {
                out.push_str(&render_relationship_kind_graph(kind, rels, &name_by_id));
                out.push('\n');
            }
        }
    }

    RenderedPage {
        file_name: "Architecture.md".to_string(),
        content: out,
    }
}

/// A whole-set Mermaid `graph TD` over every relationship of one kind — unlike
/// [`render_mermaid_graph`], not centered on a single object; used by
/// [`render_architecture`]'s per-relationship-kind `## Dependency Graph` subsections.
fn render_relationship_kind_graph(
    kind_label: &str,
    rels: &[&KirRelationship],
    name_by_id: &HashMap<KirId, &str>,
) -> String {
    let mut out = String::from("```mermaid\ngraph TD\n");
    let mut seen: HashSet<KirId> = HashSet::new();
    for rel in rels {
        for id in [rel.from, rel.to] {
            if seen.insert(id) {
                let label = name_by_id.get(&id).copied().unwrap_or("unknown");
                out.push_str(&format!(
                    "    {}[\"{}\"]\n",
                    mermaid_node_id(&id),
                    mermaid_escape_label(label)
                ));
            }
        }
        let arrow = mermaid_arrow(&rel.kind);
        out.push_str(&format!(
            "    {} {arrow}|{kind_label}| {}\n",
            mermaid_node_id(&rel.from),
            mermaid_node_id(&rel.to)
        ));
    }
    out.push_str("```\n");
    out
}

fn is_symbol_kind(kind: &ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Custom(s)
            if s == "RustSymbol" || s == "PythonSymbol" || s == "ElixirSymbol" || s == "JsSymbol"
    )
}

/// Render `API.md`: real `Custom("RustSymbol")`/`Custom("PythonSymbol")`/`Custom("ElixirSymbol")`
/// program-entity objects (`rust_analyzer.rs`/`python_analyzer.rs`/`elixir_analyzer.rs`, RFC
/// 0041/0038-0040/0081) — each carrying a `kind` property (function/struct/enum/trait/class/…) —
/// grouped by their real immediate `Contains` parent: a `File` for Rust/Python
/// (`Custom("RustModule")`/`Custom("PythonModule")` represent `use`/import targets instead, a
/// `DependsOn` from the file, not a `Contains` into it), or an `ElixirModule` for Elixir — a real
/// structural difference (Elixir's own module system is the direct container of its functions;
/// `elixir_analyzer.rs` emits `File Contains Module Contains Symbol`, not `File Contains Symbol`
/// directly), grouping by the more meaningful unit for that language rather than forcing every
/// language into file-shaped grouping. Each symbol links to its own detail page (written alongside
/// this file by `--layout curated`, RFC 0042). Falls back to the legacy `File.symbols` text-scan
/// (bare identifier names, no `kind`, no links) only when zero real symbol objects are compiled,
/// so a workspace with no real-AST-analyzed language still gets *something* rather than an empty
/// page.
pub fn render_api(objects: &[KirObject], relationships: &[KirRelationship]) -> RenderedPage {
    let mut out = String::from(
        "# API\n\n_Program entities (functions, structs, enums, traits, classes, …) compiled \
         from real Rust/Python/Elixir source analysis, grouped by containing file or module. \
         Each entity links to its own detail page (relationships, evidence, 1-hop diagram), \
         written alongside this file. Real `Api`/`Service` objects, if a future connector ever \
         compiles them, would render here directly._\n\n",
    );

    let symbols: Vec<&KirObject> = objects.iter().filter(|o| is_symbol_kind(&o.kind)).collect();

    if symbols.is_empty() {
        return render_api_from_legacy_file_symbols(objects, out);
    }

    let page_names = unique_page_file_names(objects, "md");
    let file_by_id: HashMap<KirId, &KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::File)
        .map(|o| (o.id, o))
        .collect();
    let elixir_module_by_id: HashMap<KirId, &KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "ElixirModule"))
        .map(|o| (o.id, o))
        .collect();
    let mut containing_context: HashMap<KirId, KirId> = HashMap::new();
    for rel in relationships {
        if matches!(rel.kind, RelationshipKind::Contains)
            && (file_by_id.contains_key(&rel.from) || elixir_module_by_id.contains_key(&rel.from))
        {
            containing_context.insert(rel.to, rel.from);
        }
    }

    let mut by_module: BTreeMap<String, Vec<&KirObject>> = BTreeMap::new();
    for sym in &symbols {
        let module_name = containing_context
            .get(&sym.id)
            .and_then(|cid| file_by_id.get(cid).or_else(|| elixir_module_by_id.get(cid)))
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "(containing file not compiled)".to_string());
        by_module.entry(module_name).or_default().push(sym);
    }

    for (module_name, mut syms) in by_module {
        out.push_str(&format!("## {module_name}\n\n"));
        syms.sort_by(|a, b| a.name.cmp(&b.name));
        for sym in syms {
            let entity_kind = sym
                .properties
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("symbol");
            match page_names.get(&sym.id) {
                Some(link) => {
                    out.push_str(&format!("- `{entity_kind}` [`{}`]({link})\n", sym.name))
                }
                None => out.push_str(&format!("- `{entity_kind}` `{}`\n", sym.name)),
            }
        }
        out.push('\n');
    }

    RenderedPage {
        file_name: "API.md".to_string(),
        content: out,
    }
}

/// RFC 0037's original renderer: bare identifier names from `File.symbols` (a text scan for
/// declaration-line prefixes, `plugins/file/src/lib.rs`). Kept only as a fallback for workspaces
/// with no compiled `RustSymbol`/`PythonSymbol` data — see [`render_api`]'s doc comment.
fn render_api_from_legacy_file_symbols(objects: &[KirObject], mut out: String) -> RenderedPage {
    out.push_str(
        "_No `RustSymbol`/`PythonSymbol` data compiled — falling back to symbol names only, \
         extracted via a lightweight text scan for declaration-line prefixes (`fn `, `def `, \
         `class `, `func `, `interface `). Not a parsed API spec, no links._\n\n",
    );

    let mut files_with_symbols: Vec<&KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::File)
        .filter(|o| {
            o.properties
                .get("symbols")
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty())
        })
        .collect();

    if files_with_symbols.is_empty() {
        out.push_str("_No API surface data compiled._\n");
        return RenderedPage {
            file_name: "API.md".to_string(),
            content: out,
        };
    }

    files_with_symbols.sort_by(|a, b| a.name.cmp(&b.name));
    for file in &files_with_symbols {
        out.push_str(&format!("## {}\n\n", file.name));
        if let Some(symbols) = file.properties.get("symbols").and_then(|v| v.as_array()) {
            for symbol in symbols {
                if let Some(s) = symbol.as_str() {
                    out.push_str(&format!("- `{s}`\n"));
                }
            }
        }
        out.push('\n');
    }

    RenderedPage {
        file_name: "API.md".to_string(),
        content: out,
    }
}

/// The part of a `TransformNode` object's name before its trailing `:{node_index}`
/// (`transform_ir.rs::lower_to_kir` names every node `{source_path}:{index}`) — used to group
/// nodes back into their originating pipeline.
fn transform_node_origin(name: &str) -> &str {
    name.rsplit_once(':').map(|(path, _)| path).unwrap_or(name)
}

fn sequence_participant_line(node: &KirObject) -> String {
    format!(
        "    participant {} as \"{}\"\n",
        mermaid_node_id(&node.id),
        mermaid_escape_label(&node.name)
    )
}

/// Render `SequenceDiagrams.md`: two independent sections. `## Data-Flow Sequences` is one
/// Mermaid `sequenceDiagram` per compiled Transformation IR pipeline (grouped by origin), one
/// message per `FeedsInto` edge within that origin, labeled with the target node's `node_type` —
/// explicitly labeled as data flow, not a code call sequence. `## Call Sequences` (RFC 0042) is
/// the genuine code call sequence RFC 0037 didn't have data for: real `RelationshipKind::Calls`
/// edges (`rust_analyzer.rs`'s `CallVisitor`), grouped by the caller's containing module, one
/// small `sequenceDiagram` per module with calls.
pub fn render_sequence_diagrams(
    objects: &[KirObject],
    relationships: &[KirRelationship],
) -> RenderedPage {
    let mut out = String::from("# Sequence Diagrams\n\n");

    out.push_str(
        "## Data-Flow Sequences\n\n_Rendered from Transformation IR `FeedsInto` edges — a \
         data-flow sequence between compiled pipeline steps, not a code call sequence._\n\n",
    );

    let nodes: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "TransformNode"))
        .collect();
    if nodes.is_empty() {
        out.push_str("_No transformation pipelines compiled._\n\n");
        return render_call_sequences_section(objects, relationships, out);
    }

    let id_to_node: HashMap<KirId, &KirObject> = nodes.iter().map(|o| (o.id, *o)).collect();

    let mut origins: Vec<&str> = nodes
        .iter()
        .map(|o| transform_node_origin(&o.name))
        .collect();
    origins.sort();
    origins.dedup();

    for origin in origins {
        out.push_str(&format!("## {origin}\n\n"));
        let origin_ids: HashSet<KirId> = nodes
            .iter()
            .filter(|o| transform_node_origin(&o.name) == origin)
            .map(|o| o.id)
            .collect();

        out.push_str("```mermaid\nsequenceDiagram\n");
        for node in nodes
            .iter()
            .filter(|o| transform_node_origin(&o.name) == origin)
        {
            out.push_str(&sequence_participant_line(node));
        }

        let edges: Vec<&KirRelationship> = relationships
            .iter()
            .filter(|r| {
                is_feeds_into(&r.kind) && origin_ids.contains(&r.from) && origin_ids.contains(&r.to)
            })
            .collect();
        for edge in &edges {
            let (Some(_), Some(to_node)) = (id_to_node.get(&edge.from), id_to_node.get(&edge.to))
            else {
                continue;
            };
            let node_type = to_node
                .properties
                .get("node_type")
                .and_then(|v| v.as_str())
                .unwrap_or("step");
            out.push_str(&format!(
                "    {}->>{}: {node_type}\n",
                mermaid_node_id(&edge.from),
                mermaid_node_id(&edge.to)
            ));
        }
        out.push_str("```\n");
        if edges.is_empty() {
            // Found by running against a real Pentaho .kjb job — job-orchestration entries are
            // always `Unmapped` by design (never wired together), so a job can have several
            // participants and still zero `FeedsInto` edges; "single step" was wrong whenever
            // more than one node shared an origin with no edges between them.
            let step_word = if origin_ids.len() == 1 {
                "step"
            } else {
                "steps"
            };
            out.push_str(&format!(
                "\n_({} {step_word} — no `FeedsInto` edges compiled for this pipeline)_\n",
                origin_ids.len()
            ));
        }
        out.push('\n');
    }

    render_call_sequences_section(objects, relationships, out)
}

/// The `## Call Sequences` section appended to `SequenceDiagrams.md` (RFC 0042): real
/// `RelationshipKind::Calls` edges, grouped by the caller's containing module (via `Contains`
/// edges module→symbol, the same association `render_api` uses), one small `sequenceDiagram` per
/// module — capped the same way `render_architecture`'s `## Dependency Graph` caps an oversized
/// relationship kind, so one module with hundreds of internal calls can't make the whole page
/// unreadable.
fn render_call_sequences_section(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    mut out: String,
) -> RenderedPage {
    out.push_str(
        "## Call Sequences\n\n_Rendered from real `Calls` edges (function/method call graph, \
         RFC 0041) — grouped by the caller's containing module. A genuine code call sequence, \
         distinct from the data-flow sequences above._\n\n",
    );

    let call_edges: Vec<&KirRelationship> = relationships
        .iter()
        .filter(|r| matches!(r.kind, RelationshipKind::Calls))
        .collect();
    if call_edges.is_empty() {
        out.push_str("_No `Calls` relationships compiled._\n");
        return RenderedPage {
            file_name: "SequenceDiagrams.md".to_string(),
            content: out,
        };
    }

    // Grouped by containing *file*, not `Custom("RustModule")`/`Custom("PythonModule")` — those
    // two kinds are `use`/import targets (`DependsOn` from the file), not containers; the real
    // `Contains` edge into a symbol comes from its defining `File` (see `render_api`'s doc
    // comment for the same distinction).
    let file_ids: HashSet<KirId> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::File)
        .map(|o| o.id)
        .collect();
    let file_name_by_id: HashMap<KirId, &str> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::File)
        .map(|o| (o.id, o.name.as_str()))
        .collect();
    let mut symbol_file: HashMap<KirId, KirId> = HashMap::new();
    for rel in relationships {
        if matches!(rel.kind, RelationshipKind::Contains) && file_ids.contains(&rel.from) {
            symbol_file.insert(rel.to, rel.from);
        }
    }
    let symbol_name_by_id: HashMap<KirId, &str> = objects
        .iter()
        .filter(|o| is_symbol_kind(&o.kind))
        .map(|o| (o.id, o.name.as_str()))
        .collect();

    let mut by_module: BTreeMap<&str, Vec<&KirRelationship>> = BTreeMap::new();
    for edge in &call_edges {
        let file_name = symbol_file
            .get(&edge.from)
            .and_then(|fid| file_name_by_id.get(fid))
            .copied()
            .unwrap_or("(file unknown)");
        by_module.entry(file_name).or_default().push(edge);
    }

    const MAX_CALL_EDGES: usize = 20;
    for (module_name, edges) in by_module {
        out.push_str(&format!("### {module_name}\n\n"));
        if edges.len() > MAX_CALL_EDGES {
            out.push_str(&format!(
                "_{} `Calls` edges compiled for this module — diagram omitted, too large to \
                 render usefully._\n\n",
                edges.len()
            ));
            continue;
        }
        out.push_str("```mermaid\nsequenceDiagram\n");
        let mut seen: HashSet<KirId> = HashSet::new();
        for edge in &edges {
            for id in [edge.from, edge.to] {
                if seen.insert(id) {
                    let label = symbol_name_by_id.get(&id).copied().unwrap_or("unknown");
                    out.push_str(&format!(
                        "    participant {} as \"{}\"\n",
                        mermaid_node_id(&id),
                        mermaid_escape_label(label)
                    ));
                }
            }
        }
        for edge in &edges {
            out.push_str(&format!(
                "    {}->>{}: calls\n",
                mermaid_node_id(&edge.from),
                mermaid_node_id(&edge.to)
            ));
        }
        out.push_str("```\n\n");
    }

    RenderedPage {
        file_name: "SequenceDiagrams.md".to_string(),
        content: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirId, ObjectKind, RelationshipKind, SourceLocation};

    fn sample_table() -> KirObject {
        KirObject::new("customers", ObjectKind::Table).with_property(
            "columns",
            serde_json::json!([{"name": "id", "data_type": "int"}]),
        )
    }

    #[test]
    fn renders_name_kind_and_properties() {
        let table = sample_table();
        let page = render_object_page(&table, &[], &[], &HashMap::new());
        assert_eq!(page.file_name, "table-customers.md");
        assert!(page.content.contains("# customers (Table)"));
        assert!(page.content.contains("`columns`"));
    }

    #[test]
    fn empty_object_renders_honest_placeholders_not_panics() {
        let table = KirObject::new("empty", ObjectKind::Table);
        let page = render_object_page(&table, &[], &[], &HashMap::new());
        assert!(page.content.contains("_No compiled properties._"));
        assert!(
            page.content
                .contains("_No compiled relationships touch this object._")
        );
        assert!(page.content.contains("_No relationships to diagram._"));
        assert!(page.content.contains("_No evidence cited._"));
    }

    /// Phase 2 ("Real Descriptions, Purpose, and Links"): a real `"description"` property (Phase
    /// 1's real doc-comment extraction) is promoted to its own `## Definition` section, not left
    /// in the generic `## Properties` table where it would be shown twice.
    #[test]
    fn a_real_description_property_becomes_the_definition_section_not_a_duplicated_property() {
        let module = KirObject::new(
            "Plausible.Auth.Password",
            ObjectKind::Custom("ElixirModule".into()),
        )
        .with_property(
            "description",
            serde_json::json!("Handles password hashing."),
        );
        let page = render_object_page(&module, &[], &[], &HashMap::new());
        assert!(
            page.content
                .contains("## Definition\n\nHandles password hashing.")
        );
        // Not duplicated into the generic Properties table.
        assert!(!page.content.contains("`description`"));
    }

    #[test]
    fn no_real_description_property_renders_an_honest_not_documented_placeholder() {
        let table = sample_table();
        let page = render_object_page(&table, &[], &[], &HashMap::new());
        assert!(
            page.content
                .contains("## Definition\n\n_Not documented in source._")
        );
    }

    #[test]
    fn the_html_page_also_promotes_description_into_its_own_definition_section() {
        let module = KirObject::new("Foo", ObjectKind::Custom("ElixirModule".into()))
            .with_property("description", serde_json::json!("Does the thing."));
        let model = build_object_page_model(&module, &[], &[], &HashMap::new());
        let page = render_html_object_page(&model);
        assert!(page.content.contains("<h2>Definition</h2>"));
        assert!(page.content.contains("<p>Does the thing.</p>"));
    }

    #[test]
    fn relationship_with_resolved_evidence_cites_the_fragment() {
        let table = sample_table();
        let other = KirId::new();
        let ev = KirEvidence::new(
            SourceLocation::file("schema.sql"),
            "FOREIGN KEY (customer_id) REFERENCES customers(id)",
        );
        let mut rel = KirRelationship::new(RelationshipKind::ForeignKey, table.id, other);
        rel.evidence.push(ev.id);

        let page = render_object_page(
            &table,
            std::slice::from_ref(&rel),
            &[ev.clone()],
            &HashMap::new(),
        );
        // Phase 2 ("Real Descriptions, Purpose, and Links"): real relationships now group by
        // real structural meaning (direction), not raw kind — a real outgoing, non-`Contains`
        // edge like this one is real "Dependent on" data, not its own `ForeignKey` header.
        assert!(page.content.contains("### Dependent on"));
        assert!(page.content.contains(&format!("→ `{other}`")));
        assert!(
            page.content
                .contains("FOREIGN KEY (customer_id) REFERENCES customers(id)")
        );
    }

    #[test]
    fn relationship_citing_unresolved_evidence_says_so_honestly() {
        let table = sample_table();
        let other = KirId::new();
        let mut rel = KirRelationship::new(RelationshipKind::ForeignKey, table.id, other);
        rel.evidence.push(KirId::new()); // not in the resolved evidence slice

        let page = render_object_page(&table, std::slice::from_ref(&rel), &[], &HashMap::new());
        assert!(page.content.contains("evidence unavailable"));
    }

    #[test]
    fn relationships_group_by_real_structural_meaning_without_dropping_any_kind() {
        let table = sample_table();
        let a = KirId::new();
        let b = KirId::new();
        let parent = KirId::new();
        // Two different real outgoing, non-`Contains` kinds — both real "Dependent on" data,
        // grouped together by direction (Phase 2), neither dropped.
        let fk = KirRelationship::new(RelationshipKind::ForeignKey, table.id, a);
        let coupled = KirRelationship::new(RelationshipKind::CoupledWith, table.id, b);
        // A real incoming `Contains` edge — the table's own structural home.
        let contains_parent = KirRelationship::new(RelationshipKind::Contains, parent, table.id);

        let page = render_object_page(
            &table,
            &[fk, coupled, contains_parent],
            &[],
            &HashMap::new(),
        );
        assert!(page.content.contains("### Dependent on"));
        assert!(page.content.contains(&format!("→ `{a}`")));
        assert!(page.content.contains(&format!("→ `{b}`")));
        assert!(page.content.contains("### Based on"));
        assert!(page.content.contains(&format!("← `{parent}`")));
    }

    #[test]
    fn slugify_handles_dots_and_mixed_case() {
        let table = KirObject::new("gold.Dim_Customer", ObjectKind::Table);
        let page = render_object_page(&table, &[], &[], &HashMap::new());
        assert_eq!(page.file_name, "table-gold-dim-customer.md");
    }

    #[test]
    fn incoming_relationship_renders_reverse_arrow() {
        let table = sample_table();
        let other = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, other, table.id);
        let page = render_object_page(&table, &[rel], &[], &HashMap::new());
        assert!(page.content.contains(&format!("← `{other}`")));
    }

    #[test]
    fn relationship_with_resolved_name_shows_name_not_just_id() {
        let table = sample_table();
        let other = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, table.id, other);
        let names = HashMap::from([(other, "orders".to_string())]);

        let page = render_object_page(&table, &[rel], &[], &names);
        assert!(page.content.contains(&format!("→ orders (`{other}`)")));
    }

    #[test]
    fn column_is_not_significant_but_every_other_kind_is() {
        assert!(!is_significant(&ObjectKind::Column));
        for kind in [
            ObjectKind::File,
            ObjectKind::Directory,
            ObjectKind::Table,
            ObjectKind::Pipeline,
            ObjectKind::Dataset,
            ObjectKind::Unknown,
            ObjectKind::Custom("TransformNode".to_string()),
        ] {
            assert!(is_significant(&kind), "{kind:?} should be significant");
        }
    }

    #[test]
    fn non_table_kinds_render_pages_with_kind_prefixed_file_names() {
        let file = KirObject::new("src/main.rs", ObjectKind::File);
        let page = render_object_page(&file, &[], &[], &HashMap::new());
        assert_eq!(page.file_name, "file-src-main-rs.md");
        assert!(page.content.contains("# src/main.rs (File)"));

        let pipeline = KirObject::new("fact_sales", ObjectKind::Custom("TransformNode".into()));
        let page = render_object_page(&pipeline, &[], &[], &HashMap::new());
        assert_eq!(page.file_name, "transformnode-fact-sales.md");
        assert!(page.content.contains("# fact_sales (TransformNode)"));
    }

    #[test]
    fn different_kinds_sharing_a_name_do_not_collide_on_file_name() {
        let table = KirObject::new("orders", ObjectKind::Table);
        let pipeline = KirObject::new("orders", ObjectKind::Pipeline);
        let table_page = render_object_page(&table, &[], &[], &HashMap::new());
        let pipeline_page = render_object_page(&pipeline, &[], &[], &HashMap::new());
        assert_ne!(table_page.file_name, pipeline_page.file_name);
    }

    #[test]
    fn index_page_groups_by_kind_and_links_every_page() {
        let pages = vec![
            (
                ObjectKind::Table,
                "orders".to_string(),
                "table-orders.md".to_string(),
            ),
            (
                ObjectKind::Table,
                "customers".to_string(),
                "table-customers.md".to_string(),
            ),
            (
                ObjectKind::File,
                "main.rs".to_string(),
                "file-main-rs.md".to_string(),
            ),
        ];
        let index = render_index_page(&pages, &[]);
        assert_eq!(index.file_name, "index.md");
        assert!(index.content.contains("## Table (2)"));
        assert!(index.content.contains("## File (1)"));
        assert!(index.content.contains("[orders](table-orders.md)"));
        assert!(index.content.contains("[customers](table-customers.md)"));
        assert!(index.content.contains("[main.rs](file-main-rs.md)"));
        // alphabetical within a kind group: customers before orders
        let customers_pos = index.content.find("customers").unwrap();
        let orders_pos = index.content.find("orders").unwrap();
        assert!(customers_pos < orders_pos);
    }

    #[test]
    fn index_page_on_empty_set_is_honest_not_empty_file() {
        let index = render_index_page(&[], &[]);
        assert!(index.content.contains("No documented objects yet"));
    }

    #[test]
    fn index_page_lists_diagrams_ahead_of_object_groups() {
        let pages = vec![(
            ObjectKind::Table,
            "orders".to_string(),
            "table-orders.md".to_string(),
        )];
        let diagrams = vec![(
            "Entity-Relationship Diagram".to_string(),
            "er-diagram.md".to_string(),
        )];
        let index = render_index_page(&pages, &diagrams);
        assert!(index.content.contains("## Diagrams"));
        assert!(
            index
                .content
                .contains("[Entity-Relationship Diagram](er-diagram.md)")
        );
        let diagrams_pos = index.content.find("## Diagrams").unwrap();
        let table_pos = index.content.find("## Table").unwrap();
        assert!(diagrams_pos < table_pos, "diagrams section comes first");
    }

    #[test]
    fn index_page_with_no_diagrams_omits_the_diagrams_section() {
        let pages = vec![(
            ObjectKind::Table,
            "orders".to_string(),
            "table-orders.md".to_string(),
        )];
        let index = render_index_page(&pages, &[]);
        assert!(!index.content.contains("## Diagrams"));
    }

    #[test]
    fn object_page_embeds_a_diagram_section_with_a_fenced_mermaid_block() {
        let table = sample_table();
        let other = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, table.id, other);
        let names = HashMap::from([(other, "orders".to_string())]);

        let page = render_object_page(&table, &[rel], &[], &names);
        assert!(page.content.contains("## Diagram"));
        assert!(page.content.contains("```mermaid"));
        assert!(page.content.contains("graph TD"));
        assert!(page.content.contains("\"customers\""));
        assert!(page.content.contains("\"orders\""));
    }

    #[test]
    fn mermaid_graph_labels_edges_with_relationship_kind_and_direction() {
        let table = sample_table();
        let other = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, table.id, other);
        let names = HashMap::from([(other, "orders".to_string())]);

        let diagram = render_mermaid_graph(&table, &[rel], &names);
        assert!(diagram.starts_with("```mermaid\ngraph TD\n"));
        assert!(diagram.trim_end().ends_with("```"));
        assert!(diagram.contains("-->|ForeignKey|"));
    }

    #[test]
    fn mermaid_graph_dashes_coupled_with_edges_to_signal_a_derived_relationship() {
        let table = sample_table();
        let other = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::CoupledWith, table.id, other);
        let diagram = render_mermaid_graph(&table, &[rel], &HashMap::new());
        assert!(diagram.contains("-.->|CoupledWith|"));
    }

    #[test]
    fn mermaid_graph_unresolved_neighbor_falls_back_to_id_not_dropped() {
        let table = sample_table();
        let other = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, table.id, other);
        let diagram = render_mermaid_graph(&table, &[rel], &HashMap::new());
        assert!(diagram.contains(&other.to_string()));
    }

    #[test]
    fn mermaid_graph_escapes_quotes_in_labels() {
        let table = KirObject::new("weird \"quoted\" name", ObjectKind::Table);
        let diagram = render_mermaid_graph(&table, &[], &HashMap::new());
        assert!(!diagram.contains("\"weird \"quoted\""));
        assert!(diagram.contains("weird 'quoted' name"));
    }

    #[test]
    fn er_diagram_renders_foreign_key_edges_between_given_tables() {
        let customers = KirObject::new("customers", ObjectKind::Table);
        let orders = KirObject::new("orders", ObjectKind::Table);
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, customers.id);

        let diagram = render_er_diagram(&[customers, orders], &[rel]);
        assert!(diagram.starts_with("```mermaid\nerDiagram\n"));
        assert!(diagram.contains("\"orders\" }o--|| \"customers\" : references"));
    }

    #[test]
    fn er_diagram_excludes_foreign_keys_to_objects_outside_the_table_set() {
        let orders = KirObject::new("orders", ObjectKind::Table);
        let outside = KirId::new(); // not in the `tables` slice passed in
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, outside);

        let diagram = render_er_diagram(&[orders], &[rel]);
        assert!(diagram.contains("no ForeignKey relationships"));
    }

    #[test]
    fn er_diagram_ignores_non_foreign_key_relationships() {
        let customers = KirObject::new("customers", ObjectKind::Table);
        let orders = KirObject::new("orders", ObjectKind::Table);
        let rel = KirRelationship::new(RelationshipKind::CoupledWith, orders.id, customers.id);

        let diagram = render_er_diagram(&[customers, orders], &[rel]);
        assert!(diagram.contains("no ForeignKey relationships"));
    }

    #[test]
    fn er_diagram_quotes_entity_names_containing_spaces() {
        let order_details = KirObject::new("Order Details", ObjectKind::Table);
        let orders = KirObject::new("orders", ObjectKind::Table);
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, order_details.id, orders.id);

        let diagram = render_er_diagram(&[order_details, orders], &[rel]);
        assert!(diagram.contains("\"Order Details\""));
    }

    // ── Phase 4 — page model + HTML renderer ────────────────────────────────

    #[test]
    fn model_and_markdown_page_agree_with_the_direct_render_object_page_wrapper() {
        let table = sample_table();
        let other = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, table.id, other);
        let names = HashMap::from([(other, "orders".to_string())]);

        let direct = render_object_page(&table, &[rel.clone()], &[], &names);
        let model = build_object_page_model(&table, &[rel], &[], &names);
        let via_model = render_markdown_object_page(&model);
        assert_eq!(direct, via_model);
    }

    #[test]
    fn html_page_has_correct_file_extension_and_is_a_full_document() {
        let table = sample_table();
        let model = build_object_page_model(&table, &[], &[], &HashMap::new());
        let page = render_html_object_page(&model);
        assert_eq!(page.file_name, "table-customers.html");
        assert!(page.content.starts_with("<!doctype html>"));
        assert!(page.content.contains("<title>customers — Table</title>"));
        assert!(page.content.contains("<h1>customers"));
    }

    #[test]
    fn html_page_escapes_dangerous_characters_in_object_derived_text() {
        let table = KirObject::new("<script>alert(1)</script>", ObjectKind::Table);
        let model = build_object_page_model(&table, &[], &[], &HashMap::new());
        let page = render_html_object_page(&model);
        assert!(!page.content.contains("<script>alert"));
        assert!(page.content.contains("&lt;script&gt;"));
    }

    #[test]
    fn html_page_embeds_mermaid_source_without_markdown_fence() {
        let table = sample_table();
        let other = KirId::new();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, table.id, other);
        let model = build_object_page_model(&table, &[rel], &[], &HashMap::new());
        let page = render_html_object_page(&model);
        assert!(page.content.contains("<pre class=\"mermaid-source\">"));
        assert!(page.content.contains("graph TD"));
        assert!(!page.content.contains("```mermaid"));
    }

    #[test]
    fn html_page_on_empty_object_renders_honest_placeholders() {
        let table = KirObject::new("empty", ObjectKind::Table);
        let model = build_object_page_model(&table, &[], &[], &HashMap::new());
        let page = render_html_object_page(&model);
        assert!(page.content.contains("No compiled properties."));
        assert!(
            page.content
                .contains("No compiled relationships touch this object.")
        );
        assert!(page.content.contains("No relationships to diagram."));
        assert!(page.content.contains("No evidence cited."));
    }

    #[test]
    fn html_index_lists_diagrams_and_groups_pages_by_kind() {
        let pages = vec![(
            ObjectKind::Table,
            "orders".to_string(),
            "table-orders.html".to_string(),
        )];
        let diagrams = vec![(
            "Entity-Relationship Diagram".to_string(),
            "er-diagram.html".to_string(),
        )];
        let index = render_html_index_page(&pages, &diagrams);
        assert_eq!(index.file_name, "index.html");
        assert!(index.content.contains("<h2>Diagrams</h2>"));
        assert!(index.content.contains("href=\"er-diagram.html\""));
        assert!(index.content.contains("<h2>Table (1)</h2>"));
        assert!(
            index
                .content
                .contains("href=\"table-orders.html\">orders</a>")
        );
    }

    #[test]
    fn html_index_on_empty_set_is_honest_not_blank() {
        let index = render_html_index_page(&[], &[]);
        assert!(index.content.contains("No documented objects yet"));
    }

    #[test]
    fn strip_mermaid_fence_removes_fence_but_keeps_body() {
        let fenced = "```mermaid\ngraph TD\n    a[\"x\"]\n```\n";
        let body = strip_mermaid_fence(fenced);
        assert_eq!(body, "graph TD\n    a[\"x\"]\n");
        assert!(!body.contains("```"));
    }

    #[test]
    fn html_er_diagram_page_has_correct_file_name_and_embeds_source() {
        let customers = KirObject::new("customers", ObjectKind::Table);
        let orders = KirObject::new("orders", ObjectKind::Table);
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, customers.id);

        let page = render_html_er_diagram_page(&[customers, orders], &[rel]);
        assert_eq!(page.file_name, "er-diagram.html");
        assert!(page.content.contains("erDiagram"));
        assert!(!page.content.contains("```"));
        assert!(page.content.contains("orders&quot; }o--|| &quot;customers"));
    }

    // ── Phase 5 — prose section ──────────────────────────────────────────────

    #[test]
    fn build_object_page_model_initializes_prose_to_none() {
        let table = sample_table();
        let model = build_object_page_model(&table, &[], &[], &HashMap::new());
        assert!(model.prose.is_none());
    }

    #[test]
    fn markdown_page_embeds_prose_and_its_citations_ahead_of_properties() {
        let table = sample_table();
        let mut model = build_object_page_model(&table, &[], &[], &HashMap::new());
        let ev_id = KirId::new();
        model.prose = Some(ProseSection {
            text: "Customers holds one row per registered account.".to_string(),
            cited_evidence: vec![ev_id],
        });

        let page = render_markdown_object_page(&model);
        assert!(page.content.contains("## Overview"));
        assert!(
            page.content
                .contains("Customers holds one row per registered account.")
        );
        assert!(
            page.content
                .contains(&format!("_Cited evidence: `{ev_id}`_"))
        );
        let overview_pos = page.content.find("## Overview").unwrap();
        let properties_pos = page.content.find("## Properties").unwrap();
        assert!(
            overview_pos < properties_pos,
            "Overview comes before Properties"
        );
    }

    #[test]
    fn markdown_page_without_prose_has_no_overview_section() {
        let table = sample_table();
        let model = build_object_page_model(&table, &[], &[], &HashMap::new());
        let page = render_markdown_object_page(&model);
        assert!(!page.content.contains("## Overview"));
    }

    #[test]
    fn html_page_embeds_prose_and_escapes_it() {
        let table = sample_table();
        let mut model = build_object_page_model(&table, &[], &[], &HashMap::new());
        model.prose = Some(ProseSection {
            text: "Has a <b>bold</b> claim.".to_string(),
            cited_evidence: vec![],
        });

        let page = render_html_object_page(&model);
        assert!(page.content.contains("<h2>Overview</h2>"));
        assert!(
            page.content
                .contains("Has a &lt;b&gt;bold&lt;/b&gt; claim.")
        );
        assert!(!page.content.contains("<b>bold</b>"));
    }

    #[test]
    fn html_page_without_prose_has_no_overview_section() {
        let table = sample_table();
        let model = build_object_page_model(&table, &[], &[], &HashMap::new());
        let page = render_html_object_page(&model);
        assert!(!page.content.contains("<h2>Overview</h2>"));
    }

    // ── RFC 0037 — curated documentation set ────────────────────────────────

    #[test]
    fn readme_lists_component_counts_and_doc_links() {
        let objects = vec![
            KirObject::new("orders", ObjectKind::Table),
            KirObject::new("customers", ObjectKind::Table),
            KirObject::new("main.rs", ObjectKind::File),
        ];
        let page = render_readme(&objects);
        assert_eq!(page.file_name, "README.md");
        assert!(page.content.contains("**Table**: 2"));
        assert!(page.content.contains("**File**: 1"));
        assert!(page.content.contains("[Architecture](Architecture.md)"));
        assert!(page.content.contains("[API](API.md)"));
        assert!(
            page.content
                .contains("[Sequence Diagrams](SequenceDiagrams.md)")
        );
    }

    #[test]
    fn readme_on_empty_ledger_is_honest_not_a_fabricated_summary() {
        let page = render_readme(&[]);
        assert!(page.content.contains("No compiled objects yet"));
        assert!(page.content.contains("No contributor data compiled."));
    }

    #[test]
    fn readme_ranks_contributors_by_commit_count_descending() {
        let alice = KirObject::new("alice", ObjectKind::Person)
            .with_property("commit_count", serde_json::json!(3));
        let bob = KirObject::new("bob", ObjectKind::Person)
            .with_property("commit_count", serde_json::json!(9));
        let page = render_readme(&[alice, bob]);
        let bob_pos = page.content.find("bob (9 commits)").unwrap();
        let alice_pos = page.content.find("alice (3 commits)").unwrap();
        assert!(bob_pos < alice_pos, "higher commit_count ranks first");
    }

    #[test]
    fn architecture_lists_technologies_and_their_dependent_files() {
        let file = KirObject::new("db.py", ObjectKind::File);
        let tech = KirObject::new("PostgreSQL", ObjectKind::Custom("Technology".to_string()));
        let rel = KirRelationship::new(RelationshipKind::DependsOn, file.id, tech.id);

        let page = render_architecture(&[file, tech], &[rel], &[]);
        assert_eq!(page.file_name, "Architecture.md");
        assert!(page.content.contains("## Technology Inventory"));
        assert!(page.content.contains("PostgreSQL"));
        assert!(page.content.contains("— used by: db.py"));
    }

    #[test]
    fn architecture_technology_inventory_deduplicates_repeated_dependent_relationships() {
        // Real bug, found live: `KirRelationship::new` mints a fresh random id every time, so
        // ledger-level content-signature dedup never recognizes a logically-identical DependsOn
        // edge re-derived by a later recover/commit as the same one — real duplicate edges
        // accumulate across repeated commits. This view must not surface that as quadruplicated
        // "used by" text.
        let file = KirObject::new("db.py", ObjectKind::File);
        let tech = KirObject::new("PostgreSQL", ObjectKind::Custom("Technology".to_string()));
        let rel_a = KirRelationship::new(RelationshipKind::DependsOn, file.id, tech.id);
        let rel_b = KirRelationship::new(RelationshipKind::DependsOn, file.id, tech.id);
        assert_ne!(
            rel_a.id, rel_b.id,
            "reproduces the real non-deterministic id shape"
        );

        let page = render_architecture(&[file, tech], &[rel_a, rel_b], &[]);
        assert!(page.content.contains("— used by: db.py\n"));
        assert!(!page.content.contains("db.py, db.py"));
    }

    #[test]
    fn architecture_on_no_technologies_is_honest_not_a_fabricated_list() {
        let page = render_architecture(&[], &[], &[]);
        assert!(
            page.content
                .contains("_No external technology dependencies compiled._")
        );
        assert!(
            page.content
                .contains("No technology dependencies compiled.")
        );
        assert!(
            page.content
                .contains("No crate directory matched a compiled subsystem rollup.")
        );
        assert!(
            page.content
                .contains("No table foreign-key relationships compiled.")
        );
        assert!(
            page.content
                .contains("No structural relationships compiled.")
        );
        assert!(
            page.content
                .contains("No crate/workspace manifests compiled.")
        );
        assert!(
            page.content
                .contains("No CI/CD pipeline definitions compiled.")
        );
        assert!(page.content.contains("No subsystem rollups compiled."));
        assert!(
            page.content
                .contains("No open architecture questions compiled.")
        );
    }

    #[test]
    fn architecture_renders_subsystem_rollups_and_links_components_to_them() {
        let rollup = KirObject::new("ekos/crates/kir", ObjectKind::Custom("Rollup".to_string()))
            .with_property("member_count", serde_json::json!(2))
            .with_property("group_key", serde_json::json!("dir:ekos/crates/kir"));

        let page = render_architecture(std::slice::from_ref(&rollup), &[], &[]);
        assert!(page.content.contains("## Subsystems"));
        assert!(page.content.contains("2 member file(s)"));
        assert!(
            page.content
                .contains("**Rollup**: 1 — see below, `## Subsystems`")
        );
    }

    #[test]
    fn architecture_renders_crate_topology_and_links_components_to_api() {
        let kir = KirObject::new("ekos-kir", ObjectKind::Custom("Crate".to_string()));
        let cli = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        let dep = KirRelationship::new(RelationshipKind::DependsOn, cli.id, kir.id);
        let symbol = KirObject::new("run", ObjectKind::Custom("RustSymbol".to_string()));

        let page = render_architecture(&[kir, cli, symbol], &[dep], &[]);
        assert!(page.content.contains("## Crate & Workspace Topology"));
        assert!(page.content.contains("ekos-cli"));
        assert!(page.content.contains("ekos-kir"));
        assert!(
            page.content
                .contains("**RustSymbol**: 1 — see [API.md](API.md)")
        );
        assert!(page.content.contains("C4 mapping (RFC 0065 §23)"));
        assert!(page.content.contains("C4 **Container**"));
    }

    #[test]
    fn architecture_summary_reports_real_counts_and_top_technologies() {
        let file = KirObject::new("db.py", ObjectKind::File);
        let popular = KirObject::new("serde", ObjectKind::Custom("Technology".to_string()));
        let niche = KirObject::new("obscure-lib", ObjectKind::Custom("Technology".to_string()));
        let rel_a = KirRelationship::new(RelationshipKind::DependsOn, file.id, popular.id);
        let rel_b = KirRelationship::new(RelationshipKind::DependsOn, file.id, niche.id);

        let page = render_architecture(&[file, popular, niche], &[rel_a, rel_b], &[]);
        assert!(page.content.contains("## Architecture Summary"));
        assert!(page.content.contains("RFC 0068 §14"));
        assert!(page.content.contains("**Primary technologies:**"));
        assert!(page.content.contains("serde (1 dependent(s))"));
        assert!(page.content.contains("**Open questions:** 0"));
        assert!(page.content.contains("**Purpose:** _not yet computed"));
        assert!(
            page.content
                .contains("**Architecture confidence:** _not yet computed")
        );
    }

    #[test]
    fn architecture_summary_deduplicates_repeated_dependent_relationships() {
        let file = KirObject::new("db.py", ObjectKind::File);
        let tech = KirObject::new("serde", ObjectKind::Custom("Technology".to_string()));
        let rel_a = KirRelationship::new(RelationshipKind::DependsOn, file.id, tech.id);
        let rel_b = KirRelationship::new(RelationshipKind::DependsOn, file.id, tech.id);

        let page = render_architecture(&[file, tech], &[rel_a, rel_b], &[]);
        assert!(page.content.contains("serde (1 dependent(s))"));
        assert!(!page.content.contains("serde (2 dependent(s))"));
    }

    #[test]
    fn architecture_runtime_view_links_to_sequence_diagrams_when_calls_exist() {
        let a = KirObject::new("foo", ObjectKind::Custom("RustSymbol".to_string()));
        let b = KirObject::new("bar", ObjectKind::Custom("RustSymbol".to_string()));
        let call = KirRelationship::new(RelationshipKind::Calls, a.id, b.id);

        let page = render_architecture(&[a, b], &[call], &[]);
        assert!(page.content.contains("## Runtime View"));
        assert!(page.content.contains("RFC 0068 §20"));
        assert!(
            page.content
                .contains("[SequenceDiagrams.md](SequenceDiagrams.md)")
        );
    }

    #[test]
    fn architecture_runtime_view_is_honest_when_no_call_or_flow_edges_exist() {
        let page = render_architecture(&[], &[], &[]);
        assert!(
            page.content
                .contains("_No call or data-flow sequences compiled._")
        );
        assert!(!page.content.contains("SequenceDiagrams.md]"));
    }

    #[test]
    fn architecture_components_links_technology_to_its_renamed_inventory_section() {
        // Regression: the Technology Inventory section was renamed from `## Technologies` to
        // `## Technology Inventory` (RFC 0070), but `components_cross_reference`'s link text was
        // never updated — a real stale cross-reference found while investigating RFC 0068
        // Increment 6, fixed alongside it.
        let tech = KirObject::new("clap", ObjectKind::Custom("Technology".to_string()));
        let page = render_architecture(&[tech], &[], &[]);
        assert!(
            page.content
                .contains("see below, `## Technology Inventory`")
        );
        assert!(!page.content.contains("see below, `## Technologies`"));
    }

    #[test]
    fn data_architecture_lists_real_data_stores_with_foreign_key_counts() {
        let customers = KirObject::new("customers", ObjectKind::Table);
        let orders = KirObject::new("orders", ObjectKind::Table);
        let fk = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, customers.id);

        let section = render_data_architecture(&[customers, orders], &[fk]);
        assert!(section.contains("### Data Stores"));
        assert!(section.contains("2 compiled data store(s)"));
        assert!(section.contains("**customers** — 1 real foreign-key edge(s)"));
        assert!(section.contains("**orders** — 1 real foreign-key edge(s)"));
    }

    #[test]
    fn data_architecture_links_sequence_diagrams_when_transformations_exist() {
        let source = KirObject::new(
            "pipeline.ktr:0",
            ObjectKind::Custom("TransformNode".to_string()),
        );
        let sink = KirObject::new(
            "pipeline.ktr:1",
            ObjectKind::Custom("TransformNode".to_string()),
        );
        let feeds = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            source.id,
            sink.id,
        );

        let section = render_data_architecture(&[source, sink], &[feeds]);
        assert!(section.contains("### Transformations & Lineage"));
        assert!(section.contains("[SequenceDiagrams.md](SequenceDiagrams.md)"));
    }

    #[test]
    fn data_architecture_is_honest_about_every_uncomputed_dimension_on_an_empty_ledger() {
        let section = render_data_architecture(&[], &[]);
        assert!(section.contains("_No compiled data stores (Tables/Datasets)._"));
        assert!(section.contains("_No transformations compiled._"));
        assert!(section.contains("### Data Domains"));
        assert!(section.contains("### Ownership"));
        assert!(section.contains("### Lifecycle"));
        assert!(section.contains("### Data Quality"));
        assert!(!section.contains("SequenceDiagrams.md]"));
    }

    #[test]
    fn data_architecture_shows_real_read_and_write_counts_per_data_store() {
        let read_table = KirObject::new("customers", ObjectKind::Table);
        let write_table = KirObject::new("customer_orders", ObjectKind::Table);
        let untouched_table = KirObject::new("audit_log", ObjectKind::Table);
        let source = KirObject::new("etl.sql:0", ObjectKind::Custom("TransformNode".to_string()));
        let sink = KirObject::new("etl.sql:1", ObjectKind::Custom("TransformNode".to_string()));
        let reads = KirRelationship::new(
            RelationshipKind::Custom("ReadsFrom".to_string()),
            source.id,
            read_table.id,
        );
        let writes = KirRelationship::new(
            RelationshipKind::Custom("WritesTo".to_string()),
            sink.id,
            write_table.id,
        );

        let section = render_data_architecture(
            &[read_table, write_table, untouched_table],
            &[reads, writes],
        );
        assert!(section.contains(
            "**customers** — 0 real foreign-key edge(s), read by 1 transformation(s), written by 0 transformation(s)"
        ));
        assert!(section.contains(
            "**customer_orders** — 0 real foreign-key edge(s), read by 0 transformation(s), written by 1 transformation(s)"
        ));
        assert!(section.contains(
            "**audit_log** — 0 real foreign-key edge(s), read by 0 transformation(s), written by 0 transformation(s)"
        ));
    }

    #[test]
    fn data_architecture_lineage_note_names_rfc_0075_when_links_exist() {
        let table = KirObject::new("customers", ObjectKind::Table);
        let source = KirObject::new("etl.sql:0", ObjectKind::Custom("TransformNode".to_string()));
        let reads = KirRelationship::new(
            RelationshipKind::Custom("ReadsFrom".to_string()),
            source.id,
            table.id,
        );
        let section = render_data_architecture(&[table, source], &[reads]);
        assert!(section.contains("cross-referenced to the Data Stores above (RFC 0075)"));
    }

    #[test]
    fn data_architecture_lineage_note_is_honest_when_flows_exist_but_nothing_linked() {
        let a = KirObject::new(
            "pipeline.ktr:0",
            ObjectKind::Custom("TransformNode".to_string()),
        );
        let b = KirObject::new(
            "pipeline.ktr:1",
            ObjectKind::Custom("TransformNode".to_string()),
        );
        let feeds = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            a.id,
            b.id,
        );
        let section = render_data_architecture(&[a, b], &[feeds]);
        assert!(section.contains("None of this workspace's `TransformNode` source/sink names"));
    }

    #[test]
    fn data_domains_groups_by_schema_qualifier_and_reports_unqualified_count() {
        let orders = KirObject::new("sales.orders", ObjectKind::Table);
        let customers = KirObject::new("sales.customers", ObjectKind::Table);
        let bare = KirObject::new("audit_log", ObjectKind::Table);
        let section = data_domains_section(&[&orders, &customers, &bare]);
        assert!(section.contains("**sales** — customers, orders"));
        assert!(section.contains("1 compiled table name(s) have no schema qualifier"));
    }

    #[test]
    fn data_domains_is_honest_when_no_table_is_schema_qualified() {
        let bare = KirObject::new("customers", ObjectKind::Table);
        let section = data_domains_section(&[&bare]);
        assert!(section.contains("_not yet computed"));
        assert!(section.contains("none of the 1 compiled table name(s)"));
    }

    #[test]
    fn data_domains_on_no_stores_is_honest_not_a_fabricated_grouping() {
        let section = data_domains_section(&[]);
        assert!(section.contains("_not yet computed — no compiled data stores"));
    }

    #[test]
    fn architecture_includes_data_architecture_section_with_rfc_reference() {
        let table = KirObject::new("customers", ObjectKind::Table);
        let page = render_architecture(&[table], &[], &[]);
        assert!(page.content.contains("## Data Architecture"));
        assert!(page.content.contains("RFC 0068 §22"));
        assert!(page.content.contains("**customers**"));
    }

    #[test]
    fn architecture_renders_system_context_from_real_crate_technology_dependency() {
        let krate = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        let tech = KirObject::new("clap", ObjectKind::Custom("Technology".to_string()));
        let dep = KirRelationship::new(RelationshipKind::DependsOn, krate.id, tech.id);

        let page = render_architecture(&[krate, tech], &[dep], &[]);
        assert!(page.content.contains("## System Context"));
        assert!(page.content.contains("RFC 0068 §15"));
        assert!(page.content.contains("[\"System\"]"));
        assert!(page.content.contains("[\"clap\"]"));
        assert!(page.content.contains("-->|DependsOn|"));
    }

    #[test]
    fn architecture_system_context_excludes_technology_with_no_real_dependency_edge() {
        // A Technology object that exists but that no compiled Crate actually depends on (e.g.
        // detected by a different analyzer, or stale) must not appear in the System Context —
        // only real, currently-compiled dependency edges count.
        let krate = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        let unused_tech =
            KirObject::new("unused-lib", ObjectKind::Custom("Technology".to_string()));

        let page = render_architecture(&[krate, unused_tech], &[], &[]);
        assert!(page.content.contains("## System Context"));
        assert!(
            page.content
                .contains("_No external technology dependencies compiled._")
        );
        assert!(!page.content.contains("unused-lib\"]"));
    }

    #[test]
    fn architecture_links_system_context_svg_when_real_dependency_data_exists() {
        let krate = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        let tech = KirObject::new("clap", ObjectKind::Custom("Technology".to_string()));
        let dep = KirRelationship::new(RelationshipKind::DependsOn, krate.id, tech.id);

        let page = render_architecture(&[krate, tech], &[dep], &[]);
        assert!(
            page.content
                .contains("[System Context diagram (SVG)](system-context.svg)")
        );
    }

    #[test]
    fn architecture_does_not_link_system_context_svg_when_no_real_dependency_data_exists() {
        let krate = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        let page = render_architecture(&[krate], &[], &[]);
        assert!(!page.content.contains("system-context.svg"));
    }

    #[test]
    fn render_system_context_svg_returns_none_without_real_dependency_data() {
        let krate = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        assert!(render_system_context_svg(&[krate], &[]).is_none());
    }

    #[test]
    fn render_system_context_svg_renders_a_real_svg_document_with_technology_nodes() {
        let krate = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        let tech = KirObject::new("clap", ObjectKind::Custom("Technology".to_string()));
        let dep = KirRelationship::new(RelationshipKind::DependsOn, krate.id, tech.id);

        let page = render_system_context_svg(&[krate, tech], &[dep]).unwrap();
        assert_eq!(page.file_name, "system-context.svg");
        assert!(page.content.starts_with("<svg "));
        assert!(page.content.contains("</svg>"));
        assert!(page.content.contains(">System<"));
        assert!(page.content.contains(">clap<"));
        assert!(page.content.contains("marker-end=\"url(#arrow)\""));
    }

    #[test]
    fn render_crate_topology_svg_renders_real_internal_crate_dependencies() {
        let core = KirObject::new("ekos-core", ObjectKind::Custom("Crate".to_string()));
        let cli = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        let dep = KirRelationship::new(RelationshipKind::DependsOn, cli.id, core.id);

        let page = render_crate_topology_svg(&[core, cli], &[dep]).unwrap();
        assert_eq!(page.file_name, "crate-topology.svg");
        assert!(page.content.starts_with("<svg "));
        assert!(page.content.contains(">ekos-core<"));
        assert!(page.content.contains(">ekos-cli<"));
    }

    #[test]
    fn render_crate_topology_svg_is_none_with_no_internal_dependencies() {
        let krate = KirObject::new("ekos-cli", ObjectKind::Custom("Crate".to_string()));
        assert!(render_crate_topology_svg(&[krate], &[]).is_none());
    }

    #[test]
    fn render_graph_svg_on_empty_nodes_is_an_empty_string() {
        assert_eq!(render_graph_svg(&[], &[]), "");
    }

    #[test]
    fn render_graph_svg_escapes_labels_and_lays_out_children_below_their_root() {
        let nodes = vec![
            ("root".to_string(), "Ro<ot> & \"Sons\"".to_string()),
            ("a".to_string(), "A".to_string()),
            ("b".to_string(), "B".to_string()),
        ];
        let edges = vec![
            ("root".to_string(), "a".to_string()),
            ("root".to_string(), "b".to_string()),
        ];

        let svg = render_graph_svg(&nodes, &edges);
        assert!(svg.contains("Ro&lt;ot&gt; &amp; &quot;Sons&quot;"));
        assert_eq!(svg.matches("<rect ").count(), 3);
        assert_eq!(svg.matches("<line ").count(), 2);
    }

    #[test]
    fn render_graph_svg_places_every_node_exactly_once_even_with_a_cycle() {
        // A cycle means neither node ever reaches indegree 0 through the main loop — must still
        // appear via the final "remaining nodes" fallback layer, not be silently dropped.
        let nodes = vec![
            ("a".to_string(), "A".to_string()),
            ("b".to_string(), "B".to_string()),
        ];
        let edges = vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "a".to_string()),
        ];
        let svg = render_graph_svg(&nodes, &edges);
        assert_eq!(svg.matches("<rect ").count(), 2);
        assert_eq!(svg.matches("<line ").count(), 2);
    }

    /// RFC 0083 Phase 4: a real System Context-shaped layer (one root, many same-layer children —
    /// e.g. 46 real technologies) must wrap into multiple rows instead of one unreadably wide row.
    #[test]
    fn render_graph_svg_wraps_a_layer_wider_than_max_nodes_per_row() {
        let mut nodes = vec![("root".to_string(), "Root".to_string())];
        let mut edges = Vec::new();
        for i in 0..12 {
            let id = format!("n{i:02}");
            nodes.push((id.clone(), format!("Node {i}")));
            edges.push(("root".to_string(), id));
        }

        let svg = render_graph_svg(&nodes, &edges);
        assert_eq!(svg.matches("<rect ").count(), 13);

        // 12 same-layer children wrapped at MAX_NODES_PER_ROW=8 → two visual rows (8 + 4), plus
        // the root's own row → 3 distinct y values, not the unwrapped 2 (root row + one 12-wide
        // child row).
        let y_values: HashSet<&str> = svg
            .lines()
            .filter(|l| l.starts_with("  <rect "))
            .filter_map(|l| l.split("y=\"").nth(1))
            .filter_map(|rest| rest.split('"').next())
            .collect();
        assert_eq!(
            y_values.len(),
            3,
            "expected 3 distinct row y-positions: {svg}"
        );

        // Width must reflect the widest *row* (8 nodes), not the widest *layer* (12 nodes) —
        // the whole point of wrapping.
        let unwrapped_width = SVG_MARGIN * 2.0 + 12.0 * SVG_NODE_WIDTH + 11.0 * SVG_NODE_GAP;
        let wrapped_width = SVG_MARGIN * 2.0 + 8.0 * SVG_NODE_WIDTH + 7.0 * SVG_NODE_GAP;
        assert!(svg.contains(&format!("width=\"{wrapped_width:.1}\"")));
        assert!(!svg.contains(&format!("width=\"{unwrapped_width:.1}\"")));
    }

    #[test]
    fn architecture_component_view_links_a_crate_to_its_matching_rollup() {
        let krate = KirObject::new("ekos-kir", ObjectKind::Custom("Crate".to_string()))
            .with_property("path", serde_json::json!("ekos/crates/kir"));
        let rollup = KirObject::new("ekos/crates/kir", ObjectKind::Custom("Rollup".to_string()))
            .with_property("member_count", serde_json::json!(9));

        let page = render_architecture(&[krate, rollup], &[], &[]);
        assert!(page.content.contains("## Component View"));
        assert!(page.content.contains("**ekos-kir**"));
        assert!(page.content.contains("9 member file(s)"));
    }

    #[test]
    fn architecture_component_view_honestly_reports_a_crate_with_no_matching_rollup() {
        // RFC 0044's own >=2-member threshold means many real crates legitimately have no
        // rollup — that's not an error, but (RFC 0083 Phase 4) it must still be named and
        // counted, not silently vanish with zero trace.
        let krate = KirObject::new("ekos-tiny", ObjectKind::Custom("Crate".to_string()))
            .with_property("path", serde_json::json!("ekos/crates/tiny"));

        let page = render_architecture(&[krate], &[], &[]);
        assert!(page.content.contains("## Component View"));
        assert!(page.content.contains("ekos-tiny"));
        assert!(
            page.content
                .contains("1 crate(s) have no matching subsystem rollup")
        );
        assert!(!page.content.contains("member file(s)"));
    }

    #[test]
    fn architecture_component_view_reports_no_crates_at_all_when_none_are_compiled() {
        let page = render_architecture(&[], &[], &[]);
        assert!(
            page.content
                .contains("_No crate directory matched a compiled subsystem rollup._")
        );
    }

    #[test]
    fn architecture_renders_open_questions_from_architecture_gaps() {
        let gap = KirObject::new(
            "unresolved dependency 'foo' for ekos-orphan",
            ObjectKind::Custom("ArchitectureGap".to_string()),
        )
        .with_property(
            "question",
            serde_json::json!("What does 'foo' resolve to for ekos-orphan?"),
        )
        .with_property("affected_crate", serde_json::json!("ekos-orphan"));

        let page = render_architecture(&[gap], &[], &[]);
        assert!(page.content.contains("## Open Questions"));
        assert!(
            page.content
                .contains("What does 'foo' resolve to for ekos-orphan? (affects `ekos-orphan`)")
        );
    }

    #[test]
    fn architecture_renders_cicd_pipelines() {
        let pipeline = KirObject::new("CI", ObjectKind::Pipeline)
            .with_property("triggers", serde_json::json!(["push"]))
            .with_property(
                "jobs",
                serde_json::json!([{"name": "build", "steps": ["Checkout", "Test"]}]),
            );

        let page = render_architecture(&[pipeline], &[], &[]);
        assert!(page.content.contains("## CI/CD Pipelines"));
        assert!(page.content.contains("### CI"));
        assert!(page.content.contains("Triggers: `push`"));
        assert!(page.content.contains("**build**"));
        assert!(page.content.contains("Checkout"));
    }

    #[test]
    fn architecture_dependency_graph_excludes_feeds_into_edges() {
        let a = KirObject::new("a", ObjectKind::Custom("TransformNode".to_string()));
        let b = KirObject::new("b", ObjectKind::Custom("TransformNode".to_string()));
        let c = KirObject::new("c", ObjectKind::File);
        let d = KirObject::new("d", ObjectKind::File);
        let feeds_into = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            a.id,
            b.id,
        );
        let coupled = KirRelationship::new(RelationshipKind::CoupledWith, c.id, d.id);

        let page = render_architecture(&[a, b, c, d], &[feeds_into, coupled], &[]);
        assert!(page.content.contains("### CoupledWith"));
        assert!(
            !page.content.contains("### FeedsInto"),
            "pipeline-internal FeedsInto edges belong in SequenceDiagrams.md, not Architecture.md"
        );
    }

    /// Regression test for a real bug found by running this against a real Pentaho+PDF workspace:
    /// `Contains` edges from PDF pages/sections alone produced 74 edges in one relationship kind,
    /// still unreadable even after excluding `FeedsInto` — the size cap must apply per kind, not
    /// only to the one kind known in advance to be large.
    #[test]
    fn architecture_omits_a_diagram_for_a_relationship_kind_with_too_many_edges() {
        let objects: Vec<KirObject> = (0..50)
            .map(|i| KirObject::new(format!("section-{i}"), ObjectKind::Custom("Section".into())))
            .collect();
        let doc = KirObject::new("doc.pdf", ObjectKind::File);
        let relationships: Vec<KirRelationship> = objects
            .iter()
            .map(|o| KirRelationship::new(RelationshipKind::Contains, doc.id, o.id))
            .collect();

        let mut all_objects = objects;
        all_objects.push(doc);
        let page = render_architecture(&all_objects, &relationships, &[]);

        assert!(page.content.contains("### Contains"));
        assert!(
            page.content
                .contains("50 `Contains` relationships compiled")
        );
        assert!(
            page.content
                .contains("diagram omitted, too large to render usefully")
        );
        assert!(!page.content.contains("```mermaid\ngraph TD\n    n"));
        assert!(
            page.content.contains("- doc.pdf → section-0"),
            "File/Section endpoints get no curated detail page, so the overflow sample must \
             render plain text, not a dangling markdown link — got: {}",
            page.content
        );
        assert!(
            !page.content.contains("[doc.pdf]("),
            "must never link an endpoint kind curated never writes a page for"
        );
    }

    #[test]
    fn architecture_embeds_er_diagram_when_foreign_keys_exist() {
        let orders = KirObject::new("orders", ObjectKind::Table);
        let customers = KirObject::new("customers", ObjectKind::Table);
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, customers.id);

        let page = render_architecture(&[orders, customers], &[rel], &[]);
        assert!(page.content.contains("## Entity Relationships"));
        assert!(page.content.contains("erDiagram"));
    }

    #[test]
    fn api_lists_files_with_symbols_grouped_by_file() {
        let file = KirObject::new("service.py", ObjectKind::File).with_property(
            "symbols",
            serde_json::json!(["handle_request", "parse_body"]),
        );
        let page = render_api(&[file], &[]);
        assert_eq!(page.file_name, "API.md");
        assert!(page.content.contains("## service.py"));
        assert!(page.content.contains("- `handle_request`"));
        assert!(page.content.contains("- `parse_body`"));
        assert!(page.content.contains("falling back to symbol names only"));
    }

    #[test]
    fn api_on_no_symbols_is_honest_not_a_fabricated_surface() {
        let file = KirObject::new("empty.py", ObjectKind::File);
        let page = render_api(&[file], &[]);
        assert!(page.content.contains("No API surface data compiled."));
    }

    #[test]
    fn api_prefers_real_rust_symbol_objects_over_the_legacy_file_symbols_fallback() {
        let file = KirObject::new("crates/kir/src/lib.rs", ObjectKind::File);
        let function = KirObject::new(
            "build_object_page_model",
            ObjectKind::Custom("RustSymbol".to_string()),
        )
        .with_property("kind", serde_json::json!("function"));
        let contains = KirRelationship::new(RelationshipKind::Contains, file.id, function.id);

        let page = render_api(&[file.clone(), function], &[contains]);
        assert_eq!(page.file_name, "API.md");
        assert!(page.content.contains(&format!("## {}", file.name)));
        assert!(page.content.contains("`function`"));
        assert!(page.content.contains("build_object_page_model"));
        assert!(!page.content.contains("falling back to symbol names only"));
    }

    #[test]
    fn api_groups_elixir_symbols_by_their_owning_module_not_their_file() {
        // RFC 0081: elixir_analyzer.rs emits `File Contains Module Contains Symbol`, not `File
        // Contains Symbol` directly like Rust/Python — API.md must resolve the two-level
        // containment instead of leaving every Elixir symbol in the "not compiled" bucket.
        let file = KirObject::new("lib/plausible/auth/password.ex", ObjectKind::File);
        let module = KirObject::new(
            "Plausible.Auth.Password",
            ObjectKind::Custom("ElixirModule".to_string()),
        );
        let function = KirObject::new("hash", ObjectKind::Custom("ElixirSymbol".to_string()))
            .with_property("kind", serde_json::json!("function"));
        let file_contains_module =
            KirRelationship::new(RelationshipKind::Contains, file.id, module.id);
        let module_contains_symbol =
            KirRelationship::new(RelationshipKind::Contains, module.id, function.id);

        let page = render_api(
            &[file, module, function],
            &[file_contains_module, module_contains_symbol],
        );
        assert!(page.content.contains("## Plausible.Auth.Password"));
        assert!(page.content.contains("hash"));
        assert!(!page.content.contains("(containing file not compiled)"));
    }

    #[test]
    fn sequence_diagrams_render_one_block_per_distinct_origin() {
        let a0 = KirObject::new(
            "job_a.ktr:0",
            ObjectKind::Custom("TransformNode".to_string()),
        );
        let a1 = {
            let mut o = KirObject::new(
                "job_a.ktr:1",
                ObjectKind::Custom("TransformNode".to_string()),
            );
            o.properties.insert("node_type".to_string(), "Sink".into());
            o
        };
        let b0 = KirObject::new(
            "job_b.ktr:0",
            ObjectKind::Custom("TransformNode".to_string()),
        );
        let rel = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            a0.id,
            a1.id,
        );

        let page = render_sequence_diagrams(&[a0, a1, b0], &[rel]);
        assert_eq!(page.file_name, "SequenceDiagrams.md");
        assert!(page.content.contains("## job_a.ktr"));
        assert!(page.content.contains("## job_b.ktr"));
        assert!(page.content.contains("sequenceDiagram"));
        assert!(page.content.contains(": Sink"));
        assert!(page.content.contains("data-flow sequence"));
        let job_a_pos = page.content.find("## job_a.ktr").unwrap();
        let job_b_pos = page.content.find("## job_b.ktr").unwrap();
        assert!(
            job_a_pos < job_b_pos,
            "two origins render as two separate blocks, not merged"
        );
    }

    #[test]
    fn sequence_diagrams_on_no_transform_nodes_is_honest_not_a_fabricated_flow() {
        let page = render_sequence_diagrams(&[], &[]);
        assert!(
            page.content
                .contains("No transformation pipelines compiled.")
        );
        assert!(page.content.contains("No `Calls` relationships compiled."));
    }

    #[test]
    fn sequence_diagrams_renders_call_sequences_grouped_by_caller_module() {
        let file = KirObject::new("crates/kir/src/lib.rs", ObjectKind::File);
        let caller = KirObject::new(
            "build_object_page_model",
            ObjectKind::Custom("RustSymbol".to_string()),
        );
        let callee = KirObject::new(
            "render_markdown_object_page",
            ObjectKind::Custom("RustSymbol".to_string()),
        );
        let contains = KirRelationship::new(RelationshipKind::Contains, file.id, caller.id);
        let calls = KirRelationship::new(RelationshipKind::Calls, caller.id, callee.id);

        let page = render_sequence_diagrams(&[file.clone(), caller, callee], &[contains, calls]);
        assert!(page.content.contains("## Call Sequences"));
        assert!(page.content.contains(&format!("### {}", file.name)));
        assert!(page.content.contains("build_object_page_model"));
        assert!(page.content.contains("render_markdown_object_page"));
        assert!(page.content.contains("sequenceDiagram"));
    }

    #[test]
    fn sequence_diagrams_single_step_pipeline_notes_no_edges() {
        let solo = KirObject::new(
            "solo.ktr:0",
            ObjectKind::Custom("TransformNode".to_string()),
        );
        let page = render_sequence_diagrams(&[solo], &[]);
        assert!(
            page.content
                .contains("1 step — no `FeedsInto` edges compiled")
        );
    }

    /// Regression test for a real bug found by running this against a real Pentaho `.kjb` job:
    /// job-orchestration entries are always `Unmapped` (never wired together), so a job can have
    /// several participants and zero edges — the placeholder said "single step" even with 8
    /// participants before this fix.
    #[test]
    fn sequence_diagrams_multi_step_pipeline_with_no_edges_uses_plural_wording() {
        let a = KirObject::new("job.kjb:0", ObjectKind::Custom("TransformNode".to_string()));
        let b = KirObject::new("job.kjb:1", ObjectKind::Custom("TransformNode".to_string()));
        let c = KirObject::new("job.kjb:2", ObjectKind::Custom("TransformNode".to_string()));
        let page = render_sequence_diagrams(&[a, b, c], &[]);
        assert!(
            page.content
                .contains("3 steps — no `FeedsInto` edges compiled")
        );
        assert!(!page.content.contains("single step"));
    }

    /// RFC 0086 (Phase 6): a real Ecto Repo `ElixirModule` (Backend, inherited from its owning
    /// `File` via `Contains`) with a real `DependsOn` edge to a database-adapter `Technology`
    /// object must produce a real Backend→Database cross-tier edge in System Decomposition.
    #[test]
    fn ecto_repo_adapter_produces_a_real_backend_to_database_edge() {
        let file = KirObject::new("lib/plausible/repo.ex", ObjectKind::File);
        let module = KirObject::new(
            "Plausible.Repo",
            ObjectKind::Custom("ElixirModule".to_string()),
        );
        let mut postgres =
            KirObject::new("PostgreSQL", ObjectKind::Custom("Technology".to_string()));
        postgres
            .properties
            .insert("ecosystem".into(), serde_json::json!("database"));
        let contains = KirRelationship::new(RelationshipKind::Contains, file.id, module.id);
        let depends_on = KirRelationship::new(RelationshipKind::DependsOn, module.id, postgres.id);

        let (nodes, edges) =
            system_decomposition_graph(&[file, module, postgres], &[contains, depends_on], &[])
                .unwrap();

        assert!(nodes.iter().any(|(id, _)| id == "layer_backend"));
        assert!(nodes.iter().any(|(id, _)| id == "layer_sql"));
        assert!(
            edges
                .iter()
                .any(|(from, to)| from == "layer_backend" && to == "layer_sql")
        );
    }

    #[test]
    fn a_clickhouse_ecto_adapter_routes_to_the_clickhouse_bucket_not_sql() {
        let file = KirObject::new("lib/plausible/clickhouse_repo.ex", ObjectKind::File);
        let module = KirObject::new(
            "Plausible.ClickhouseRepo",
            ObjectKind::Custom("ElixirModule".to_string()),
        );
        let mut clickhouse =
            KirObject::new("ClickHouse", ObjectKind::Custom("Technology".to_string()));
        clickhouse
            .properties
            .insert("ecosystem".into(), serde_json::json!("database"));
        let contains = KirRelationship::new(RelationshipKind::Contains, file.id, module.id);
        let depends_on =
            KirRelationship::new(RelationshipKind::DependsOn, module.id, clickhouse.id);

        let (_, edges) =
            system_decomposition_graph(&[file, module, clickhouse], &[contains, depends_on], &[])
                .unwrap();

        assert!(
            edges
                .iter()
                .any(|(from, to)| from == "layer_backend" && to == "layer_clickhouse")
        );
        assert!(!edges.iter().any(|(_, to)| to == "layer_sql"));
    }

    /// Real Backend/Frontend *counts* must stay exactly the real `File` count — a module that
    /// merely lives inside a Backend file must not inflate the displayed "Backend (N files)"
    /// number, even though it does need to resolve to a layer for edge purposes.
    #[test]
    fn module_layer_inheritance_never_inflates_the_displayed_file_counts() {
        let file = KirObject::new("lib/plausible/repo.ex", ObjectKind::File);
        let module = KirObject::new(
            "Plausible.Repo",
            ObjectKind::Custom("ElixirModule".to_string()),
        );
        let contains = KirRelationship::new(RelationshipKind::Contains, file.id, module.id);

        let (nodes, _) =
            system_decomposition_graph(&[file, module], std::slice::from_ref(&contains), &[])
                .unwrap();

        let backend_label = &nodes
            .iter()
            .find(|(id, _)| id == "layer_backend")
            .unwrap()
            .1;
        assert_eq!(backend_label, "Backend (1 file)");
    }
}
