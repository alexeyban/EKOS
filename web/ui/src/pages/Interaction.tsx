// RFC 0256 — the interaction map: who reads and writes each table, declared by code and observed
// in the database, and the findings that need a person.
//
// Read-only. Accounting for a database role is a review decision (`ekos review interaction`),
// made on the Review page like every other.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useParams } from "react-router-dom";
import { api } from "../api/client";

export type Observed = { calls: number; rows: number; window_start?: string; window_end: string; truncated?: boolean };
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
export type Matrix = { tables: MatrixRow[]; observed_facts: number; observed_note: string };
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
};

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

  return (
    <div className="sem-page">
      <h2>Findings</h2>
      {findings.isLoading && <p className="muted">Loading…</p>}
      {findings.error && <p className="err">{String(findings.error)}</p>}
      {findings.data && findings.data.length === 0 && (
        <p className="muted">No findings. Without an observation of the database (`ekos interaction observe`) none is derived.</p>
      )}
      {findings.data && findings.data.length > 0 && (
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
            {findings.data.map((f) => (
              <tr key={f.id}>
                <td>
                  <span className="chip warn">{FINDING_LABELS[f.finding] ?? f.finding}</span>
                </td>
                <td>
                  <button type="button" className="linkish" onClick={() => setTable(f.table)}>
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
      <h2>Who reads and writes</h2>
      <div className="sem-views">
        <input aria-label="table" placeholder="table" value={table} onChange={(e) => setTable(e.target.value)} />
        {matrix.data && <span className="muted sem-note">{matrix.data.observed_note}</span>}
      </div>
      {matrix.isLoading && <p className="muted">Loading…</p>}
      {matrix.error && <p className="err">{String(matrix.error)}</p>}
      {matrix.data && rows.length === 0 && <p className="muted">No table access found.</p>}
      {rows.map((r) => (
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
