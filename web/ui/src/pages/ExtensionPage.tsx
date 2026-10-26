// RFC 0198: one page an extension declares — panels of its read commands' JSON, and buttons that
// run its commands as jobs through the same route as the Commands tab. RFC 0207 adds selections
// (a package, then a step) that panels and buttons use, and code / graph / markdown views.

import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate, useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { Me } from "../WorkspaceShell";
import {
  actionArgs,
  at,
  dependents,
  layoutGraph,
  missingParams,
  rowsOf,
  selectionQuery,
  show,
  tone,
  type ExtAction,
  type ExtColumn,
  type ExtPage,
  type ExtPanel,
  type ExtParam,
  withInputs,
} from "./extension-view";
import { Markdown } from "./markdown";

interface CliEntry {
  path: string[];
  class: string;
  allowed: boolean;
  confirm: boolean;
}

type Chosen = Record<string, string>;

export const useExtensionPages = () =>
  useQuery({
    queryKey: ["extension-pages"],
    queryFn: () => api<{ pages: ExtPage[] }>("/extensions"),
    staleTime: Infinity,
  });

/** One panel's data, shared by the panel and by the selectors it feeds. */
function usePanelData(ws: string, page: ExtPage, panel: ExtPanel | undefined, chosen: Chosen) {
  const missing = panel ? missingParams(panel, chosen) : ["panel"];
  const query = panel ? selectionQuery(panel, chosen) : "";
  return useQuery({
    queryKey: ["extension-panel", ws, page.extension, page.id, panel?.id, query],
    queryFn: () =>
      api<unknown>(`/workspaces/${ws}/extensions/${page.extension}/${page.id}/${panel?.id}${query}`),
    enabled: ws !== "" && panel !== undefined && missing.length === 0,
  });
}

function Value({ col, value }: { col: ExtColumn; value: unknown }) {
  if (!col.badge) return <>{show(value)}</>;
  if (value === undefined || value === null || value === "") return <>–</>;
  return <span className={`chip small ${tone(value)}`}>{show(value)}</span>;
}

function GraphView({ data, view }: { data: unknown; view: Extract<ExtPanel["view"], { kind: "graph" }> }) {
  const rawNodes = at(data, view.nodes);
  const rawEdges = at(data, view.edges);
  if (!Array.isArray(rawNodes) || rawNodes.length === 0) return null;
  const nodes = rawNodes.map((n) => ({
    id: show(at(n, view.id)),
    label: show(at(n, view.label)),
    group: show(at(n, view.group)),
    badge: at(n, view.badge),
  }));
  const edges = (Array.isArray(rawEdges) ? rawEdges : []).map((e) => ({
    from: show(at(e, view.from)),
    to: show(at(e, view.to)),
  }));
  const g = layoutGraph(nodes, edges);
  const w = 160;
  const h = 28;
  const pos = new Map(g.nodes.map((n) => [n.id, n]));
  const lanes = new Map<string, number>();
  for (const n of g.nodes) lanes.set(n.group, Math.min(lanes.get(n.group) ?? n.y, n.y));
  return (
    <div className="ext-graph" style={{ overflowX: "auto" }}>
      <svg width={g.width + 20} height={g.height + 36} role="img" aria-label="graph">
        <defs>
          <marker id="ext-arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M0,0 L10,5 L0,10 z" fill="currentColor" />
          </marker>
        </defs>
        <g transform="translate(10,24)">
          {[...lanes].map(([group, y]) => (
            <text key={group} x={0} y={y - 6} className="ext-lane">
              {group}
            </text>
          ))}
          {g.edges.map((e, i) => {
            const a = pos.get(e.from);
            const b = pos.get(e.to);
            if (!a || !b) return null;
            return (
              <line
                key={i}
                x1={a.x + w}
                y1={a.y + h / 2}
                x2={b.x}
                y2={b.y + h / 2}
                stroke="currentColor"
                opacity={0.4}
                markerEnd="url(#ext-arrow)"
              />
            );
          })}
          {g.nodes.map((n) => (
            <g key={n.id} transform={`translate(${n.x},${n.y})`} className={`ext-node ${tone(n.badge)}`}>
              <rect width={w} height={h} rx={5} />
              <text x={8} y={18}>
                {n.label.length > 22 ? `${n.label.slice(0, 21)}…` : n.label}
              </text>
              <title>{`${n.group}: ${n.label} (${show(n.badge)})`}</title>
            </g>
          ))}
        </g>
      </svg>
    </div>
  );
}

function PanelView({
  panel,
  data,
  selectable,
  chosen,
  onSelect,
}: {
  panel: ExtPanel;
  data: unknown;
  selectable: ExtParam[];
  chosen: Chosen;
  onSelect: (param: ExtParam, value: string) => void;
}) {
  const v = panel.view;
  if (v.kind === "table") {
    const rows = rowsOf(data, v);
    if (rows.length === 0) return <p className="muted">{panel.empty}</p>;
    const sel = selectable[0];
    return (
      <table className="lm-table">
        <thead>
          <tr>
            {v.columns.map((c) => (
              <th key={c.key}>{c.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => {
            const value = sel ? show(at(row, sel.key)) : "";
            const active = sel !== undefined && chosen[sel.name] === value;
            return (
              <tr
                key={i}
                className={sel ? `selectable${active ? " selected" : ""}` : undefined}
                onClick={sel ? () => onSelect(sel, value) : undefined}
                title={sel ? `choose this ${sel.label.toLowerCase()}` : undefined}
              >
                {v.columns.map((c) => (
                  <td key={c.key}>
                    <Value col={c} value={at(row, c.key)} />
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
    );
  }
  if (v.kind === "fields") {
    const present = v.fields.filter((f) => at(data, f.key) !== undefined);
    if (present.length === 0) return <p className="muted">{panel.empty}</p>;
    return (
      <dl className="ext-fields">
        {present.map((f) => (
          <div key={f.key}>
            <dt className="muted small">{f.label}</dt>
            <dd>
              <Value col={f} value={at(data, f.key)} />
            </dd>
          </div>
        ))}
      </dl>
    );
  }
  if (v.kind === "code") {
    const left = at(data, v.left);
    const right = at(data, v.right);
    if (typeof left !== "string" && typeof right !== "string") return <p className="muted">{panel.empty}</p>;
    return (
      <div className="ext-code">
        <div>
          <div className="muted small">{v.left_label}</div>
          <pre className="ext-text">{typeof left === "string" && left !== "" ? left : "–"}</pre>
        </div>
        <div>
          <div className="muted small">{v.right_label}</div>
          <pre className="ext-text">{typeof right === "string" && right !== "" ? right : "–"}</pre>
        </div>
      </div>
    );
  }
  if (v.kind === "graph") {
    const nodes = at(data, v.nodes);
    if (!Array.isArray(nodes) || nodes.length === 0) return <p className="muted">{panel.empty}</p>;
    return <GraphView data={data} view={v} />;
  }
  if (v.kind === "markdown") {
    const text = at(data, v.pointer);
    return typeof text === "string" && text !== "" ? <Markdown text={text} /> : <p className="muted">{panel.empty}</p>;
  }
  const text = at(data, v.pointer);
  return typeof text === "string" && text !== "" ? (
    <pre className="ext-text">{text}</pre>
  ) : (
    <p className="muted">{panel.empty}</p>
  );
}

function PanelCard({
  ws,
  page,
  panel,
  chosen,
  onSelect,
}: {
  ws: string;
  page: ExtPage;
  panel: ExtPanel;
  chosen: Chosen;
  onSelect: (param: ExtParam, value: string) => void;
}) {
  const q = usePanelData(ws, page, panel, chosen);
  const missing = missingParams(panel, chosen);
  const labels = (page.params ?? []).filter((p) => missing.includes(p.name)).map((p) => p.label.toLowerCase());
  const selectable = (page.params ?? []).filter((p) => p.source === panel.id);
  return (
    <section className="card">
      <div className="ext-panel-head">
        <strong>{panel.title}</strong>
        {missing.length === 0 && (
          <button className="linkish small" onClick={() => q.refetch()} disabled={q.isFetching}>
            {q.isFetching ? "refreshing…" : "refresh"}
          </button>
        )}
      </div>
      {missing.length > 0 ? (
        <p className="muted">Choose a {labels.join(" and a ")} to see this.</p>
      ) : q.isError ? (
        <p className="err">{String(q.error)}</p>
      ) : q.data === undefined ? (
        <p className="muted">loading…</p>
      ) : (
        <PanelView panel={panel} data={q.data} selectable={selectable} chosen={chosen} onSelect={onSelect} />
      )}
    </section>
  );
}

function Selector({
  ws,
  page,
  param,
  chosen,
  onSelect,
}: {
  ws: string;
  page: ExtPage;
  param: ExtParam;
  chosen: Chosen;
  onSelect: (param: ExtParam, value: string) => void;
}) {
  const source = page.panels.find((p) => p.id === param.source);
  const q = usePanelData(ws, page, source, chosen);
  const rows = at(q.data, param.rows);
  const values = Array.isArray(rows) ? [...new Set(rows.map((r) => show(at(r, param.key))))] : [];
  const waiting = source ? missingParams(source, chosen).length > 0 : true;
  return (
    <label className="ext-selector">
      <span className="muted small">{param.label}</span>
      <select
        aria-label={param.label}
        value={chosen[param.name] ?? ""}
        disabled={waiting || values.length === 0}
        onChange={(e) => onSelect(param, e.target.value)}
      >
        <option value="">{waiting ? "—" : values.length === 0 ? "none" : "choose…"}</option>
        {values.map((v) => (
          <option key={v} value={v}>
            {v}
          </option>
        ))}
      </select>
    </label>
  );
}

function ActionButton({
  ws,
  action,
  entry,
  chosen,
}: {
  ws: string;
  action: ExtAction;
  entry?: CliEntry;
  chosen: Chosen;
}) {
  const navigate = useNavigate();
  const name = action.command.join(" ");
  const [confirm, setConfirm] = useState("");
  const [asking, setAsking] = useState(false);
  const [consented, setConsented] = useState(false);
  const [typed, setTyped] = useState<Record<string, string>>({});
  const args = actionArgs(action, chosen);
  const run = useMutation({
    mutationFn: () =>
      apiPost<{ run_id: string }>(`/workspaces/${ws}/cli`, {
        command: action.command,
        args: withInputs(action, args ?? {}, typed),
        confirm: confirm || null,
      }),
    onSuccess: (r) => navigate(`/runs/${r.run_id}`),
  });
  if (!entry) return null;
  const bound = (action.binds ?? []).map(([, p]) => chosen[p]).filter(Boolean);
  const label = bound.length > 0 ? `${action.label}: ${bound.join(", ")}` : action.label;
  const title = !entry.allowed
    ? "your role cannot run this here"
    : args === null
      ? "choose what it runs on first"
      : `ekos ${name} (${entry.class})`;
  const inputs = action.inputs ?? [];
  const needsAsk = entry.confirm || Boolean(action.confirm) || inputs.length > 0;
  if (asking) {
    const typedOk = !entry.confirm || confirm === name;
    const consentOk = !action.confirm || consented;
    return (
      <span className="ext-confirm">
        {inputs.map(([arg, label]) => (
          <input
            key={arg}
            aria-label={label}
            placeholder={label}
            maxLength={512}
            value={typed[arg] ?? ""}
            onChange={(e) => setTyped({ ...typed, [arg]: e.target.value })}
          />
        ))}
        {action.confirm && (
          <label className="small">
            <input type="checkbox" checked={consented} onChange={(e) => setConsented(e.target.checked)} />{" "}
            {action.confirm}
          </label>
        )}
        {entry.confirm && (
          <input
            aria-label={`type ${name} to confirm`}
            placeholder={`type: ${name}`}
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
          />
        )}
        <button disabled={!typedOk || !consentOk || run.isPending} onClick={() => run.mutate()}>
          Run
        </button>
        <button className="linkish small" onClick={() => setAsking(false)}>
          cancel
        </button>
        {run.isError && <span className="err small"> {String(run.error)}</span>}
      </span>
    );
  }
  return (
    <span>
      <button
        title={title}
        disabled={!entry.allowed || args === null || run.isPending}
        onClick={() => (needsAsk ? setAsking(true) : run.mutate())}
      >
        {label}
      </button>
      {run.isError && <span className="err small"> {String(run.error)}</span>}
    </span>
  );
}

export function ExtensionPage() {
  const { id = "", ext = "", page: pageId = "" } = useParams();
  const me = useOutletContext<Me>();
  const pages = useExtensionPages();
  const [chosen, setChosen] = useState<Chosen>({});
  const cli = useQuery({
    queryKey: ["cli", id],
    queryFn: () => api<{ commands: CliEntry[] }>(`/workspaces/${id}/cli`),
    enabled: id !== "",
  });
  if (pages.isError) return <p className="err">{String(pages.error)}</p>;
  if (!pages.data) return <p className="muted">loading…</p>;
  const page = pages.data.pages.find((p) => p.extension === ext && p.id === pageId);
  if (!page) return <p className="muted">This console's binary declares no page {ext}/{pageId}.</p>;
  const entryFor = (a: ExtAction) =>
    cli.data?.commands.find((c) => c.path.join(" ") === a.command.join(" "));
  const select = (param: ExtParam, value: string) => {
    setChosen((prev) => {
      const next = { ...prev, [param.name]: value };
      for (const d of dependents(page, param.name)) delete next[d];
      return next;
    });
  };
  return (
    <div className="ext-page">
      <section className="card">
        <strong>{page.title}</strong>
        {page.description && <p className="muted">{page.description}</p>}
        {(page.params ?? []).length > 0 && (
          <div className="ext-selectors">
            {(page.params ?? []).map((p) => (
              <Selector key={p.name} ws={id} page={page} param={p} chosen={chosen} onSelect={select} />
            ))}
          </div>
        )}
        {page.actions.length > 0 && (
          <div className="ext-actions">
            {page.actions.map((a) => (
              <ActionButton key={a.label} ws={id} action={a} entry={entryFor(a)} chosen={chosen} />
            ))}
            {me?.role === "read" && (
              <span className="muted small">Running these needs an operator or admin role.</span>
            )}
          </div>
        )}
      </section>
      {page.panels.map((p) => (
        <PanelCard key={p.id} ws={id} page={page} panel={p} chosen={chosen} onSelect={select} />
      ))}
    </div>
  );
}
