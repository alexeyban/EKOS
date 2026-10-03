// RFC 0170 — human review of business-semantics hypotheses, and the LinkML schema they export.
//
// Three views on one page (`?view=`): the review queue, the gap report, and the LinkML schema
// viewer/editor. Every decision goes through the console API to the human-only CLI commands; the
// MCP surface can never promote a hypothesis.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams, useSearchParams } from "react-router-dom";
import { api } from "../api/client";
import type { ReviewStatus, SemanticsItem, SemanticsKind } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { Linkml } from "./Linkml";
import { SemanticsItemPanel } from "./SemanticsItemPanel";
import {
  KIND_FILTER,
  KIND_LABEL,
  displayName,
  reviewOrder,
  status,
  statusChip,
  summary,
} from "./semantics-shared";

const VIEWS = [
  { key: "review", label: "Review" },
  { key: "gaps", label: "Gaps & conflicts" },
  { key: "linkml", label: "LinkML schema" },
] as const;

export function Semantics() {
  const me = useOutletContext<Me>();
  const [params, setParams] = useSearchParams();
  const view = params.get("view") ?? "review";

  return (
    <div className="sem-page">
      <div className="sem-views">
        {VIEWS.map((v) => (
          <button
            key={v.key}
            className={view === v.key ? "pill active" : "pill"}
            onClick={() => setParams({ view: v.key })}
          >
            {v.label}
          </button>
        ))}
        <span className="muted sem-note">
          Hypotheses recovered from code traces (RFC 0170). Only a person confirms one.
        </span>
      </div>
      {view === "review" && <Review me={me} />}
      {view === "gaps" && <Gaps me={me} />}
      {view === "linkml" && <Linkml me={me} />}
    </div>
  );
}

function useItems(id: string, kind: string, statusFilter: string) {
  const q = new URLSearchParams();
  if (kind) q.set("kind", kind);
  if (statusFilter) q.set("status", statusFilter);
  return useQuery({
    queryKey: ["semantics", id, kind, statusFilter],
    queryFn: () => api<SemanticsItem[]>(`/workspaces/${id}/semantics/items?${q}`),
    enabled: id !== "",
  });
}

function Review({ me }: { me: Me }) {
  const { id = "" } = useParams();
  const [kind, setKind] = useState("concept");
  const [statusFilter, setStatus] = useState("");
  const [text, setText] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const items = useItems(id, kind, statusFilter);

  const needle = text.trim().toLowerCase();
  const shown = reviewOrder(items.data ?? []).filter(
    (i) =>
      !needle ||
      displayName(i).toLowerCase().includes(needle) ||
      summary(i).toLowerCase().includes(needle),
  );
  const counts = (items.data ?? []).reduce<Record<string, number>>((m, i) => {
    m[status(i)] = (m[status(i)] ?? 0) + 1;
    return m;
  }, {});

  return (
    <div className={selected ? "sem-layout with-panel" : "sem-layout"}>
      <section className="card sem-list">
        <div className="sem-filters">
          <select aria-label="kind" value={kind} onChange={(e) => setKind(e.target.value)}>
            {(Object.keys(KIND_FILTER) as SemanticsKind[]).map((k) => (
              <option key={k} value={KIND_FILTER[k]}>
                {KIND_LABEL[k]}s
              </option>
            ))}
            <option value="">everything</option>
          </select>
          <select
            aria-label="status"
            value={statusFilter}
            onChange={(e) => setStatus(e.target.value)}
          >
            <option value="">any status</option>
            {(["needs_review", "hypothesis", "confirmed", "rejected"] as ReviewStatus[]).map(
              (s) => (
                <option key={s} value={s}>
                  {s.replace("_", " ")}
                </option>
              ),
            )}
          </select>
          <input
            aria-label="search"
            placeholder="search name or definition"
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </div>
        <p className="muted">
          {items.data?.length ?? 0} item(s)
          {Object.entries(counts).map(([s, n]) => (
            <span key={s} className={statusChip(s)} style={{ marginLeft: "0.4rem" }}>
              {n} {s.replace("_", " ")}
            </span>
          ))}
        </p>
        {items.isLoading && <p className="muted">loading…</p>}
        {items.isError && <p className="err">{String(items.error)}</p>}
        {items.data?.length === 0 && (
          <p className="muted">
            Nothing here. Business semantics needs <code>[semantics] enabled = true</code> in
            ekos.toml and an <code>ekos commit</code>.
          </p>
        )}
        <ul className="sem-items">
          {shown.map((i) => (
            <li
              key={i.id}
              className={selected === i.id ? "selected" : ""}
              onClick={() => setSelected(i.id)}
            >
              <div className="sem-item-head">
                <span className="chip">{KIND_LABEL[i.kind]}</span>
                <strong>{displayName(i)}</strong>
                <span className={statusChip(status(i))}>{status(i).replace("_", " ")}</span>
              </div>
              <div className="muted sem-item-sum">{summary(i)}</div>
            </li>
          ))}
        </ul>
      </section>
      {selected && (
        <SemanticsItemPanel
          workspace={id}
          itemId={selected}
          me={me}
          onClose={() => setSelected(null)}
        />
      )}
    </div>
  );
}

function Gaps({ me }: { me: Me }) {
  const { id = "" } = useParams();
  const [selected, setSelected] = useState<string | null>(null);
  const gaps = useQuery({
    queryKey: ["semantics-gaps", id],
    queryFn: () => api<SemanticsItem[]>(`/workspaces/${id}/semantics/gaps`),
    enabled: id !== "",
  });
  const groups = (gaps.data ?? []).reduce<Record<string, SemanticsItem[]>>((m, g) => {
    const key =
      status(g) === "needs_review" && g.kind !== "SemanticGap" && g.kind !== "ConceptConflict"
        ? "needs review — the evidence changed or vanished"
        : g.kind === "ConceptConflict"
          ? "conflicting definitions"
          : String(g.properties["gap_type"] ?? "gap").replace("_", " ");
    (m[key] ??= []).push(g);
    return m;
  }, {});

  return (
    <div className={selected ? "sem-layout with-panel" : "sem-layout"}>
      <section className="card sem-list">
        <strong>Open questions</strong>
        <p className="muted">
          No trace, no recovery: these are the places where the code depends on a meaning no
          source states. Confirm a gap if it is a real unknown, reject it if it is not.
        </p>
        {gaps.isError && <p className="err">{String(gaps.error)}</p>}
        {gaps.data?.length === 0 && <p className="muted">no open questions</p>}
        {Object.entries(groups).map(([title, list]) => (
          <div key={title}>
            <h4 className="sem-group">
              {title} <span className="muted">({list.length})</span>
            </h4>
            <ul className="sem-items">
              {list.map((g) => (
                <li
                  key={g.id}
                  className={selected === g.id ? "selected" : ""}
                  onClick={() => setSelected(g.id)}
                >
                  <div className="sem-item-head">
                    <strong>{displayName(g)}</strong>
                    <span className={statusChip(status(g))}>{status(g).replace("_", " ")}</span>
                  </div>
                  <div className="muted sem-item-sum">
                    {String(g.properties["question"] ?? g.properties["review_reason"] ?? "")}
                  </div>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </section>
      {selected && (
        <SemanticsItemPanel
          workspace={id}
          itemId={selected}
          me={me}
          onClose={() => setSelected(null)}
        />
      )}
    </div>
  );
}
