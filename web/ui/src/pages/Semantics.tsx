// RFC 0170 — human review of business-semantics hypotheses, and the LinkML schema they export.
//
// Three views on one page (`?view=`): the review queue, the gap report, and the LinkML schema
// viewer/editor. Every decision goes through the console API to the human-only CLI commands; the
// MCP surface can never promote a hypothesis.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams, useSearchParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { ReviewStatus, SemanticsItem, SemanticsKind } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { Linkml } from "./Linkml";
import { SemanticsItemPanel } from "./SemanticsItemPanel";
import {
  KIND_FILTER,
  KIND_LABEL,
  REVIEWABLE,
  displayName,
  loadReviewer,
  reviewOrder,
  saveReviewer,
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
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [bulkDone, setBulkDone] = useState("");
  const items = useItems(id, kind, statusFilter);
  const canWrite = me.role === "write";

  const needle = text.trim().toLowerCase();
  const shown = reviewOrder(items.data ?? []).filter(
    (i) =>
      !needle ||
      displayName(i).toLowerCase().includes(needle) ||
      summary(i).toLowerCase().includes(needle),
  );
  const pickable = shown.filter((i) => REVIEWABLE.includes(i.kind));
  const allPicked = pickable.length > 0 && pickable.every((i) => picked.has(i.id));
  const toggle = (itemId: string) =>
    setPicked((p) => {
      const n = new Set(p);
      if (n.has(itemId)) n.delete(itemId);
      else n.add(itemId);
      return n;
    });
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
        {canWrite && pickable.length > 0 && (
          <label className="sem-pickall">
            <input
              type="checkbox"
              checked={allPicked}
              onChange={() =>
                setPicked(allPicked ? new Set() : new Set(pickable.map((i) => i.id)))
              }
            />
            select all {pickable.length} shown
          </label>
        )}
        {canWrite && picked.size > 0 && (
          <BulkBar
            workspace={id}
            me={me}
            ids={[...picked]}
            onDone={(message) => {
              setPicked(new Set());
              setBulkDone(message);
            }}
          />
        )}
        {bulkDone && picked.size === 0 && <p className="ok-line">{bulkDone}</p>}
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
                {canWrite && REVIEWABLE.includes(i.kind) && (
                  <input
                    type="checkbox"
                    aria-label={`select ${displayName(i)}`}
                    checked={picked.has(i.id)}
                    onClick={(e) => e.stopPropagation()}
                    onChange={() => toggle(i.id)}
                  />
                )}
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

/** Confirm or reject every selected item in one decision — the CLI applies all or none. */
function BulkBar({
  workspace,
  me,
  ids,
  onDone,
}: {
  workspace: string;
  me: Me;
  ids: string[];
  /** Called after a decision (with what happened) or on clear (with ""). The bar unmounts when the
   * selection empties, so the parent shows the message. */
  onDone: (message: string) => void;
}) {
  const qc = useQueryClient();
  const [note, setNote] = useState("");
  const [reviewer, setReviewer] = useState(loadReviewer());
  const decide = useMutation({
    mutationFn: (action: "confirm" | "reject") =>
      apiPost<{ count: number; reviewer: string }>(
        `/workspaces/${workspace}/semantics/review-bulk`,
        { action, ids, note: note || null, reviewer: me.mode === "token" ? reviewer : null },
      ),
    onSuccess: (r, action) => {
      if (me.mode === "token") saveReviewer(reviewer);
      setNote("");
      onDone(`${r.count} item(s) ${action}ed — recorded as ${r.reviewer}.`);
      void qc.invalidateQueries({ queryKey: ["semantics"] });
      void qc.invalidateQueries({ queryKey: ["semantics-gaps"] });
      void qc.invalidateQueries({ queryKey: ["semantics-item"] });
      void qc.invalidateQueries({ queryKey: ["linkml"] });
    },
  });
  const needsName = me.mode === "token" && !reviewer.trim();
  return (
    <div className="sem-bulk" role="region" aria-label="bulk review">
      <strong>{ids.length} selected</strong>
      {me.mode === "token" && (
        <input
          aria-label="bulk reviewer"
          placeholder="reviewer name"
          value={reviewer}
          onChange={(e) => setReviewer(e.target.value)}
        />
      )}
      <input
        aria-label="bulk note"
        placeholder="note (required to reject)"
        value={note}
        onChange={(e) => setNote(e.target.value)}
      />
      <div className="btnrow">
        <button
          className="save"
          disabled={decide.isPending || needsName}
          onClick={() => decide.mutate("confirm")}
        >
          Confirm {ids.length}
        </button>
        <button
          className="danger-btn"
          disabled={decide.isPending || needsName || !note.trim()}
          onClick={() => decide.mutate("reject")}
        >
          Reject {ids.length}
        </button>
        <button className="linkish" onClick={() => onDone("")}>
          clear
        </button>
      </div>
      {decide.isError && <p className="err">{String(decide.error)}</p>}
    </div>
  );
}
