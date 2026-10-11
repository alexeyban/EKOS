// RFC 0172/0174 — where sources disagree about one fact, and a person's decision.
//
// A decision goes through the console API to the human-only `ekos conflicts resolve`; the MCP
// surface can list conflicts but never settle one.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { Conflict } from "../api/types";
import type { Me } from "../WorkspaceShell";
import {
  TYPE_LABEL,
  claimValue,
  claimWhere,
  conflictChip,
  conflictOrder,
} from "./conflicts-shared";
import { loadReviewer, saveReviewer } from "./semantics-shared";

export function Conflicts() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const [statusFilter, setStatus] = useState("open");
  const [selected, setSelected] = useState<string | null>(null);
  const list = useQuery({
    queryKey: ["conflicts", id, statusFilter],
    queryFn: () =>
      api<Conflict[]>(
        `/workspaces/${id}/conflicts${statusFilter ? `?status=${statusFilter}` : ""}`,
      ),
    enabled: id !== "",
  });
  const shown = conflictOrder(list.data ?? []);
  const current = shown.find((c) => c.id === selected) ?? null;

  return (
    <div className="sem-page">
      <div className="sem-views">
        <select aria-label="status" value={statusFilter} onChange={(e) => setStatus(e.target.value)}>
          <option value="open">open</option>
          <option value="resolved">resolved</option>
          <option value="dismissed">dismissed (both valid)</option>
          <option value="">any status</option>
        </select>
        <span className="muted sem-note">
          Two sources state one fact differently (RFC 0172). EKOS keeps both claims; only a person
          decides.
        </span>
      </div>
      <div className={current ? "sem-layout with-panel" : "sem-layout"}>
        <section className="card sem-list">
          <p className="muted">{shown.length} conflict(s)</p>
          {list.isLoading && <p className="muted">loading…</p>}
          {list.isError && <p className="err">{String(list.error)}</p>}
          {list.data?.length === 0 && (
            <p className="muted">
              {statusFilter === "open"
                ? "No open conflicts: no source disagrees with another about the same fact."
                : "Nothing with this status."}
            </p>
          )}
          <ul className="sem-items">
            {shown.map((c) => (
              <li
                key={c.id}
                className={selected === c.id ? "selected" : ""}
                onClick={() => setSelected(c.id)}
              >
                <div className="sem-item-head">
                  <span className="chip">{TYPE_LABEL[c.conflict_type] ?? c.conflict_type}</span>
                  <strong>{c.name}</strong>
                  <span className={conflictChip(c.status)}>{c.status}</span>
                </div>
                <div className="muted sem-item-sum">
                  {(c.claims ?? []).map((k) => claimValue(k.value)).join("  ≠  ")}
                </div>
              </li>
            ))}
          </ul>
        </section>
        {current && (
          <ConflictPanel
            key={current.id}
            workspace={id}
            conflict={current}
            me={me}
            onClose={() => setSelected(null)}
          />
        )}
      </div>
    </div>
  );
}

function ConflictPanel({
  workspace,
  conflict: c,
  me,
  onClose,
}: {
  workspace: string;
  conflict: Conflict;
  me: Me;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [note, setNote] = useState("");
  const [reviewer, setReviewer] = useState(loadReviewer());
  const canWrite = me.role === "write";
  const claims = c.claims ?? [];
  const decide = useMutation({
    mutationFn: (decision: { pick: number } | { both_valid: true }) =>
      apiPost<{ reviewer: string }>(`/workspaces/${workspace}/conflicts/${c.id}/resolve`, {
        ...decision,
        note: note || null,
        reviewer: me.mode === "token" ? reviewer : null,
      }),
    onSuccess: () => {
      if (me.mode === "token") saveReviewer(reviewer);
      setNote("");
      void qc.invalidateQueries({ queryKey: ["conflicts"] });
    },
  });
  const needsName = me.mode === "token" && !reviewer.trim();

  return (
    <aside className="card sem-panel" aria-label="conflict">
      <div className="sem-panel-head">
        <strong className="sem-title">{c.name}</strong>
        <button className="linkish" onClick={onClose}>
          close
        </button>
      </div>
      <p className="sem-summary">
        <span className={conflictChip(c.status)}>{c.status}</span> {c.status_meaning}
      </p>
      <dl className="sem-props">
        <div>
          <dt>about</dt>
          <dd>
            {c.subject.name} <span className="muted">({c.subject.kind})</span>
          </dd>
        </div>
        <div>
          <dt>attribute</dt>
          <dd>{c.attribute}</dd>
        </div>
        {c.chosen_by_ekos != null && (
          <div>
            <dt>EKOS kept</dt>
            <dd>{claimValue(c.chosen_by_ekos)}</dd>
          </div>
        )}
        {c.measured?.violating_rows != null && (
          <div>
            <dt>measured</dt>
            <dd>{c.measured.violating_rows} contradicting row(s)</dd>
          </div>
        )}
        {c.review_note && (
          <div>
            <dt>note</dt>
            <dd>{c.review_note}</dd>
          </div>
        )}
      </dl>

      <strong className="sem-sub">Claims</strong>
      <ol className="sem-evidence conflict-claims">
        {claims.map((k, n) => (
          <li key={n}>
            <span>
              <strong>{n + 1}.</strong> {claimValue(k.value)}
              {c.picked_claim &&
                c.picked_claim.path === k.path &&
                claimValue(c.picked_claim.value) === claimValue(k.value) && (
                  <span className="chip ok" style={{ marginLeft: "0.4rem" }}>
                    picked
                  </span>
                )}
            </span>
            <span className="muted">
              {claimWhere(k)} · {k.source}
            </span>
            {canWrite && (
              <button
                disabled={decide.isPending || needsName}
                onClick={() => decide.mutate({ pick: n + 1 })}
              >
                Claim {n + 1} is right
              </button>
            )}
          </li>
        ))}
      </ol>

      <div className="sem-decide">
        <strong className="sem-sub">Decision</strong>
        {!canWrite && <p className="muted">Deciding needs the write role.</p>}
        {canWrite && (
          <>
            {me.mode === "token" && (
              <label className="sem-field">
                Reviewer (token mode has no identity; recorded as token:&lt;name&gt;)
                <input
                  aria-label="reviewer"
                  value={reviewer}
                  onChange={(e) => setReviewer(e.target.value)}
                />
              </label>
            )}
            <label className="sem-field">
              Note (required to say both are valid)
              <textarea
                aria-label="note"
                rows={2}
                value={note}
                onChange={(e) => setNote(e.target.value)}
              />
            </label>
            <div className="btnrow">
              <button
                disabled={decide.isPending || needsName || !note.trim()}
                onClick={() => decide.mutate({ both_valid: true })}
              >
                Both are valid
              </button>
            </div>
            {decide.isError && <p className="err">{String(decide.error)}</p>}
            {decide.isSuccess && <p className="ok-line">Recorded as {decide.data.reviewer}.</p>}
          </>
        )}
      </div>
    </aside>
  );
}
