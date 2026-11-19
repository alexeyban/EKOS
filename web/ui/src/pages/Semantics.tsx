// RFC 0170 — human review of business-semantics hypotheses, and the LinkML schema they export.
//
// Four views on one page (`?view=`): the review queue, the gap report, the LinkML schema
// viewer/editor and the Apache Ossie model. Every decision goes through the console API to the
// human-only CLI commands; the MCP surface can never promote a hypothesis.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams, useSearchParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { ReviewStatus, SemanticsItem, SemanticsKind } from "../api/types";
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";
import { type Me, mayReview } from "../WorkspaceShell";
import { PickRow, SplitView, words } from "./decide-kit";
import { Linkml } from "./Linkml";
import { Ossie } from "./Ossie";
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
  { key: "ossie", label: "Apache Ossie" },
] as const;

export function Semantics() {
  const me = useOutletContext<Me>();
  const [params, setParams] = useSearchParams();
  const view = params.get("view") ?? "review";

  return (
    <div className="dk-page">
      <PageHead
        title="Semantics"
        sub="Business meanings recovered from code traces (RFC 0170): what a table, a code or a rule stands for. They stay hypotheses until a person confirms one."
      />
      <div className="dk-tabs" role="group" aria-label="views">
        {VIEWS.map((v) => (
          <button
            key={v.key}
            type="button"
            aria-pressed={view === v.key}
            onClick={() => setParams({ view: v.key })}
          >
            {v.label}
          </button>
        ))}
      </div>
      {view === "review" && <Review me={me} />}
      {view === "gaps" && <Gaps me={me} />}
      {view === "linkml" && <Linkml me={me} />}
      {view === "ossie" && <Ossie me={me} />}
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

const STATUSES: ReviewStatus[] = ["needs_review", "hypothesis", "confirmed", "rejected"];

function ReviewFilters({
  kind,
  setKind,
  statusFilter,
  setStatus,
  text,
  setText,
}: {
  kind: string;
  setKind: (v: string) => void;
  statusFilter: string;
  setStatus: (v: string) => void;
  text: string;
  setText: (v: string) => void;
}) {
  return (
    <div className="dk-toolbar">
      <select aria-label="kind" value={kind} onChange={(e) => setKind(e.target.value)}>
        {(Object.keys(KIND_FILTER) as SemanticsKind[]).map((k) => (
          <option key={k} value={KIND_FILTER[k]}>
            {KIND_LABEL[k]}s
          </option>
        ))}
        <option value="">everything</option>
      </select>
      <select aria-label="status" value={statusFilter} onChange={(e) => setStatus(e.target.value)}>
        <option value="">any status</option>
        {STATUSES.map((s) => (
          <option key={s} value={s}>
            {words(s)}
          </option>
        ))}
      </select>
      <input
        className="dk-search"
        aria-label="search"
        placeholder="search name or definition"
        value={text}
        onChange={(e) => setText(e.target.value)}
      />
    </div>
  );
}

/** "12 item(s)" and a chip per status. */
function CountLine({ items }: { items: SemanticsItem[] }) {
  const counts = items.reduce<Record<string, number>>((m, i) => {
    m[status(i)] = (m[status(i)] ?? 0) + 1;
    return m;
  }, {});
  return (
    <div className="dk-count">
      <span>{items.length} item(s)</span>
      {Object.entries(counts).map(([s, n]) => (
        <span key={s} className={statusChip(s)}>
          {n} {words(s)}
        </span>
      ))}
    </div>
  );
}

function matches(i: SemanticsItem, needle: string): boolean {
  return (
    !needle ||
    displayName(i).toLowerCase().includes(needle) ||
    summary(i).toLowerCase().includes(needle)
  );
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
  const canWrite = mayReview(me);

  const needle = text.trim().toLowerCase();
  const shown = reviewOrder(items.data ?? []).filter((i) => matches(i, needle));
  const pickable = shown.filter((i) => REVIEWABLE.includes(i.kind));
  const allPicked = pickable.length > 0 && pickable.every((i) => picked.has(i.id));
  const toggle = (itemId: string) =>
    setPicked((p) => {
      const n = new Set(p);
      if (n.has(itemId)) n.delete(itemId);
      else n.add(itemId);
      return n;
    });

  return (
    <SplitView
      panel={
        selected && (
          <SemanticsItemPanel
            workspace={id}
            itemId={selected}
            me={me}
            onClose={() => setSelected(null)}
          />
        )
      }
    >
      <section className="card dk-list" aria-label="hypotheses">
        <ReviewFilters
          kind={kind}
          setKind={setKind}
          statusFilter={statusFilter}
          setStatus={setStatus}
          text={text}
          setText={setText}
        />
        <CountLine items={items.data ?? []} />
        {canWrite && pickable.length > 0 && (
          <label className="dk-pickall">
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
        <QueryView
          q={items}
          what="semantics items"
          rows={6}
          isEmpty={(d) => d.length === 0}
          empty={
            <Empty title="Nothing to review here" icon="semantics">
              <p>
                Try another kind or status. If nothing was ever synthesized, set{" "}
                <code>[semantics] enabled = true</code> in ekos.toml and run the pipeline through{" "}
                <code>ekos commit</code>.
              </p>
            </Empty>
          }
        >
          {() =>
            shown.length === 0 ? (
              <p className="muted">No item matches the search.</p>
            ) : (
              <ul className="dk-rows">
                {shown.map((i) => (
                  <PickRow
                    key={i.id}
                    selected={selected === i.id}
                    onPick={() => setSelected(i.id)}
                    title={displayName(i)}
                    lead={
                      canWrite &&
                      REVIEWABLE.includes(i.kind) && (
                        <input
                          type="checkbox"
                          aria-label={`select ${displayName(i)}`}
                          checked={picked.has(i.id)}
                          onChange={() => toggle(i.id)}
                        />
                      )
                    }
                    before={<span className="chip">{KIND_LABEL[i.kind]}</span>}
                    after={<span className={statusChip(status(i))}>{words(status(i))}</span>}
                    sub={summary(i)}
                  />
                ))}
              </ul>
            )
          }
        </QueryView>
      </section>
    </SplitView>
  );
}

/** Which heading a gap is listed under. */
function gapGroup(g: SemanticsItem): string {
  if (g.kind === "ConceptConflict") return "conflicting definitions";
  if (status(g) === "needs_review" && g.kind !== "SemanticGap") {
    return "needs review — the evidence changed or vanished";
  }
  return words(String(g.properties["gap_type"] ?? "gap"));
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
    (m[gapGroup(g)] ??= []).push(g);
    return m;
  }, {});

  return (
    <SplitView
      panel={
        selected && (
          <SemanticsItemPanel
            workspace={id}
            itemId={selected}
            me={me}
            onClose={() => setSelected(null)}
          />
        )
      }
    >
      <section className="card dk-list" aria-label="open questions">
        <h2 className="card-label">Open questions</h2>
        <p className="muted">
          No trace, no recovery: these are the places where the code depends on a meaning no
          source states. Confirm a gap if it is a real unknown, reject it if it is not.
        </p>
        <QueryView
          q={gaps}
          what="semantic gaps"
          rows={6}
          isEmpty={(d) => d.length === 0}
          empty={
            <Empty title="No open questions" icon="check">
              Every meaning the code depends on is stated by a source.
            </Empty>
          }
        >
          {() =>
            Object.entries(groups).map(([title, list]) => (
              <div key={title}>
                <h3 className="dk-group">
                  {title} <span className="muted">({list.length})</span>
                </h3>
                <ul className="dk-rows">
                  {list.map((g) => (
                    <PickRow
                      key={g.id}
                      selected={selected === g.id}
                      onPick={() => setSelected(g.id)}
                      title={displayName(g)}
                      after={<span className={statusChip(status(g))}>{words(status(g))}</span>}
                      sub={String(g.properties["question"] ?? g.properties["review_reason"] ?? "")}
                    />
                  ))}
                </ul>
              </div>
            ))
          }
        </QueryView>
      </section>
    </SplitView>
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
        { action, ids, note: note || null, reviewer: me.mode !== "oidc" ? reviewer : null },
      ),
    onSuccess: (r, action) => {
      if (me.mode !== "oidc") saveReviewer(reviewer);
      setNote("");
      onDone(`${r.count} item(s) ${action}ed — recorded as ${r.reviewer}.`);
      void qc.invalidateQueries({ queryKey: ["semantics"] });
      void qc.invalidateQueries({ queryKey: ["semantics-gaps"] });
      void qc.invalidateQueries({ queryKey: ["semantics-item"] });
      void qc.invalidateQueries({ queryKey: ["linkml"] });
    },
  });
  const needsName = me.mode !== "oidc" && !reviewer.trim();
  return (
    <div className="dk-bulk" role="region" aria-label="bulk review">
      <strong>{ids.length} selected</strong>
      {me.mode !== "oidc" && (
        <input
          aria-label="bulk reviewer"
          placeholder="reviewer name (recorded as token: or local:)"
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
      <div className="dk-actions">
        <button
          type="button"
          className="primary"
          disabled={decide.isPending || needsName}
          onClick={() => decide.mutate("confirm")}
        >
          Confirm {ids.length}
        </button>
        <button
          type="button"
          className="dk-danger"
          disabled={decide.isPending || needsName || !note.trim()}
          onClick={() => decide.mutate("reject")}
        >
          Reject {ids.length}
        </button>
        <button type="button" className="linkish" onClick={() => onDone("")}>
          clear
        </button>
      </div>
      {decide.isError && <ErrorNote error={decide.error} />}
    </div>
  );
}
