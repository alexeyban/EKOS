// RFC 0198: one page an extension declares — panels of its read commands' JSON, and buttons that
// run its commands as jobs through the same route as the Commands tab. RFC 0207 adds selections
// (a package, then a step) that panels and buttons use, and code / graph / markdown views.

import { useMutation, useQuery } from "@tanstack/react-query";
import { type ReactElement, useState } from "react";
import { useNavigate, useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import { Empty, ErrorNote, PageHead, QueryView, Skeleton } from "../ui";
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
  type ExtView,
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
    <div className="ext-graph x-graph-scroll">
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

type ViewOf<K extends ExtView["kind"]> = Extract<ExtView, { kind: K }>;

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
  const empty = <p className="muted">{panel.empty}</p>;
  switch (v.kind) {
    case "table":
      return <TableView view={v} data={data} empty={empty} sel={selectable[0]} chosen={chosen} onSelect={onSelect} />;
    case "fields":
      return <FieldsView view={v} data={data} empty={empty} />;
    case "code":
      return <CodeView view={v} data={data} empty={empty} />;
    case "graph": {
      const nodes = at(data, v.nodes);
      return Array.isArray(nodes) && nodes.length > 0 ? <GraphView data={data} view={v} /> : empty;
    }
    case "markdown": {
      const text = at(data, v.pointer);
      return typeof text === "string" && text !== "" ? <Markdown text={text} /> : empty;
    }
    default: {
      const text = at(data, v.pointer);
      return typeof text === "string" && text !== "" ? <pre className="ext-text">{text}</pre> : empty;
    }
  }
}

function TableView({
  view,
  data,
  empty,
  sel,
  chosen,
  onSelect,
}: {
  view: ViewOf<"table">;
  data: unknown;
  empty: ReactElement;
  sel: ExtParam | undefined;
  chosen: Chosen;
  onSelect: (param: ExtParam, value: string) => void;
}) {
  const rows = rowsOf(data, view);
  if (rows.length === 0) return empty;
  return (
    <div className="table-scroll">
      <table className="lm-table x-table">
        <thead>
          <tr>
            {view.columns.map((c) => (
              <th key={c.key}>{c.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) =>
            sel ? (
              <SelectableRow key={i} view={view} row={row} sel={sel} chosen={chosen} onSelect={onSelect} />
            ) : (
              <tr key={i}>
                {view.columns.map((c) => (
                  <td key={c.key}>
                    <Value col={c} value={at(row, c.key)} />
                  </td>
                ))}
              </tr>
            ),
          )}
        </tbody>
      </table>
    </div>
  );
}

/** A row that chooses a selection: its first cell is the button (whose ::after covers the row,
 * so a click anywhere on the row still chooses), and the keyboard reaches it too. */
function SelectableRow({
  view,
  row,
  sel,
  chosen,
  onSelect,
}: {
  view: ViewOf<"table">;
  row: unknown;
  sel: ExtParam;
  chosen: Chosen;
  onSelect: (param: ExtParam, value: string) => void;
}) {
  const value = show(at(row, sel.key));
  const on = chosen[sel.name] === value;
  return (
    <tr className={on ? "selectable selected" : "selectable"}>
      {view.columns.map((c, j) => (
        <td key={c.key}>
          {j === 0 ? (
            <button
              type="button"
              className="x-row-pick"
              aria-pressed={on}
              title={`choose this ${sel.label.toLowerCase()}`}
              onClick={() => onSelect(sel, value)}
            >
              <Value col={c} value={at(row, c.key)} />
            </button>
          ) : (
            <Value col={c} value={at(row, c.key)} />
          )}
        </td>
      ))}
    </tr>
  );
}

function FieldsView({ view, data, empty }: { view: ViewOf<"fields">; data: unknown; empty: ReactElement }) {
  const present = view.fields.filter((f) => at(data, f.key) !== undefined);
  if (present.length === 0) return empty;
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

/** A side of a code view: the text, or a dash when there is none. */
function codeText(value: unknown): string {
  return typeof value === "string" && value !== "" ? value : "–";
}

function CodeView({ view, data, empty }: { view: ViewOf<"code">; data: unknown; empty: ReactElement }) {
  const left = at(data, view.left);
  const right = at(data, view.right);
  if (typeof left !== "string" && typeof right !== "string") return empty;
  return (
    <div className="ext-code">
      <div>
        <div className="muted small">{view.left_label}</div>
        <pre className="ext-text">{codeText(left)}</pre>
      </div>
      <div>
        <div className="muted small">{view.right_label}</div>
        <pre className="ext-text">{codeText(right)}</pre>
      </div>
    </div>
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
    <section className="card" aria-label={panel.title}>
      <div className="ext-panel-head">
        <h2 className="card-label">{panel.title}</h2>
        {missing.length === 0 && (
          <button type="button" className="linkish small" onClick={() => q.refetch()} disabled={q.isFetching}>
            {q.isFetching ? "refreshing…" : "refresh"}
          </button>
        )}
      </div>
      {missing.length > 0 ? (
        <p className="muted">Choose a {labels.join(" and a ")} to see this.</p>
      ) : (
        <QueryView q={q} what={panel.title} rows={3}>
          {(data) => (
            <PanelView panel={panel} data={data} selectable={selectable} chosen={chosen} onSelect={onSelect} />
          )}
        </QueryView>
      )}
    </section>
  );
}

/** The empty choice of a selector: nothing to choose from yet, none at all, or a prompt. */
function placeholder(waiting: boolean, count: number): string {
  if (waiting) return "—";
  return count === 0 ? "none" : "choose…";
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
        <option value="">{placeholder(waiting, values.length)}</option>
        {values.map((v) => (
          <option key={v} value={v}>
            {v}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Why an action button is or is not ready, for its tooltip. */
function actionTitle(entry: CliEntry, unbound: boolean, name: string): string {
  if (!entry.allowed) return "your role cannot run this here";
  if (unbound) return "choose what it runs on first";
  return `ekos ${name} (${entry.class})`;
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
  const title = actionTitle(entry, args === null, name);
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
        <button
          type="button"
          className="primary"
          disabled={!typedOk || !consentOk || run.isPending}
          onClick={() => run.mutate()}
        >
          Run
        </button>
        <button type="button" className="linkish small" onClick={() => setAsking(false)}>
          cancel
        </button>
        {run.isError && <span className="err small"> {String(run.error)}</span>}
      </span>
    );
  }
  return (
    <span>
      <button
        type="button"
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
  if (pages.isError) return <ErrorNote error={pages.error} what="extension pages" />;
  if (!pages.data) return <Skeleton rows={4} label="loading extension page" />;
  const page = pages.data.pages.find((p) => p.extension === ext && p.id === pageId);
  if (!page) {
    return (
      <Empty title="No such extension page" icon="extension">
        This console's binary declares no page {ext}/{pageId}.
      </Empty>
    );
  }
  const entryFor = (a: ExtAction) =>
    cli.data?.commands.find((c) => c.path.join(" ") === a.command.join(" "));
  const select = (param: ExtParam, value: string) => {
    setChosen((prev) => {
      const next = { ...prev, [param.name]: value };
      for (const d of dependents(page, param.name)) delete next[d];
      return next;
    });
  };
  const params = page.params ?? [];
  return (
    <div className="ext-page">
      <PageHead title={page.title} sub={page.description || undefined} />
      {(params.length > 0 || page.actions.length > 0) && (
        <section className="card x-ext-controls" aria-label="selections and actions">
          {params.length > 0 && (
            <div className="ext-selectors">
              {params.map((p) => (
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
      )}
      {page.panels.map((p) => (
        <PanelCard key={p.id} ws={id} page={page} panel={p} chosen={chosen} onSelect={select} />
      ))}
    </div>
  );
}
