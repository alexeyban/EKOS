// RFC 0213 — the source-model review: one tab per subject, decisions in place, and a sign-off
// bound to the digest shown here.
//
// Decisions and sign-off go through the console API to the human-only `ekos review …` and
// `ekos signoff …` commands; no MCP tool can make either, and no API token holds the right.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { NavLink, useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { ReviewItem, ReviewSubjectDetail, SubjectStatus } from "../api/types";
import { Empty, ErrorNote, PageHead, Skeleton } from "../ui";
import { type Me, mayReview } from "../WorkspaceShell";
import {
  DECIDED_ELSEWHERE,
  SUBJECT_LABEL,
  decisionLine,
  factsLine,
  infoLine,
  signChip,
  signLine,
  visibleItems,
} from "./review-shared";
import { DetailPanel, ReviewerField, SplitView } from "./decide-kit";
import { loadReviewer, saveReviewer } from "./semantics-shared";

const SHOWN = 300;

export function Review() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const [subject, setSubject] = useState("scope");
  const list = useQuery({
    queryKey: ["review", id],
    queryFn: () => api<{ subjects: SubjectStatus[] }>(`/workspaces/${id}/review`),
    // An answer without the list reads as an empty review, not a crash.
    select: (d) => ({ subjects: d?.subjects ?? [] }),
    enabled: id !== "",
  });
  const current =
    list.data?.subjects.find((s) => s.subject === subject) ?? list.data?.subjects[0];

  return (
    <div className="dk-page">
      <PageHead
        title="Review"
        sub="Agree to the source model before anything is built on it. Each sign-off is bound to the digest of what this page shows; when a signed fact changes, it turns stale (RFC 0213)."
      />
      {list.isLoading && (
        <>
          <Skeleton rows={4} label="computing the review" />
          <p className="muted small">
            Computing the review… (the first time after a compile reads the whole ledger)
          </p>
        </>
      )}
      {list.isError && <ErrorNote error={list.error} what="review subjects" />}
      {list.data && list.data.subjects.length === 0 && (
        <Empty title="Nothing to review yet" icon="review">
          The review lists what a compile recovered. Run the pipeline through{" "}
          <code>ekos commit</code>, then come back to agree to the source model.
        </Empty>
      )}
      {list.data && list.data.subjects.length > 0 && (
        <div className="dk-tabs" role="group" aria-label="subjects">
          {list.data.subjects.map((s) => (
            <button
              key={s.subject}
              type="button"
              aria-pressed={s.subject === current?.subject}
              onClick={() => setSubject(s.subject)}
            >
              {SUBJECT_LABEL[s.subject] ?? s.subject}{" "}
              <span className={signChip(s.signoff)}>{s.signoff.state}</span>
              {s.blocking > 0 && <span className="chip bad">{s.blocking} blocking</span>}
            </button>
          ))}
        </div>
      )}
      {current && <SubjectView key={current.subject} workspace={id} status={current} me={me} />}
    </div>
  );
}

function SubjectView({
  workspace,
  status,
  me,
}: {
  workspace: string;
  status: SubjectStatus;
  me: Me;
}) {
  const subject = status.subject;
  const [undecided, setUndecided] = useState(false);
  const [filter, setFilter] = useState("");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const detail = useQuery({
    queryKey: ["review", workspace, subject, undecided],
    queryFn: () =>
      api<ReviewSubjectDetail>(
        `/workspaces/${workspace}/review/${subject}${undecided ? "?undecided=true" : ""}`,
      ),
  });
  const elsewhere = DECIDED_ELSEWHERE[subject];
  const reviewer = useReviewer(me);
  const rows = visibleItems(detail.data?.rows ?? [], filter);

  return (
    <SplitView
      panel={
        <DetailPanel label="decide and sign off">
          {mayReview(me) && reviewer.field}
          {!elsewhere && (
            <Decide
              workspace={workspace}
              subject={subject}
              targets={[...picked]}
              me={me}
              r={reviewer}
              onDone={() => setPicked(new Set())}
            />
          )}
          <SignOffBox workspace={workspace} status={status} me={me} r={reviewer} />
        </DetailPanel>
      }
    >
      <section className="card dk-list" aria-label="items">
        <p className="dk-count">
          <strong>{SUBJECT_LABEL[subject] ?? subject}</strong>
          <span>{status.items} item(s)</span>
          <span className="chip ok">{status.decided} decided</span>
          {status.outdated > 0 && <span className="chip warn">{status.outdated} outdated</span>}
          {status.blocking > 0 && <span className="chip bad">{status.blocking} blocking</span>}
        </p>
        {elsewhere && (
          <p className="muted">
            These items are decided on the{" "}
            <NavLink to={`/w/${workspace}/${elsewhere.to}`}>{elsewhere.label}</NavLink> page; sign
            the subject off here.
          </p>
        )}
        <div className="dk-toolbar">
          <input
            className="dk-search"
            aria-label="filter"
            placeholder="filter"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <label className="dk-check">
            <input
              type="checkbox"
              checked={undecided}
              onChange={(e) => setUndecided(e.target.checked)}
            />{" "}
            undecided only
          </label>
        </div>
        {detail.isPending && <Skeleton rows={6} label="loading the items" />}
        {detail.isError && <ErrorNote error={detail.error} />}
        {detail.data && (
          <ItemTable
            rows={rows}
            selectable={!elsewhere}
            picked={picked}
            setPicked={setPicked}
            undecided={undecided}
          />
        )}
      </section>
    </SplitView>
  );
}

/** The subject's items: signed facts, what is only shown, and the decision. */
function ItemTable({
  rows,
  selectable,
  picked,
  setPicked,
  undecided,
}: {
  rows: ReviewItem[];
  selectable: boolean;
  picked: Set<string>;
  setPicked: (f: (p: Set<string>) => Set<string>) => void;
  undecided: boolean;
}) {
  const shown = rows.slice(0, SHOWN);
  const toggle = (item: ReviewItem) =>
    setPicked((p) => {
      const n = new Set(p);
      if (n.has(item.label)) n.delete(item.label);
      else n.add(item.label);
      return n;
    });
  if (rows.length === 0) {
    return (
      <Empty title={undecided ? "Everything here is decided" : "No items"} icon="check">
        {undecided
          ? "Untick “undecided only” to see the decisions, or sign the subject off."
          : "Nothing matches the filter, or the compile recovered nothing for this subject."}
      </Empty>
    );
  }
  return (
    <>
      {rows.length > SHOWN && (
        <p className="muted">
          showing {SHOWN} of {rows.length}; narrow with the filter
        </p>
      )}
      <div className="table-scroll">
        <table className="dk-table dk-review-table">
          <thead>
            <tr>
              {selectable && (
                <th>
                  <input
                    type="checkbox"
                    aria-label="select all shown"
                    checked={shown.length > 0 && shown.every((i) => picked.has(i.label))}
                    onChange={(e) =>
                      setPicked(() =>
                        e.target.checked ? new Set(shown.map((i) => i.label)) : new Set(),
                      )
                    }
                  />
                </th>
              )}
              <th>item</th>
              <th>facts (signed)</th>
              <th>shown only</th>
              <th>decision</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((i) => (
              <tr key={i.id} className={picked.has(i.label) ? "is-selected" : undefined}>
                {selectable && (
                  <td>
                    <input
                      type="checkbox"
                      aria-label={`select ${i.label}`}
                      checked={picked.has(i.label)}
                      onChange={() => toggle(i)}
                    />
                  </td>
                )}
                <td>
                  {i.blocking && <span className="chip bad">blocking</span>} {i.label}
                </td>
                <td>{factsLine(i)}</td>
                <td className="muted">{infoLine(i)}</td>
                <td>{decisionLine(i)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}

const VERDICTS: Record<string, { verdict: string; label: string }[]> = {
  scope: [
    { verdict: "in", label: "In scope" },
    { verdict: "out", label: "Out of scope" },
  ],
  pii: [
    { verdict: "confirm", label: "Is personal data" },
    { verdict: "reject", label: "Is not" },
  ],
  "write-behaviour": [{ verdict: "confirm", label: "Confirm" }],
  identity: [
    { verdict: "confirm", label: "Same thing" },
    { verdict: "reject", label: "Different things" },
  ],
};

type Reviewer = ReturnType<typeof useReviewer>;

/** One reviewer field per page: token and local mode have no identity, so the person types a
 * name, recorded as `token:<name>` or `local:<name>`. */
function useReviewer(me: Me) {
  const [reviewer, setReviewer] = useState(loadReviewer());
  const needsName = me.mode !== "oidc" && !reviewer.trim();
  const field =
    me.mode !== "oidc" ? (
      <ReviewerField ariaLabel="reviewer" value={reviewer} onChange={setReviewer} />
    ) : null;
  const who = me.mode !== "oidc" ? reviewer : null;
  const remember = () => {
    if (me.mode !== "oidc") saveReviewer(reviewer);
  };
  return { field, who, needsName, remember };
}

function Decide({
  workspace,
  subject,
  targets,
  me,
  r,
  onDone,
}: {
  workspace: string;
  subject: string;
  targets: string[];
  me: Me;
  r: Reviewer;
  onDone: () => void;
}) {
  const qc = useQueryClient();
  const [reason, setReason] = useState("");
  const [mask, setMask] = useState("redact");
  const decide = useMutation({
    mutationFn: (verdict: string) =>
      apiPost<{ reviewer: string; message: string }>(
        `/workspaces/${workspace}/review/${subject}/decide`,
        {
          targets,
          verdict,
          mask: subject === "pii" && verdict === "confirm" ? mask : null,
          reason: reason || null,
          reviewer: r.who,
        },
      ),
    onSuccess: () => {
      r.remember();
      setReason("");
      onDone();
      void qc.invalidateQueries({ queryKey: ["review", workspace] });
      void qc.invalidateQueries({ queryKey: ["workflow", workspace] });
    },
  });
  if (!mayReview(me)) return <p className="muted">Deciding needs the reviewer role.</p>;
  return (
    <section className="dk-section" aria-label="decide">
      <h4 className="dk-sub">Decide {targets.length} selected</h4>
      {subject === "pii" && (
        <label className="dk-field">
          <span>Mask downstream</span>
          <select aria-label="mask" value={mask} onChange={(e) => setMask(e.target.value)}>
            <option value="redact">redact</option>
            <option value="hash">hash</option>
            <option value="none">none</option>
          </select>
        </label>
      )}
      <label className="dk-field">
        <span>{subject === "identity" ? "Note (required for “different”)" : "Reason"}</span>
        <textarea
          aria-label="reason"
          rows={2}
          value={reason}
          onChange={(e) => setReason(e.target.value)}
        />
      </label>
      <div className="dk-actions">
        {(VERDICTS[subject] ?? []).map((v) => (
          <button
            key={v.verdict}
            type="button"
            disabled={decide.isPending || r.needsName || targets.length === 0}
            onClick={() => decide.mutate(v.verdict)}
          >
            {v.label}
          </button>
        ))}
      </div>
      {decide.isPending && <p className="muted small">recording… (reads the whole ledger)</p>}
      {decide.isError && <ErrorNote error={decide.error} />}
      {decide.isSuccess && <p className="ok-line">{decide.data.message}</p>}
    </section>
  );
}

function SignOffBox({
  workspace,
  status,
  me,
  r,
}: {
  workspace: string;
  status: SubjectStatus;
  me: Me;
  r: Reviewer;
}) {
  const qc = useQueryClient();
  const subject = status.subject;
  const [confirm, setConfirm] = useState("");
  const [note, setNote] = useState("");
  const [reason, setReason] = useState("");
  const done = () => {
    r.remember();
    setConfirm("");
    setNote("");
    setReason("");
    void qc.invalidateQueries({ queryKey: ["review", workspace] });
    void qc.invalidateQueries({ queryKey: ["workflow", workspace] });
  };
  const approve = useMutation({
    mutationFn: () =>
      apiPost<{ message: string }>(`/workspaces/${workspace}/review/${subject}/signoff`, {
        digest: status.digest,
        confirm,
        note: note || null,
        reviewer: r.who,
      }),
    onSuccess: done,
  });
  const revoke = useMutation({
    mutationFn: () =>
      apiPost<{ message: string }>(`/workspaces/${workspace}/review/${subject}/revoke`, {
        reason,
        reviewer: r.who,
      }),
    onSuccess: done,
  });
  const s = status.signoff;
  const signed = s.state === "valid" || s.state === "stale";
  const busy = approve.isPending || revoke.isPending;

  return (
    <section className="dk-section" aria-label="sign-off">
      <h4 className="dk-sub">Sign-off</h4>
      <p>
        <span className={signChip(s)}>{s.state}</span> {signLine(s)}
      </p>
      {s.state === "stale" && <StaleList s={s} />}
      <p className="muted small">
        digest <span className="dk-digest">{status.digest}</span>
      </p>
      {mayReview(me) ? (
        <>
          {status.blocking > 0 && (
            <p className="muted">
              {status.blocking} blocking item(s) must be decided before this subject can be signed
              off.
            </p>
          )}
          {status.blocking === 0 && (
            <>
              <label className="dk-field">
                <span>
                  Type <code>{subject}</code> to sign off what this page shows
                </span>
                <input
                  aria-label="confirm"
                  value={confirm}
                  onChange={(e) => setConfirm(e.target.value)}
                />
              </label>
              <label className="dk-field">
                <span>Note</span>
                <input aria-label="note" value={note} onChange={(e) => setNote(e.target.value)} />
              </label>
              <div className="dk-actions">
                <button
                  type="button"
                  className="primary"
                  disabled={approve.isPending || r.needsName || confirm.trim() !== subject}
                  onClick={() => approve.mutate()}
                >
                  Sign off
                </button>
              </div>
            </>
          )}
          {signed && (
            <>
              <label className="dk-field">
                <span>Reason to withdraw</span>
                <input
                  aria-label="revoke reason"
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                />
              </label>
              <div className="dk-actions">
                <button
                  type="button"
                  className="dk-danger"
                  disabled={revoke.isPending || r.needsName || !reason.trim()}
                  onClick={() => revoke.mutate()}
                >
                  Revoke
                </button>
              </div>
            </>
          )}
          {(approve.isError || revoke.isError) && (
            <ErrorNote error={approve.error ?? revoke.error} />
          )}
          {busy && (
            <p className="muted small">recording… (checks the digest against the whole ledger)</p>
          )}
          {approve.isSuccess && <p className="ok-line">{approve.data.message}</p>}
          {revoke.isSuccess && <p className="ok-line">{revoke.data.message}</p>}
        </>
      ) : (
        <p className="muted">Signing off needs the reviewer role.</p>
      )}
    </section>
  );
}

/** What changed since a stale sign-off, at most 20 lines. */
function StaleList({ s }: { s: SubjectStatus["signoff"] }) {
  if (s.state !== "stale") return null;
  const lines = [
    ...s.added.map((x) => `added ${x}`),
    ...s.removed.map((x) => `removed ${x}`),
    ...s.changed.map((x) => `changed ${x}`),
  ].slice(0, 20);
  return (
    <ul className="muted small dk-stale">
      {lines.map((x) => (
        <li key={x}>{x}</li>
      ))}
    </ul>
  );
}
