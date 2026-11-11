// RFC 0257 — a transformation EKOS recovered (a view, a routine, a query, a Kettle job) drawn as
// a left-to-right chain of steps. For the selected step: what it does in words and in source text,
// its columns, its evidence and what people wrote about it. Selecting a step highlights its span in
// the source; clicking the source selects the step.
//
// Read-only except for annotations, which go through the existing human-only annotation route
// (`ekos annotate add` with the step's node id as the target). A recovered transformation is
// evidence of what the old system does: it is never edited here.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams, useSearchParams } from "react-router-dom";
import { api } from "../api/client";
import { type Me, mayOperate } from "../WorkspaceShell";
import { AddForm } from "./Annotations";
import { reach, stateChip } from "./annotations-shared";
import {
  type ChainGraph,
  type ChainNode,
  type TransformationList,
  badgeOf,
  chainLayout,
  expandWith,
  segments,
  stepTitle,
} from "./chain-shared";
import { show, tone } from "./extension-view";

export function Transformations() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const [params, setParams] = useSearchParams();
  const object = params.get("object") ?? "";
  const [filter, setFilter] = useState("");
  const list = useQuery({
    queryKey: ["transformations", id],
    queryFn: () => api<TransformationList>(`/workspaces/${id}/transformations?limit=5000`),
    enabled: id !== "",
  });
  const needle = filter.trim().toLowerCase();
  const rows = (list.data?.transformations ?? []).filter(
    (r) => !needle || r.name.toLowerCase().includes(needle),
  );
  const open = (o: string) => setParams(o ? { object: o } : {});

  return (
    <div className="sem-page">
      <div className="sem-views">
        <input
          aria-label="filter"
          placeholder="view, routine or file name"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        {list.data && (
          <span className="muted sem-note">
            {rows.length} of {list.data.total} transformation(s)
            {list.data.hidden > 0 && ` · ${list.data.hidden} statement(s) nothing was understood of not listed`}
          </span>
        )}
      </div>
      {list.isError && <p className="err">{String(list.error)}</p>}
      <div className={object ? "lm-layout" : "sem-layout"}>
        <section className="card lm-tree" aria-label="transformations">
          {list.isLoading && <p className="muted">loading…</p>}
          {list.data && list.data.total === 0 && (
            <p className="muted">
              No transformation was recovered yet: views, routines and queries get a graph at{" "}
              <code>ekos recover</code>.
            </p>
          )}
          <ul className="sem-items">
            {rows.slice(0, 300).map((r) => (
              <li key={r.key} className={r.object === object ? "selected" : undefined}>
                <button className="sem-item-pick" onClick={() => open(r.object)}>
                  {r.name}
                </button>
                <div className="muted">
                  <span className="chip small">{r.kind}</span> {r.steps} step(s)
                  {r.unmapped > 0 && <span className="chip small warn">{r.unmapped} not understood</span>}
                </div>
              </li>
            ))}
          </ul>
        </section>
        {object && <ChainPage workspace={id} object={object} me={me} />}
      </div>
    </div>
  );
}

function ChainPage({ workspace, object, me }: { workspace: string; object: string; me: Me }) {
  const [expand, setExpand] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const graph = useQuery({
    // Under "annotations" so that adding one on a step (which invalidates that key) redraws it.
    queryKey: ["annotations", "chain", workspace, object, expand],
    queryFn: () =>
      api<ChainGraph>(
        `/workspaces/${workspace}/transformations/graph?object=${object}` +
          (expand ? `&expand=${encodeURIComponent(expand)}` : ""),
      ),
  });
  const g = graph.data;
  const step = g?.nodes.find((n) => n.id === selected) ?? null;
  return (
    <section className="card" aria-label="chain">
      {graph.isLoading && <p className="muted">loading…</p>}
      {graph.isError && <p className="err">{String(graph.error)}</p>}
      {g && (
        <>
          <h3 style={{ marginTop: 0 }}>
            <span className="chip">{g.transformation.kind}</span> {g.transformation.name}
          </h3>
          <p className="muted">
            {g.counts.nodes} step(s), {g.counts.unmapped} not understood by recovery · {g.transformation.path}
            {g.collapsed && " · collapsed by statement: select a statement to see its steps"}
          </p>
          {g.collapsed && (
            <div className="btnrow">
              <button className="pill" onClick={() => setExpand("all")}>
                show every step
              </button>
            </div>
          )}
          <ChainView graph={g} selected={selected} onSelect={setSelected} />
          <div className="sem-layout with-panel">
            <SourcePanel graph={g} selected={selected} onSelect={setSelected} />
            {step ? (
              <StepPanel
                workspace={workspace}
                me={me}
                step={step}
                rows={g.rows}
                onExpand={() => setExpand(expandWith(expand, step.statement))}
              />
            ) : (
              <p className="muted">Select a step to see what it does.</p>
            )}
          </div>
        </>
      )}
    </section>
  );
}

const W = 160;
const H = 28;

export function ChainView({
  graph,
  selected,
  onSelect,
}: {
  graph: ChainGraph;
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const layout = chainLayout(graph);
  const pos = new Map(layout.nodes.map((n) => [n.id, n]));
  const lanes = new Map<string, number>();
  for (const n of layout.nodes) lanes.set(n.group, Math.min(lanes.get(n.group) ?? n.y, n.y));
  const kinds = new Map(graph.edges.map((e) => [`${e.from}>${e.to}`, e.kind]));
  return (
    <div className="ext-graph" style={{ overflowX: "auto" }}>
      <svg width={layout.width + 20} height={layout.height + 36} role="img" aria-label="transformation chain">
        <defs>
          <marker id="chain-arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M0,0 L10,5 L0,10 z" fill="currentColor" />
          </marker>
        </defs>
        <g transform="translate(10,24)">
          {[...lanes].map(([lane, y]) => (
            <text key={lane} x={0} y={y - 6} className="ext-lane">
              {lane}
            </text>
          ))}
          {layout.edges.map((e, i) => {
            const a = pos.get(e.from);
            const b = pos.get(e.to);
            if (!a || !b) return null;
            const order = kinds.get(`${e.from}>${e.to}`) === "order";
            return (
              <line
                key={i}
                x1={a.x + W}
                y1={a.y + H / 2}
                x2={b.x}
                y2={b.y + H / 2}
                stroke="currentColor"
                opacity={0.4}
                strokeDasharray={order ? "4 3" : undefined}
                markerEnd="url(#chain-arrow)"
              />
            );
          })}
          {layout.nodes.map((n) => (
            <g
              key={n.id}
              transform={`translate(${n.x},${n.y})`}
              className={`ext-node ${tone(n.badge)}${n.id === selected ? " chain-selected" : ""}`}
              role="button"
              tabIndex={0}
              aria-label={`step ${n.label}`}
              onClick={() => onSelect(n.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") onSelect(n.id);
              }}
              style={{ cursor: "pointer" }}
            >
              <rect width={W} height={H} rx={5} strokeWidth={n.id === selected ? 2.5 : 1} />
              <text x={8} y={18}>
                {n.label.length > 22 ? `${n.label.slice(0, 21)}…` : n.label}
              </text>
              <title>{`${n.label}${show(n.badge) === "–" ? "" : ` (${show(n.badge)})`}`}</title>
            </g>
          ))}
        </g>
      </svg>
    </div>
  );
}

export function SourcePanel({
  graph,
  selected,
  onSelect,
}: {
  graph: ChainGraph;
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  if (!graph.source) {
    return (
      <p className="muted">
        No definition text: recovery recorded no span for this statement, or the file no longer holds it.
      </p>
    );
  }
  const text = graph.source.text;
  const segs = segments(text.length, graph.spans, selected);
  return (
    <div>
      <p className="muted">
        {graph.source.path}:{graph.source.start_line} — read-only
      </p>
      <pre className="props" aria-label="source text" style={{ whiteSpace: "pre-wrap" }}>
        {segs.map((s) => {
          const piece = text.slice(s.start, s.end);
          if (!s.node) return <span key={s.start}>{piece}</span>;
          const node = s.node;
          return (
            <button
              type="button"
              key={s.start}
              className={s.selected ? "chain-span selected" : "chain-span"}
              aria-pressed={s.selected}
              onClick={() => onSelect(node)}
            >
              {piece}
            </button>
          );
        })}
      </pre>
    </div>
  );
}

function Columns({ label, cols }: { label: string; cols: string[] }) {
  if (cols.length === 0) return null;
  return (
    <p>
      <span className="muted">{label}:</span> {cols.map((c) => <code key={c}>{c} </code>)}
    </p>
  );
}

export function StepPanel({
  workspace,
  me,
  step,
  rows,
  onExpand,
}: {
  workspace: string;
  me: Me;
  step: ChainNode;
  rows: ChainGraph["rows"];
  onExpand: () => void;
}) {
  const notes = step.annotations.filter((a) => a.state !== "withdrawn");
  return (
    <aside className="obj-panel" aria-label="step">
      <strong>{stepTitle(step)}</strong>{" "}
      {badgeOf(step) && <span className={`chip small ${tone(badgeOf(step))}`}>{badgeOf(step)}</span>}
      <p>{step.summary}</p>
      {step.type === "Statement" && (
        <div className="btnrow">
          <button className="pill" onClick={onExpand}>
            show its steps
          </button>
        </div>
      )}
      {step.source_text && (
        <details open>
          <summary>Source text</summary>
          <pre className="props">{step.source_text}</pre>
        </details>
      )}
      <Columns label="columns in" cols={step.columns_in} />
      <Columns label="columns used" cols={step.columns_used} />
      <Columns label="columns out" cols={step.columns_out} />
      <p className="muted">
        {step.term ? `term ${step.term} · ${step.rule ?? ""} · ${step.fidelity ?? ""}` : "recovered: no term or fidelity"}
        {" · "}
        rows: {rows.available ? "available" : rows.reason}
      </p>
      {step.evidence.length > 0 && (
        <details>
          <summary>Evidence ({step.evidence.length})</summary>
          <ul className="rel-list">
            {step.evidence.map((e, i) => (
              <li key={i}>
                <span className="muted">
                  {e.path}
                  {e.line ? `:${e.line}` : ""}
                </span>
                <div>
                  <code>{e.fragment}</code>
                </div>
              </li>
            ))}
          </ul>
        </details>
      )}
      <details open>
        <summary>Notes from people ({notes.length})</summary>
        <ul className="rel-list">
          {notes.map((a) => (
            <li key={a.id}>
              <span className={stateChip(a.state)}>{a.state}</span>{" "}
              <span className="muted">
                {a.kind} by {a.author} · {reach(a)}
              </span>
              <div>{a.text}</div>
            </li>
          ))}
        </ul>
        {step.type !== "Statement" && mayOperate(me) && !me.read_only && (
          <AddForm workspace={workspace} me={me} fixedTarget={step.id} title="Say what is wrong with this step" />
        )}
      </details>
    </aside>
  );
}
