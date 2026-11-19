// RFC 0172/0174 — where sources disagree about one fact, and a person's decision.
//
// A decision goes through the console API to the human-only `ekos conflicts resolve`; the MCP
// surface can list conflicts but never settle one.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { Conflict } from "../api/types";
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";
import { type Me, mayReview } from "../WorkspaceShell";
import {
  TYPE_LABEL,
  claimValue,
  claimWhere,
  conflictChip,
  conflictOrder,
} from "./conflicts-shared";
import { DetailPanel, PickRow, ReviewerField, SplitView } from "./decide-kit";
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
    <div className="dk-page">
      <PageHead
        title="Conflicts"
        sub="Where two sources state one fact differently (RFC 0172). EKOS keeps both claims; only a person decides which is right."
      />
      <SplitView
        panel={
          current && (
            <ConflictPanel
              key={current.id}
              workspace={id}
              conflict={current}
              me={me}
              onClose={() => setSelected(null)}
            />
          )
        }
      >
        <section className="card dk-list" aria-label="conflicts">
          <div className="dk-toolbar">
            <select
              aria-label="status"
              value={statusFilter}
              onChange={(e) => setStatus(e.target.value)}
            >
              <option value="open">open</option>
              <option value="resolved">resolved</option>
              <option value="dismissed">dismissed (both valid)</option>
              <option value="">any status</option>
            </select>
            <span className="dk-count">{shown.length} conflict(s)</span>
          </div>
          <QueryView
            q={list}
            what="conflicts"
            rows={6}
            isEmpty={(d) => d.length === 0}
            empty={
              statusFilter === "open" ? (
                <Empty title="No open conflicts." icon="check">
                  Sources agree on everything compiled so far: no source states a fact differently
                  from another.
                </Empty>
              ) : (
                <Empty title="Nothing with this status.">Pick another status above.</Empty>
              )
            }
          >
            {() => (
              <ul className="dk-rows">
                {shown.map((c) => (
                  <PickRow
                    key={c.id}
                    selected={selected === c.id}
                    onPick={() => setSelected(c.id)}
                    title={c.name}
                    before={
                      <span className="chip">{TYPE_LABEL[c.conflict_type] ?? c.conflict_type}</span>
                    }
                    after={<span className={conflictChip(c.status)}>{c.status}</span>}
                    sub={(c.claims ?? []).map((k) => claimValue(k.value)).join("  ≠  ")}
                  />
                ))}
              </ul>
            )}
          </QueryView>
        </section>
      </SplitView>
    </div>
  );
}

/** What the conflict is about, as a definition list. */
function ConflictFacts({ c }: { c: Conflict }) {
  return (
    <dl className="dk-props">
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
  );
}

function isPicked(c: Conflict, k: NonNullable<Conflict["claims"]>[number]): boolean {
  return (
    c.picked_claim != null &&
    c.picked_claim.path === k.path &&
    claimValue(c.picked_claim.value) === claimValue(k.value)
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
  const canWrite = mayReview(me);
  const claims = c.claims ?? [];
  const decide = useMutation({
    mutationFn: (decision: { pick: number } | { both_valid: true }) =>
      apiPost<{ reviewer: string }>(`/workspaces/${workspace}/conflicts/${c.id}/resolve`, {
        ...decision,
        note: note || null,
        reviewer: me.mode !== "oidc" ? reviewer : null,
      }),
    onSuccess: () => {
      if (me.mode !== "oidc") saveReviewer(reviewer);
      setNote("");
      void qc.invalidateQueries({ queryKey: ["conflicts"] });
    },
  });
  const needsName = me.mode !== "oidc" && !reviewer.trim();

  return (
    <DetailPanel label="conflict" title={<h3 className="dk-title">{c.name}</h3>} onClose={onClose}>
      <p className="dk-summary">
        <span className={conflictChip(c.status)}>{c.status}</span> {c.status_meaning}
      </p>
      <ConflictFacts c={c} />

      <h4 className="dk-sub">Claims</h4>
      <ol className="dk-claims">
        {claims.map((k, n) => (
          <li key={n}>
            <span>
              <strong>{n + 1}.</strong> {claimValue(k.value)}
              {isPicked(c, k) && <span className="chip ok dk-chip-gap">picked</span>}
            </span>
            <span className="muted">
              {claimWhere(k)} · {k.source}
            </span>
            {canWrite && (
              <button
                type="button"
                disabled={decide.isPending || needsName}
                onClick={() => decide.mutate({ pick: n + 1 })}
              >
                Claim {n + 1} is right
              </button>
            )}
          </li>
        ))}
      </ol>

      <section className="dk-section" aria-label="decision">
        <h4 className="dk-sub">Decision</h4>
        {!canWrite && <p className="muted">Deciding needs the write role.</p>}
        {canWrite && (
          <>
            {me.mode !== "oidc" && (
              <ReviewerField ariaLabel="reviewer" value={reviewer} onChange={setReviewer} />
            )}
            <label className="dk-field">
              <span>Note (required to say both are valid)</span>
              <textarea
                aria-label="note"
                rows={2}
                value={note}
                onChange={(e) => setNote(e.target.value)}
              />
            </label>
            <div className="dk-actions">
              <button
                type="button"
                disabled={decide.isPending || needsName || !note.trim()}
                onClick={() => decide.mutate({ both_valid: true })}
              >
                Both are valid
              </button>
            </div>
            {decide.isError && <ErrorNote error={decide.error} />}
            {decide.isSuccess && <p className="ok-line">Recorded as {decide.data.reviewer}.</p>}
          </>
        )}
      </section>
    </DetailPanel>
  );
}
