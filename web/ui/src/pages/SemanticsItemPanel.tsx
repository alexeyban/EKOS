// RFC 0170 — one hypothesis: what it asserts, the evidence it rests on, and the human decision.
//
// Decisions need the write role. Under OIDC the console records the signed-in identity; in token
// mode there is none, so the reviewer types a name and it is recorded as `token:<name>`.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, apiPost } from "../api/client";
import type { SemanticsDetail } from "../api/types";
import type { Me } from "../WorkspaceShell";
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
  const qc = useQueryClient();
  const detail = useQuery({
    queryKey: ["semantics-item", workspace, itemId],
    queryFn: () => api<SemanticsDetail>(`/workspaces/${workspace}/semantics/items/${itemId}`),
  });
  const [note, setNote] = useState("");
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [label, setLabel] = useState("");
  const [reviewer, setReviewer] = useState(loadReviewer());
  const [showRaw, setShowRaw] = useState(false);

  const decide = useMutation({
    mutationFn: (action: Action) =>
      apiPost<{ reviewer: string; message: string }>(
        `/workspaces/${workspace}/semantics/items/${itemId}/review`,
        {
          action,
          note: note || null,
          reviewer: me.mode === "token" ? reviewer : null,
          ...(action === "edit"
            ? { name: name || null, description: description || null, label: label || null }
            : {}),
        },
      ),
    onSuccess: () => {
      if (me.mode === "token") saveReviewer(reviewer);
      setEditing(false);
      setNote("");
      void qc.invalidateQueries({ queryKey: ["semantics"] });
      void qc.invalidateQueries({ queryKey: ["semantics-gaps"] });
      void qc.invalidateQueries({ queryKey: ["semantics-item", workspace, itemId] });
      void qc.invalidateQueries({ queryKey: ["linkml"] });
    },
  });

  const d = detail.data;
  const reviewable = d ? REVIEWABLE.includes(d.kind) : false;
  const canWrite = me.role === "write";
  const needsName = me.mode === "token" && !reviewer.trim();

  const startEdit = () => {
    if (!d) return;
    setName(prop(d, "expert_name"));
    setDescription(prop(d, "expert_description") || prop(d, "description"));
    setLabel(prop(d, "expert_label") || prop(d, "label"));
    setEditing(true);
  };

  return (
    <aside className="card sem-panel">
      <div className="sem-panel-head">
        <button className="linkish" onClick={onClose} aria-label="close">
          ✕ close
        </button>
      </div>
      {detail.isLoading && <p className="muted">loading…</p>}
      {detail.isError && <p className="err">{String(detail.error)}</p>}
      {d && (
        <>
          <div className="sem-item-head">
            <span className="chip">{KIND_LABEL[d.kind]}</span>
            <span className={statusChip(status(d))}>{status(d).replace("_", " ")}</span>
          </div>
          <h3 className="sem-title">{displayName(d)}</h3>
          {prop(d, "expert_name") && <p className="muted">recovered as {d.name}</p>}
          <p className="sem-summary">{summary(d)}</p>

          <dl className="sem-props">
            {SHOWN.filter((k) => prop(d, k) !== "").map((k) => (
              <div key={k}>
                <dt>{k.replace(/_/g, " ")}</dt>
                <dd>{prop(d, k)}</dd>
              </div>
            ))}
          </dl>
          {previousReview(d) && <p className="muted">previously {previousReview(d)}</p>}

          <strong className="sem-sub">Evidence ({d.evidence.length})</strong>
          <ul className="sem-evidence">
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
              <strong className="sem-sub">Links</strong>
              <ul className="sem-evidence">
                {d.links.map((l, n) => (
                  <li key={n}>
                    <span className="chip">{l.kind}</span> <span>{l.to}</span>
                  </li>
                ))}
              </ul>
            </>
          )}

          {reviewable && (
            <div className="sem-decide">
              <strong className="sem-sub">Decision</strong>
              {!canWrite && <p className="muted">Reviewing needs the write role.</p>}
              {canWrite && (
                <>
                  {me.mode === "token" && (
                    <label className="sem-field">
                      Reviewer (token mode has no identity; recorded as token:&lt;name&gt;)
                      <input value={reviewer} onChange={(e) => setReviewer(e.target.value)} />
                    </label>
                  )}
                  {editing && (
                    <>
                      {d.kind === "EnumMeaning" ? (
                        <label className="sem-field">
                          Label (what this code means)
                          <input value={label} onChange={(e) => setLabel(e.target.value)} />
                        </label>
                      ) : (
                        <>
                          <label className="sem-field">
                            Name
                            <input
                              value={name}
                              placeholder={d.name}
                              onChange={(e) => setName(e.target.value)}
                            />
                          </label>
                          <label className="sem-field">
                            Description
                            <textarea
                              rows={3}
                              value={description}
                              onChange={(e) => setDescription(e.target.value)}
                            />
                          </label>
                        </>
                      )}
                    </>
                  )}
                  <label className="sem-field">
                    Note {editing ? "" : "(required to reject)"}
                    <textarea rows={2} value={note} onChange={(e) => setNote(e.target.value)} />
                  </label>
                  <div className="btnrow">
                    {editing ? (
                      <>
                        <button
                          className="save"
                          disabled={decide.isPending || needsName}
                          onClick={() => decide.mutate("edit")}
                        >
                          Save &amp; confirm
                        </button>
                        <button className="linkish" onClick={() => setEditing(false)}>
                          cancel
                        </button>
                      </>
                    ) : (
                      <>
                        <button
                          className="save"
                          disabled={decide.isPending || needsName}
                          onClick={() => decide.mutate("confirm")}
                        >
                          Confirm
                        </button>
                        <button
                          className="danger-btn"
                          disabled={decide.isPending || needsName || !note.trim()}
                          onClick={() => decide.mutate("reject")}
                        >
                          Reject
                        </button>
                        {(d.kind === "BusinessConcept" || d.kind === "EnumMeaning") && (
                          <button disabled={decide.isPending} onClick={startEdit}>
                            Edit…
                          </button>
                        )}
                      </>
                    )}
                  </div>
                  {decide.isError && <p className="err">{String(decide.error)}</p>}
                  {decide.isSuccess && (
                    <p className="ok-line">Recorded as {decide.data.reviewer}.</p>
                  )}
                </>
              )}
            </div>
          )}

          <button className="linkish" onClick={() => setShowRaw(!showRaw)}>
            {showRaw ? "hide" : "show"} all properties
          </button>
          {showRaw && <pre className="sem-raw">{JSON.stringify(d.properties, null, 2)}</pre>}
        </>
      )}
    </aside>
  );
}
