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
use std::collections::{HashMap, HashSet};

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
    pub properties: Vec<(String, String)>,
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

    let mut properties: Vec<(String, String)> = object
        .properties
        .iter()
        .map(|(k, v)| (k.clone(), format_value(v)))
        .collect();
    properties.sort_by(|a, b| a.0.cmp(&b.0));

    let mut by_kind: HashMap<String, Vec<RelationshipRow>> = HashMap::new();
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
        by_kind
            .entry(rel.kind.to_string())
            .or_default()
            .push(RelationshipRow {
                outgoing,
                other_id,
                other_name: object_names.get(&other_id).cloned(),
                evidence: row_evidence,
            });
    }
    let mut relationship_groups: Vec<(String, Vec<RelationshipRow>)> =
        by_kind.into_iter().collect();
    relationship_groups.sort_by(|a, b| a.0.cmp(&b.0));

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

/// Render `Architecture.md`: component counts, real `Custom("Technology")` dependencies
/// (`dependency_analyzer.rs`), the existing ER diagram when `Table`/`ForeignKey` data exists, and
/// one small Mermaid graph per *structural* relationship kind. `Custom("FeedsInto")` edges are
/// deliberately excluded here — pipeline-internal step wiring belongs in `SequenceDiagrams.md`; a
/// real Pentaho workspace has dozens of `TransformNode`s, so inlining that here would make the
/// diagram unreadable. Splitting by relationship *purpose* is this RFC's answer to RFC 0035's
/// still-open "diagram size" question, for the curated layout.
pub fn render_architecture(
    objects: &[KirObject],
    relationships: &[KirRelationship],
) -> RenderedPage {
    let mut out = String::from("# Architecture\n\n");

    out.push_str("## Components\n\n");
    let counts = count_by_kind(objects, is_significant);
    if counts.is_empty() {
        out.push_str("_No compiled objects yet._\n\n");
    } else {
        for (kind, count) in &counts {
            out.push_str(&format!("- **{kind}**: {count}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Technologies\n\n");
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
            let dependents: Vec<&str> = relationships
                .iter()
                .filter(|r| r.to == tech.id && matches!(r.kind, RelationshipKind::DependsOn))
                .filter_map(|r| name_by_id.get(&r.from).copied())
                .collect();
            let used_by = if dependents.is_empty() {
                "_no linked files_".to_string()
            } else {
                dependents.join(", ")
            };
            out.push_str(&format!("- **{}** — used by: {used_by}\n", tech.name));
        }
        out.push('\n');
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
            if rels.len() > MAX_GRAPH_EDGES {
                out.push_str(&format!(
                    "_{} `{kind}` relationships compiled — diagram omitted, too large to render \
                     usefully. See `ekos docs generate --layout objects` for per-object detail._\n\n",
                    rels.len()
                ));
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

/// Render `API.md`: real `File` objects carrying a `symbols` property (bare identifier names
/// harvested by a substring scan for declaration-line prefixes — `plugins/file/src/lib.rs`),
/// grouped by file. Explicitly caveated as symbol names only, not a parsed API spec, since no
/// analyzer compiles `ObjectKind::Api`/`ObjectKind::Service` objects today.
pub fn render_api(objects: &[KirObject]) -> RenderedPage {
    let mut out = String::from(
        "# API\n\n_Symbol names only, extracted via a lightweight text scan for \
         declaration-line prefixes (`fn `, `def `, `class `, `func `, `interface `) — not a \
         parsed API spec. Real `Api`/`Service` objects, if ever compiled, would render here \
         directly; none are compiled today._\n\n",
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

/// Render `SequenceDiagrams.md`: one Mermaid `sequenceDiagram` per compiled Transformation IR
/// pipeline (grouped by origin), one message per `FeedsInto` edge within that origin, labeled
/// with the target node's `node_type`. Explicitly labeled as a **data-flow** sequence, not a code
/// call sequence — no analyzer compiles `RelationshipKind::Calls` data today, so this renders the
/// only real *ordered* flow data that exists rather than fabricating one.
pub fn render_sequence_diagrams(
    objects: &[KirObject],
    relationships: &[KirRelationship],
) -> RenderedPage {
    let mut out = String::from(
        "# Sequence Diagrams\n\n_Rendered from Transformation IR `FeedsInto` edges — a \
         data-flow sequence between compiled pipeline steps, not a code call sequence. EKOS does \
         not compile call-graph data today._\n\n",
    );

    let nodes: Vec<&KirObject> = objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "TransformNode"))
        .collect();
    if nodes.is_empty() {
        out.push_str("_No transformation pipelines compiled._\n");
        return RenderedPage {
            file_name: "SequenceDiagrams.md".to_string(),
            content: out,
        };
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
        assert!(page.content.contains("### ForeignKey"));
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
    fn relationships_group_by_kind_without_dropping_non_foreign_key() {
        let table = sample_table();
        let a = KirId::new();
        let b = KirId::new();
        let fk = KirRelationship::new(RelationshipKind::ForeignKey, table.id, a);
        let coupled = KirRelationship::new(RelationshipKind::CoupledWith, table.id, b);

        let page = render_object_page(&table, &[fk, coupled], &[], &HashMap::new());
        assert!(page.content.contains("### ForeignKey"));
        assert!(page.content.contains("### CoupledWith"));
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

        let page = render_architecture(&[file, tech], &[rel]);
        assert_eq!(page.file_name, "Architecture.md");
        assert!(page.content.contains("**PostgreSQL** — used by: db.py"));
    }

    #[test]
    fn architecture_on_no_technologies_is_honest_not_a_fabricated_list() {
        let page = render_architecture(&[], &[]);
        assert!(
            page.content
                .contains("No technology dependencies compiled.")
        );
        assert!(
            page.content
                .contains("No table foreign-key relationships compiled.")
        );
        assert!(
            page.content
                .contains("No structural relationships compiled.")
        );
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

        let page = render_architecture(&[a, b, c, d], &[feeds_into, coupled]);
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
        let page = render_architecture(&all_objects, &relationships);

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
    }

    #[test]
    fn architecture_embeds_er_diagram_when_foreign_keys_exist() {
        let orders = KirObject::new("orders", ObjectKind::Table);
        let customers = KirObject::new("customers", ObjectKind::Table);
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, orders.id, customers.id);

        let page = render_architecture(&[orders, customers], &[rel]);
        assert!(page.content.contains("## Entity Relationships"));
        assert!(page.content.contains("erDiagram"));
    }

    #[test]
    fn api_lists_files_with_symbols_grouped_by_file() {
        let file = KirObject::new("service.py", ObjectKind::File).with_property(
            "symbols",
            serde_json::json!(["handle_request", "parse_body"]),
        );
        let page = render_api(&[file]);
        assert_eq!(page.file_name, "API.md");
        assert!(page.content.contains("## service.py"));
        assert!(page.content.contains("- `handle_request`"));
        assert!(page.content.contains("- `parse_body`"));
        assert!(page.content.contains("Symbol names only"));
    }

    #[test]
    fn api_on_no_symbols_is_honest_not_a_fabricated_surface() {
        let file = KirObject::new("empty.py", ObjectKind::File);
        let page = render_api(&[file]);
        assert!(page.content.contains("No API surface data compiled."));
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
}
