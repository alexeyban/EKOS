// RFC 0170 — one hypothesis: what it asserts, the evidence it rests on, and the human decision.
//
// Decisions need the write role. Under OIDC the console records the signed-in identity; in token
// or local mode there is none, so the reviewer types a name, recorded as `token:<name>` or
// `local:<name>`.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, apiPost } from "../api/client";
import type { SemanticsDetail } from "../api/types";
import { ErrorNote, Skeleton } from "../ui";
import { type Me, mayReview } from "../WorkspaceShell";
import { DetailPanel, ReviewerField, words } from "./decide-kit";
import { UsageOverTime } from "./UsageOverTime";
import {
  KIND_LABEL,
  REVIEWABLE,
  displayName,
  loadReviewer,
  previousReview,
  prop,
  saveReviewer,
  status,
  statusChip,
  summary,
} from "./semantics-shared";

/** Properties worth showing, in reading order; the rest are in the raw view. */
const SHOWN = [
  "definition",
  "table",
  "column",
  "value",
  "label",
  "expert_name",
  "expert_description",
  "expert_label",
  "description",
  "llm_definition",
  "origin",
  "sites",
  "usage_sites",
  "seen_in",
  "constraint_type",
  "expression",
  "confidence",
  "reviewed_by",
  "reviewed_at",
  "review_note",
  "review_reason",
];

type Action = "confirm" | "reject" | "edit";

/** RFC 0252: the column and value whose rows per month the panel charts, if any. */
export function usageTarget(
  d: SemanticsDetail,
): { table: string; column: string; value: string | null } | null {
  const table = prop(d, "table");
  const column = prop(d, "column");
  if (!table || !column) return null;
  if (d.kind === "EnumMeaning") return { table, column, value: prop(d, "value") };
  if (d.kind === "SemanticGap" && prop(d, "gap_type") === "unexplained_value") {
    return { table, column, value: prop(d, "value") };
  }
  if (d.kind === "SemanticGap" && prop(d, "gap_type") === "data_only_values") {
    return { table, column, value: null };
  }
  return null;
}

/** The decision form's state and the call that records it; kept by the panel. */
function useDecision(workspace: string, itemId: string, me: Me) {
  const qc = useQueryClient();
  const [note, setNote] = useState("");
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [label, setLabel] = useState("");
  const [reviewer, setReviewer] = useState(loadReviewer());
  const decide = useMutation({
    mutationFn: (action: Action) =>
      apiPost<{ reviewer: string; message: string }>(
        `/workspaces/${workspace}/semantics/items/${itemId}/review`,
        {
          action,
          note: note || null,
          reviewer: me.mode !== "oidc" ? reviewer : null,
          ...(action === "edit"
            ? { name: name || null, description: description || null, label: label || null }
            : {}),
        },
      ),
    onSuccess: () => {
      if (me.mode !== "oidc") saveReviewer(reviewer);
      setEditing(false);
      setNote("");
      void qc.invalidateQueries({ queryKey: ["semantics"] });
      void qc.invalidateQueries({ queryKey: ["semantics-gaps"] });
      void qc.invalidateQueries({ queryKey: ["semantics-item", workspace, itemId] });
      void qc.invalidateQueries({ queryKey: ["linkml"] });
    },
  });
  const startEdit = (d: SemanticsDetail) => {
    setName(prop(d, "expert_name"));
    setDescription(prop(d, "expert_description") || prop(d, "description"));
    setLabel(prop(d, "expert_label") || prop(d, "label"));
    setEditing(true);
  };
  return {
    note,
    setNote,
    editing,
    setEditing,
    name,
    setName,
    description,
    setDescription,
    label,
    setLabel,
    reviewer,
    setReviewer,
    decide,
    startEdit,
    needsName: me.mode !== "oidc" && !reviewer.trim(),
  };
}

type Decision = ReturnType<typeof useDecision>;

export function SemanticsItemPanel({
  workspace,
  itemId,
  me,
  onClose,
}: {
  workspace: string;
  itemId: string;
  me: Me;
  onClose: () => void;
}) {
  const detail = useQuery({
    queryKey: ["semantics-item", workspace, itemId],
    queryFn: () => api<SemanticsDetail>(`/workspaces/${workspace}/semantics/items/${itemId}`),
  });
  const dec = useDecision(workspace, itemId, me);
  const [showRaw, setShowRaw] = useState(false);
  const d = detail.data;

  return (
    <DetailPanel
      label="hypothesis"
      onClose={onClose}
      title={
        d && (
          <div className="dk-row-head">
            <span className="chip">{KIND_LABEL[d.kind]}</span>
            <span className={statusChip(status(d))}>{words(status(d))}</span>
          </div>
        )
      }
    >
      {detail.isPending && <Skeleton rows={6} label="loading the hypothesis" />}
      {detail.isError && <ErrorNote error={detail.error} />}
      {d && (
        <>
          <ItemFacts workspace={workspace} d={d} />
          {REVIEWABLE.includes(d.kind) && <DecisionForm d={d} me={me} dec={dec} />}
          <button type="button" className="linkish dk-raw-toggle" onClick={() => setShowRaw(!showRaw)}>
            {showRaw ? "hide" : "show"} all properties
          </button>
          {showRaw && <pre className="dk-raw">{JSON.stringify(d.properties, null, 2)}</pre>}
        </>
      )}
    </DetailPanel>
  );
}

/** What the hypothesis asserts, its properties, usage and evidence. */
function ItemFacts({ workspace, d }: { workspace: string; d: SemanticsDetail }) {
  const target = usageTarget(d);
  return (
    <>
      <h3 className="dk-title">{displayName(d)}</h3>
      {prop(d, "expert_name") && <p className="muted">recovered as {d.name}</p>}
      <p className="dk-summary">{summary(d)}</p>

      <dl className="dk-props">
        {SHOWN.filter((k) => prop(d, k) !== "").map((k) => (
          <div key={k}>
            <dt>{k.replaceAll("_", " ")}</dt>
            <dd>{prop(d, k)}</dd>
          </div>
        ))}
      </dl>
      {previousReview(d) && <p className="muted">previously {previousReview(d)}</p>}
      {target && <UsageOverTime workspace={workspace} {...target} />}

      <h4 className="dk-sub">Evidence ({d.evidence.length})</h4>
      <ul className="dk-evidence">
        {d.evidence.map((e, n) => (
          <li key={n}>
            <code className="path">
              {e.path}
              {e.line ? `:${e.line}` : ""}
            </code>
            <span>{e.fragment}</span>
          </li>
        ))}
      </ul>
      {d.links.length > 0 && (
        <>
          <h4 className="dk-sub">Links</h4>
          <ul className="dk-evidence">
            {d.links.map((l, n) => (
              <li key={n}>
                <span>
                  <span className="chip">{l.kind}</span> {l.to}
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
    </>
  );
}

/** The fields an edit changes: a code's label, or a concept's name and description. */
function EditFields({ d, dec }: { d: SemanticsDetail; dec: Decision }) {
  if (d.kind === "EnumMeaning") {
    return (
      <label className="dk-field">
        <span>Label (what this code means)</span>
        <input value={dec.label} onChange={(e) => dec.setLabel(e.target.value)} />
      </label>
    );
  }
  return (
    <>
      <label className="dk-field">
        <span>Name</span>
        <input value={dec.name} placeholder={d.name} onChange={(e) => dec.setName(e.target.value)} />
      </label>
      <label className="dk-field">
        <span>Description</span>
        <textarea
          rows={3}
          value={dec.description}
          onChange={(e) => dec.setDescription(e.target.value)}
        />
      </label>
    </>
  );
}

function DecisionButtons({ d, dec }: { d: SemanticsDetail; dec: Decision }) {
  const busy = dec.decide.isPending;
  if (dec.editing) {
    return (
      <div className="dk-actions">
        <button
          type="button"
          className="primary"
          disabled={busy || dec.needsName}
          onClick={() => dec.decide.mutate("edit")}
        >
          Save &amp; confirm
        </button>
        <button type="button" className="linkish" onClick={() => dec.setEditing(false)}>
          cancel
        </button>
      </div>
    );
  }
  return (
    <div className="dk-actions">
      <button
        type="button"
        className="primary"
        disabled={busy || dec.needsName}
        onClick={() => dec.decide.mutate("confirm")}
      >
        Confirm
      </button>
      <button
        type="button"
        className="dk-danger"
        disabled={busy || dec.needsName || !dec.note.trim()}
        onClick={() => dec.decide.mutate("reject")}
      >
        Reject
      </button>
      {(d.kind === "BusinessConcept" || d.kind === "EnumMeaning") && (
        <button type="button" disabled={busy} onClick={() => dec.startEdit(d)}>
          Edit…
        </button>
      )}
    </div>
  );
}

function DecisionForm({ d, me, dec }: { d: SemanticsDetail; me: Me; dec: Decision }) {
  if (!mayReview(me)) {
    return (
      <section className="dk-section" aria-label="decision">
        <h4 className="dk-sub">Decision</h4>
        <p className="muted">Reviewing needs the write role.</p>
      </section>
    );
  }
  return (
    <section className="dk-section" aria-label="decision">
      <h4 className="dk-sub">Decision</h4>
      {me.mode !== "oidc" && <ReviewerField value={dec.reviewer} onChange={dec.setReviewer} />}
      {dec.editing && <EditFields d={d} dec={dec} />}
      <label className="dk-field">
        Note {dec.editing ? "" : "(required to reject)"}
        <textarea rows={2} value={dec.note} onChange={(e) => dec.setNote(e.target.value)} />
      </label>
      <DecisionButtons d={d} dec={dec} />
      {dec.decide.isError && <ErrorNote error={dec.decide.error} />}
      {dec.decide.isSuccess && <p className="ok-line">Recorded as {dec.decide.data.reviewer}.</p>}
    </section>
  );
}
