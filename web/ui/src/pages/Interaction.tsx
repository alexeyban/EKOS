// RFC 0256 — the interaction map: who reads and writes each table, declared by code and observed
// in the database, and the findings that need a person.
//
// Read-only. Accounting for a database role is a review decision (`ekos review interaction`),
// made on the Review page like every other.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useParams } from "react-router-dom";
import { api } from "../api/client";
import { layoutGraph } from "./extension-view";

export type Observed = {
  calls: number;
  rows: number;
  window_start?: string;
  window_end: string;
  truncated?: boolean;
  first_observed?: string;
};
export type Cell = {
  actor: string;
  kind: string;
  name: string;
  declared: string[];
  observed?: Record<string, Observed>;
  via?: string[];
  writes: boolean;
};
export type MatrixRow = { table: string; cells: Cell[] };
export type Matrix = {
  tables: MatrixRow[];
  observed_facts: number;
  observed_note: string;
  /** Databases whose engine gave no usage statistics, with the reason (RFC 0256 §3.2). */
  no_usage_statistics?: Record<string, string>;
};
export type Finding = {
  id: string;
  finding: string;
  table: string;
  actor?: string | null;
  operations?: string[];
  detail?: Record<string, unknown>;
};

export const FINDING_LABELS: Record<string, string> = {
  unexplained_writer: "unexplained writer",
  unattributed_writes: "writes no statement explains",
  multiple_writing_systems: "several writing systems",
  declared_not_observed: "declared, never observed",
  read_only_consumers: "only read by",
};

/** Findings that ask a person something; `read_only_consumers` only lists who a change breaks. */
export const ATTENTION = new Set(["unexplained_writer", "unattributed_writes", "multiple_writing_systems"]);

/** The kinds a system-level graph shows: systems and roles, not the routines they call. */
const SYSTEM_KINDS = new Set(["component", "etl_job", "process", "db_role"]);

export type SystemGraph = {
  nodes: { id: string; label: string; group: string; badge: unknown }[];
  edges: { from: string; to: string }[];
  /** Tables left out to keep the picture readable. */
  omitted: number;
};

/**
 * RFC 0256 §3.5: the system-level picture — writers → table → readers, for systems (components,
 * ETL jobs) and database roles. A table with an open finding is marked. At most `maxTables`
 * tables, the most written first.
 */
const writers = (r: MatrixRow) => r.cells.filter((c) => c.writes).length;

/** Most written first, then by name. */
const byWriters = (a: MatrixRow, b: MatrixRow) => writers(b) - writers(a) || a.table.localeCompare(b.table);

/** A writer points into the table, a reader out of it. */
const edgeOf = (c: Cell, table: string) => (c.writes ? { from: c.actor, to: table } : { from: table, to: c.actor });

export function systemGraph(m: Matrix, flagged: Set<string>, maxTables = 25): SystemGraph {
  const rows = m.tables
    .map((r) => ({ ...r, cells: r.cells.filter((c) => SYSTEM_KINDS.has(c.kind)) }))
    .filter((r) => r.cells.length > 0)
    .sort(byWriters);
  const shown = rows.slice(0, maxTables);
  const nodes = new Map<string, { id: string; label: string; group: string; badge: unknown }>();
  const edges: { from: string; to: string }[] = [];
  for (const r of shown) {
    const t = `table:${r.table}`;
    nodes.set(t, { id: t, label: r.table, group: "table", badge: flagged.has(r.table) ? "warning" : "" });
    for (const c of r.cells) {
      const badge = c.observed ? "observed" : "";
      if (!nodes.has(c.actor)) nodes.set(c.actor, { id: c.actor, label: c.name, group: c.kind, badge });
      edges.push(edgeOf(c, t));
    }
  }
  return { nodes: [...nodes.values()], edges, omitted: rows.length - shown.length };
}

function InteractionGraph({ g }: { g: SystemGraph }) {
  if (g.nodes.length === 0) return <p className="muted">No system or role touches these tables.</p>;
  const laid = layoutGraph(g.nodes, g.edges);
  const w = 160;
  const h = 28;
  const pos = new Map(laid.nodes.map((n) => [n.id, n]));
  return (
    <div className="ext-graph" style={{ overflowX: "auto" }}>
      <svg width={laid.width + 20} height={laid.height + 36} role="img" aria-label="who writes and reads which table">
        <defs>
          <marker id="ia-arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M0,0 L10,5 L0,10 z" fill="currentColor" />
          </marker>
        </defs>
        <g transform="translate(10,24)">
          {laid.edges.map((e) => {
            const a = pos.get(e.from);
            const b = pos.get(e.to);
            if (!a || !b) return null;
            return (
              <line
                key={`${e.from}->${e.to}`}
                x1={a.x + w}
                y1={a.y + h / 2}
                x2={b.x}
                y2={b.y + h / 2}
                stroke="currentColor"
                opacity={0.4}
                markerEnd="url(#ia-arrow)"
              />
            );
          })}
          {laid.nodes.map((n) => (
            <g
              key={n.id}
              transform={`translate(${n.x},${n.y})`}
              className={`ext-node ${n.badge === "warning" ? "warn" : n.badge === "observed" ? "ok" : ""}`}
            >
              <rect width={w} height={h} rx={5} />
              <text x={8} y={18}>
                {n.label.length > 22 ? `${n.label.slice(0, 21)}…` : n.label}
              </text>
              <title>{`${n.group}: ${n.label}`}</title>
            </g>
          ))}
        </g>
      </svg>
      {g.omitted > 0 && <p className="muted">{g.omitted} more table(s) not drawn: filter by table to see one.</p>}
    </div>
  );
}

/** One cell's operations in words: `update (declared, observed 3)`. */
export function opsText(c: Cell): string {
  const parts: string[] = [];
  for (const op of c.declared) {
    const seen = c.observed?.[op];
    parts.push(seen ? `${op} (declared, observed ${seen.calls})` : `${op} (declared)`);
  }
  for (const [op, o] of Object.entries(c.observed ?? {})) {
    if (!c.declared.includes(op)) parts.push(`${op} (observed ${o.calls})`);
  }
  return parts.join(", ");
}

/** Writers first, then by kind and name. */
export function sortCells(cells: Cell[]): Cell[] {
  return [...cells].sort(
    (a, b) => Number(b.writes) - Number(a.writes) || a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name),
  );
}

/** The findings that ask a person something, then the read-only consumers (folded). */
function FindingsList({
  problems,
  consumers,
  onTable,
}: {
  problems: Finding[];
  consumers: Finding[];
  onTable: (t: string) => void;
}) {
  return (
    <>
      {problems.length === 0 && (
        <p className="muted">No findings. Without an observation of the database (`ekos interaction observe`) none is derived.</p>
      )}
      {problems.length > 0 && (
        <table>
          <thead>
            <tr>
              <th>finding</th>
              <th>table</th>
              <th>actor</th>
              <th>operations</th>
            </tr>
          </thead>
          <tbody>
            {problems.map((f) => (
              <tr key={f.id}>
                <td>
                  <span className="chip warn">{FINDING_LABELS[f.finding] ?? f.finding}</span>
                </td>
                <td>
                  <button type="button" className="linkish" onClick={() => onTable(f.table)}>
                    {f.table}
                  </button>
                </td>
                <td>
                  <code>{f.actor ?? "—"}</code>
                </td>
                <td>{(f.operations ?? []).join(", ")}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {consumers.length > 0 && (
        <details>
          <summary>Read-only consumers of {consumers.length} table(s): who a change breaks</summary>
          <ul>
            {consumers.map((f) => (
              <li key={f.id}>
                <button type="button" className="linkish" onClick={() => onTable(f.table)}>
                  {f.table}
                </button>
                : {((f.detail?.readers as string[] | undefined) ?? []).join(", ")}
              </li>
            ))}
          </ul>
        </details>
      )}
    </>
  );
}

export function Interaction() {
  const { id = "" } = useParams();
  const [table, setTable] = useState("");
  const params = new URLSearchParams();
  if (table.trim()) params.set("table", table.trim());
  const matrix = useQuery({
    queryKey: ["interaction", id, params.toString()],
    queryFn: () => api<Matrix>(`/workspaces/${id}/interaction?${params}`),
    enabled: id !== "",
  });
  const findings = useQuery({
    queryKey: ["interaction-findings", id],
    queryFn: () => api<Finding[]>(`/workspaces/${id}/interaction/findings`),
    enabled: id !== "",
  });
  const rows = matrix.data?.tables ?? [];
  const [view, setView] = useState<"tables" | "graph">("tables");
  const problems = (findings.data ?? []).filter((f) => ATTENTION.has(f.finding));
  const consumers = (findings.data ?? []).filter((f) => !ATTENTION.has(f.finding));
  const none = Object.entries(matrix.data?.no_usage_statistics ?? {});

  return (
    <div className="sem-page">
      <h2>Findings</h2>
      {none.map(([db, why]) => (
        <p key={db} className="banner">
          <strong>{db}</strong>: {why}. No finding is derived from the absence of usage statistics.
        </p>
      ))}
      {findings.isLoading && <p className="muted">Loading…</p>}
      {findings.error && <p className="err">{String(findings.error)}</p>}
      {findings.data && <FindingsList problems={problems} consumers={consumers} onTable={setTable} />}
      <h2>Who reads and writes</h2>
      <div className="sem-views">
        <input aria-label="table" placeholder="table" value={table} onChange={(e) => setTable(e.target.value)} />
        <button type="button" className={`pill ${view === "tables" ? "active" : ""}`} onClick={() => setView("tables")}>
          Per table
        </button>
        <button type="button" className={`pill ${view === "graph" ? "active" : ""}`} onClick={() => setView("graph")}>
          Systems
        </button>
        {matrix.data && <span className="muted sem-note">{matrix.data.observed_note}</span>}
      </div>
      {matrix.isLoading && <p className="muted">Loading…</p>}
      {matrix.error && <p className="err">{String(matrix.error)}</p>}
      {matrix.data && rows.length === 0 && <p className="muted">No table access found.</p>}
      {view === "graph" && matrix.data && (
        <InteractionGraph g={systemGraph(matrix.data, new Set(problems.map((f) => f.table)))} />
      )}
      {view === "tables" && rows.map((r) => (
        <section key={r.table}>
          <h3>
            {r.table}{" "}
            <span className="muted">
              {r.cells.length} actor(s), {r.cells.filter((c) => c.writes).length} writing
            </span>
          </h3>
          <table>
            <thead>
              <tr>
                <th>kind</th>
                <th>actor</th>
                <th>operations</th>
              </tr>
            </thead>
            <tbody>
              {sortCells(r.cells).map((c) => (
                <tr key={c.actor}>
                  <td>{c.kind}</td>
                  <td>
                    <code>{c.name}</code>
                  </td>
                  <td>{opsText(c)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ))}
    </div>
  );
}
