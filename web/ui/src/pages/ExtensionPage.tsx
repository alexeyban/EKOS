// RFC 0198: one page an extension declares — panels of its read commands' JSON, and buttons that
// run its commands as jobs through the same route as the Commands tab.

import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate, useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { Me } from "../WorkspaceShell";
import {
  at,
  rowsOf,
  show,
  tone,
  type ExtAction,
  type ExtColumn,
  type ExtPage,
  type ExtPanel,
} from "./extension-view";

interface CliEntry {
  path: string[];
  class: string;
  allowed: boolean;
  confirm: boolean;
}

export const useExtensionPages = () =>
  useQuery({
    queryKey: ["extension-pages"],
    queryFn: () => api<{ pages: ExtPage[] }>("/extensions"),
    staleTime: Infinity,
  });

function Value({ col, value }: { col: ExtColumn; value: unknown }) {
  if (!col.badge) return <>{show(value)}</>;
  if (value === undefined || value === null || value === "") return <>–</>;
  return <span className={`chip small ${tone(value)}`}>{show(value)}</span>;
}

function PanelView({ panel, data }: { panel: ExtPanel; data: unknown }) {
  const v = panel.view;
  if (v.kind === "table") {
    const rows = rowsOf(data, v);
    if (rows.length === 0) return <p className="muted">{panel.empty}</p>;
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
          {rows.map((row, i) => (
            <tr key={i}>
              {v.columns.map((c) => (
                <td key={c.key}>
                  <Value col={c} value={at(row, c.key)} />
                </td>
              ))}
            </tr>
          ))}
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
  const text = at(data, v.pointer);
  return typeof text === "string" && text !== "" ? (
    <pre className="ext-text">{text}</pre>
  ) : (
    <p className="muted">{panel.empty}</p>
  );
}

function PanelCard({ ws, page, panel }: { ws: string; page: ExtPage; panel: ExtPanel }) {
  const q = useQuery({
    queryKey: ["extension-panel", ws, page.extension, page.id, panel.id],
    queryFn: () =>
      api<unknown>(`/workspaces/${ws}/extensions/${page.extension}/${page.id}/${panel.id}`),
    enabled: ws !== "",
  });
  return (
    <section className="card">
      <div className="ext-panel-head">
        <strong>{panel.title}</strong>
        <button className="linkish small" onClick={() => q.refetch()} disabled={q.isFetching}>
          {q.isFetching ? "refreshing…" : "refresh"}
        </button>
      </div>
      {q.isError ? (
        <p className="err">{String(q.error)}</p>
      ) : q.data === undefined ? (
        <p className="muted">loading…</p>
      ) : (
        <PanelView panel={panel} data={q.data} />
      )}
    </section>
  );
}

function ActionButton({ ws, action, entry }: { ws: string; action: ExtAction; entry?: CliEntry }) {
  const navigate = useNavigate();
  const name = action.command.join(" ");
  const [confirm, setConfirm] = useState("");
  const [asking, setAsking] = useState(false);
  const run = useMutation({
    mutationFn: () =>
      apiPost<{ run_id: string }>(`/workspaces/${ws}/cli`, {
        command: action.command,
        args: {},
        confirm: confirm || null,
      }),
    onSuccess: (r) => navigate(`/runs/${r.run_id}`),
  });
  if (!entry) return null;
  const title = entry.allowed ? `ekos ${name} (${entry.class})` : "your role cannot run this here";
  if (asking) {
    return (
      <span className="ext-confirm">
        <input
          aria-label={`type ${name} to confirm`}
          placeholder={`type: ${name}`}
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
        />
        <button disabled={confirm !== name || run.isPending} onClick={() => run.mutate()}>
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
        disabled={!entry.allowed || run.isPending}
        onClick={() => (entry.confirm ? setAsking(true) : run.mutate())}
      >
        {action.label}
      </button>
      {run.isError && <span className="err small"> {String(run.error)}</span>}
    </span>
  );
}

export function ExtensionPage() {
  const { id = "", ext = "", page: pageId = "" } = useParams();
  const me = useOutletContext<Me>();
  const pages = useExtensionPages();
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
  return (
    <div className="ext-page">
      <section className="card">
        <strong>{page.title}</strong>
        {page.description && <p className="muted">{page.description}</p>}
        {page.actions.length > 0 && (
          <div className="ext-actions">
            {page.actions.map((a) => (
              <ActionButton key={a.command.join(" ")} ws={id} action={a} entry={entryFor(a)} />
            ))}
            {me?.role === "read" && (
              <span className="muted small">Running these needs an operator or admin role.</span>
            )}
          </div>
        )}
      </section>
      {page.panels.map((p) => (
        <PanelCard key={p.id} ws={id} page={page} panel={p} />
      ))}
    </div>
  );
}
