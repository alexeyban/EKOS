// RFC 0233 — cross-reference integrity: references that resolve to nothing or to several objects,
// dependency cycles, unused tables, and SQL routine reachability.
//
// The page reads the integrity index the last `ekos commit` wrote; it never recomputes. A finding
// opens the referring object's evidence beside the list and links to it in the graph.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import type { XrefDiagnostic, XrefMetrics, XrefReport } from "../api/types";
import { ObjectPanel } from "./ObjectPanel";
import {
  DEFAULT_FILTERS,
  type Filters,
  KIND_LABEL,
  applyFilters,
  pct,
  severityChip,
  where,
} from "./integrity-shared";

const LIMIT = 500;

export function Integrity() {
  const { id = "" } = useParams();
  const [filters, setFilters] = useState<Filters>(DEFAULT_FILTERS);
  const [selected, setSelected] = useState<string | null>(null);
  const report = useQuery({
    queryKey: ["integrity", id],
    queryFn: () => api<XrefReport>(`/workspaces/${id}/integrity`),
    enabled: id !== "",
  });
  const all = report.data?.diagnostics ?? [];
  const shown = applyFilters(all, filters);
  const current = shown.find((d) => d.fingerprint === selected) ?? null;
  const set = (patch: Partial<Filters>) => setFilters((f) => ({ ...f, ...patch }));

  return (
    <div className="sem-page">
      {report.isLoading && <p className="muted">loading…</p>}
      {report.isError && (
        <p className="err">
          {String(report.error)} — the integrity index is written by <code>ekos commit</code>.
        </p>
      )}
      {report.data && <MetricsBar metrics={report.data.metrics} at={report.data.computed_at} />}
      <div className="sem-views">
        <select aria-label="kind" value={filters.kind} onChange={(e) => set({ kind: e.target.value })}>
          <option value="">every kind</option>
          {Object.entries(KIND_LABEL).map(([k, label]) => (
            <option key={k} value={k}>
              {label}
            </option>
          ))}
        </select>
        <select
          aria-label="severity"
          value={filters.severity}
          onChange={(e) => set({ severity: e.target.value })}
        >
          <option value="">any severity</option>
          <option value="error">error</option>
          <option value="warning">warning</option>
          <option value="info">info</option>
        </select>
        <label className="muted">
          confidence ≥{" "}
          <input
            aria-label="minimum confidence"
            type="number"
            min={0}
            max={1}
            step={0.1}
            value={filters.minConfidence}
            onChange={(e) => set({ minConfidence: Number(e.target.value) || 0 })}
          />
        </label>
        <label>
          <input
            type="checkbox"
            checked={filters.crossOnly}
            onChange={(e) => set({ crossOnly: e.target.checked })}
          />{" "}
          cross-system only
        </label>
        <label>
          <input
            type="checkbox"
            checked={filters.showGaps}
            onChange={(e) => set({ showGaps: e.target.checked })}
          />{" "}
          coverage gaps
        </label>
        <input
          aria-label="filter"
          placeholder="name or file"
          value={filters.text}
          onChange={(e) => set({ text: e.target.value })}
        />
      </div>
      <div className={current ? "sem-layout with-panel" : "sem-layout"}>
        <section className="card sem-list">
          <p className="muted">
            {shown.length} of {all.length} finding(s)
            {shown.length > LIMIT && ` — first ${LIMIT} shown`}
          </p>
          {report.data && all.length === 0 && (
            <p className="muted">No findings: every checked reference resolves to one object.</p>
          )}
          <ul className="sem-items">
            {shown.slice(0, LIMIT).map((d) => (
              <Finding
                key={d.fingerprint}
                d={d}
                selected={selected === d.fingerprint}
                onPick={() => setSelected(d.fingerprint)}
              />
            ))}
          </ul>
        </section>
        {current?.source.object_id && (
          <aside className="sem-panel sem-panel-stack" aria-label="finding">
            <section className="card">
              <p>
                <span className={severityChip(current.severity)}>{current.severity}</span>{" "}
                {current.reason}
              </p>
              <p className="muted">
                {where(current)} · {current.source.extractor} · confidence{" "}
                {current.confidence.toFixed(1)} · fingerprint{" "}
                <code>{current.fingerprint.slice(0, 12)}</code>
              </p>
              <Link to={`../graph?focus=${current.source.object_id}`}>open in the graph</Link>
            </section>
            <ObjectPanel
              key={current.source.object_id}
              workspace={id}
              objectId={current.source.object_id}
              onClose={() => setSelected(null)}
              onGoto={() => {}}
            />
          </aside>
        )}
      </div>
    </div>
  );
}

function Finding({
  d,
  selected,
  onPick,
}: {
  d: XrefDiagnostic;
  selected: boolean;
  onPick: () => void;
}) {
  const label = d.kind === "AmbiguousReference" ? "candidates" : "closest";
  return (
    <li className={selected ? "selected" : ""}>
      <div className="sem-item-head">
        <span className={severityChip(d.severity)}>{d.severity}</span>
        <span className="chip">{KIND_LABEL[d.kind] ?? d.kind}</span>
        <button type="button" className="sem-item-pick" aria-pressed={selected} onClick={onPick}>
          {d.target_spec.written}
        </button>
        {d.cross_system && <span className="chip small">cross-system</span>}
        {d.coverage_gap && <span className="chip small">coverage gap</span>}
      </div>
      <div className="muted sem-item-sum">
        {where(d)} — {d.reason}
        {d.candidates && d.candidates.length > 0 && d.kind !== "ReferenceCycle" && (
          <>
            {" "}
            · {label}: {d.candidates.map((c) => c.name).join(", ")}
          </>
        )}
      </div>
    </li>
  );
}

/** The connectivity metrics (RFC 0233 §6); dynamic share always beside reachability coverage. */
export function MetricsBar({ metrics: m, at }: { metrics: XrefMetrics; at?: string }) {
  const orphans = Object.entries(m.orphans ?? {})
    .map(([k, n]) => `${n} ${k}`)
    .join(", ");
  return (
    <section className="card" aria-label="integrity metrics">
      <section className="tiles">
        <Tile label="resolution" value={pct(m.resolution_rate)} />
        <Tile label="cross-system resolution" value={pct(m.cross_system_resolution_rate)} />
        <Tile label="cycles" value={m.cycles ?? 0} />
        <Tile label="unused tables / views" value={orphans || "none"} />
      </section>
      <ul className="muted">
        {Object.entries(m.reachability ?? {}).map(([family, r]) => (
          <li key={family}>
            <strong>{family}</strong>:{" "}
            {r.supported
              ? `${r.symbols} symbols, ${pct(r.reachability_coverage)} reachable, ${pct(r.dynamic_share)} dynamic (${r.unreachable} unreachable)`
              : `reachability unsupported — ${r.unsupported_because ?? ""}`}
          </li>
        ))}
      </ul>
      {at && <p className="muted">computed at commit {at.slice(0, 19).replace("T", " ")}</p>}
    </section>
  );
}

function Tile({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="tile">
      <div className="tile-value">{typeof value === "number" ? value.toLocaleString() : value}</div>
      <div className="tile-label">{label}</div>
    </div>
  );
}
