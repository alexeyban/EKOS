// RFC 0254 — open questions: what EKOS needs a person to tell it, ranked, and who to ask.
//
// Questions are derived from the ledger and their status is computed on read; nothing here
// decides one. A person can reveal a question's unexplained values (read live, never stored),
// record an answer heard elsewhere (the operator's testimony, naming who said it), hand a
// question to someone else, or — if it is theirs — say "not me".

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { Question } from "../api/types";
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";
import { type Me, mayOperate, mayReview } from "../WorkspaceShell";
import {
  QUESTION_TYPES,
  isMine,
  questionChip,
  revealedRows,
} from "./people-shared";
import { DetailPanel, PickRow, ReviewerField, SplitView } from "./decide-kit";
import { loadReviewer, saveReviewer } from "./semantics-shared";

export function Questions() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const [status, setStatus] = useState("open");
  const [type, setType] = useState("");
  const [table, setTable] = useState("");
  const [addressee, setAddressee] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const params = new URLSearchParams({ status });
  if (type) params.set("type", type);
  if (table.trim()) params.set("table", table.trim());
  if (addressee.trim()) params.set("addressee", addressee.trim());
  const list = useQuery({
    queryKey: ["questions", id, params.toString()],
    queryFn: () => api<Question[]>(`/workspaces/${id}/questions?${params}`),
    enabled: id !== "",
  });
  const shown = list.data ?? [];
  const current = shown.find((q) => q.id === selected) ?? null;

  return (
    <div className="dk-page">
      <PageHead
        title="Questions"
        sub="What EKOS cannot find in code or data, highest priority first, and who may know. An answer is a person's testimony; the review still decides."
      />
      <SplitView
        panel={
          current && (
            <QuestionPanel
              key={current.id}
              workspace={id}
              question={current}
              me={me}
              onClose={() => setSelected(null)}
            />
          )
        }
      >
        <section className="card dk-list" aria-label="questions">
          <div className="dk-toolbar">
            <select aria-label="status" value={status} onChange={(e) => setStatus(e.target.value)}>
              <option value="open">open</option>
              <option value="answered">answered</option>
              <option value="closed">closed</option>
              <option value="obsolete">obsolete</option>
              <option value="all">any status</option>
            </select>
            <select aria-label="type" value={type} onChange={(e) => setType(e.target.value)}>
              <option value="">any type</option>
              {Object.entries(QUESTION_TYPES).map(([k, label]) => (
                <option key={k} value={k}>
                  {label}
                </option>
              ))}
            </select>
            <input
              aria-label="table"
              placeholder="table"
              value={table}
              onChange={(e) => setTable(e.target.value)}
            />
            <input
              aria-label="addressee"
              placeholder="with (person)"
              value={addressee}
              onChange={(e) => setAddressee(e.target.value)}
            />
          </div>
          <div className="dk-count">{shown.length} question(s)</div>
          <QueryView
            q={list}
            what="questions"
            rows={6}
            isEmpty={(d) => d.length === 0}
            empty={
              <Empty title="No questions with these filters." icon="questions">
                With the status on open and nothing listed, EKOS has what it needs from code and
                data. Profiling a source (<code>ekos profile run</code>) or a compile can raise new
                ones.
              </Empty>
            }
          >
            {() => (
              <ul className="dk-rows">
                {shown.map((q) => (
                  <PickRow
                    key={q.id}
                    selected={selected === q.id}
                    onPick={() => setSelected(q.id)}
                    title={q.text}
                    before={<span className="chip">{QUESTION_TYPES[q.type] ?? q.type}</span>}
                    after={
                      <>
                        <span className={questionChip(q.status)}>{q.status}</span>
                        {q.blocking && <span className="chip bad">blocking</span>}
                      </>
                    }
                    sub={
                      <>
                        {q.table}
                        {q.column ? `.${q.column}` : ""} · ask {q.addressee ?? "nobody yet"}
                      </>
                    }
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

function QuestionPanel({
  workspace,
  question: q,
  me,
  onClose,
}: {
  workspace: string;
  question: Question;
  me: Me;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [reviewer, setReviewer] = useState(loadReviewer());
  const who = me.mode !== "oidc" ? reviewer : null;
  const named = me.mode === "oidc" || reviewer.trim() !== "";
  const base = `/workspaces/${workspace}/questions/${q.id}`;
  const refresh = () => {
    if (me.mode !== "oidc") saveReviewer(reviewer);
    void qc.invalidateQueries({ queryKey: ["questions", workspace] });
  };
  const notMe = useMutation({
    mutationFn: () => apiPost(`${base}/decline`, { reviewer: who }),
    onSuccess: refresh,
  });

  return (
    <DetailPanel label="question" title={<h3 className="dk-title">{q.text}</h3>} onClose={onClose}>
      <p className="dk-summary">
        <span className={questionChip(q.status)}>{q.status}</span>{" "}
        {QUESTION_TYPES[q.type] ?? q.type} · with {q.addressee ?? "nobody yet"}
      </p>
      {q.addressees && q.addressees.length > 1 && (
        <p className="muted small">Next who may know: {q.addressees.join(", ")}</p>
      )}
      {q.context != null && (
        <details className="dk-details">
          <summary>Context</summary>
          <pre className="dk-raw">{JSON.stringify(q.context, null, 2)}</pre>
        </details>
      )}
      <Answers q={q} />
      {q.type === "unexplained_values" && <Reveal base={base} />}
      {me.mode !== "oidc" && (
        <ReviewerField who="Your name" ariaLabel="reviewer" value={reviewer} onChange={setReviewer} />
      )}
      {isMine(q, me, reviewer) && (
        <div className="dk-actions">
          <button type="button" disabled={notMe.isPending} onClick={() => notMe.mutate()}>
            Not me
          </button>
          <span className="muted small">it moves to the next person who may know</span>
          {notMe.isError && <ErrorNote error={notMe.error} />}
        </div>
      )}
      {mayReview(me) && <RecordAnswer base={base} reviewer={who} named={named} onDone={refresh} />}
      {mayOperate(me) && <Reassign base={base} onDone={refresh} />}
    </DetailPanel>
  );
}

function Answers({ q }: { q: Question }) {
  if (q.answers.length === 0) return null;
  return (
    <>
      <h4 className="dk-sub">Answers</h4>
      <ul className="dk-said">
        {q.answers.map((a) => (
          <li key={a.testimony}>
            <span className="muted small">
              {a.author} ({a.channel}, {a.sent_at.slice(0, 10)})
            </span>
            <div>{a.text}</div>
          </li>
        ))}
      </ul>
    </>
  );
}

/** Reveal a question's unexplained values: read live, never stored. */
function Reveal({ base }: { base: string }) {
  const reveal = useMutation({
    mutationFn: () => api<Question>(`${base}/reveal`),
  });
  return (
    <section className="dk-section" aria-label="values">
      <div className="dk-actions">
        <button type="button" disabled={reveal.isPending} onClick={() => reveal.mutate()}>
          Reveal values
        </button>
        <span className="muted small">
          read live from the source now; not stored, counts only for personal data
        </span>
      </div>
      {reveal.isError && <ErrorNote error={reveal.error} />}
      {reveal.data && <Revealed value={reveal.data.revealed} />}
    </section>
  );
}

function Revealed({ value }: { value: unknown }) {
  const shown = revealedRows(value);
  if ("note" in shown) return <p className="muted">{shown.note}</p>;
  const cols = Array.from(new Set(shown.rows.flatMap((r) => Object.keys(r))));
  return (
    <div className="table-scroll">
      <table className="dk-table" aria-label="revealed values">
        <thead>
          <tr>
            {cols.map((c) => (
              <th key={c}>{c}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {shown.rows.map((r, n) => (
            <tr key={n}>
              {cols.map((c) => (
                <td key={c}>{String(r[c] ?? "")}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function RecordAnswer({
  base,
  reviewer,
  named,
  onDone,
}: {
  base: string;
  reviewer: string | null;
  named: boolean;
  onDone: () => void;
}) {
  const [text, setText] = useState("");
  const [saidBy, setSaidBy] = useState("");
  const save = useMutation({
    mutationFn: () =>
      apiPost<{ by: string }>(`${base}/answer`, {
        text,
        said_by: saidBy,
        reviewer,
      }),
    onSuccess: () => {
      setText("");
      setSaidBy("");
      onDone();
    },
  });
  return (
    <section className="dk-section" aria-label="record an answer">
      <h4 className="dk-sub">Record an answer heard elsewhere</h4>
      <p className="muted small">
        Recorded as your testimony, naming who said it — never as theirs.
      </p>
      <label className="dk-field">
        <span>Who said it</span>
        <input
          aria-label="said by"
          value={saidBy}
          onChange={(e) => setSaidBy(e.target.value)}
        />
      </label>
      <label className="dk-field">
        <span>What they said</span>
        <textarea
          aria-label="answer"
          rows={3}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      </label>
      <div className="dk-actions">
        <button
          type="button"
          className="primary"
          disabled={!named || save.isPending || !text.trim() || !saidBy.trim()}
          onClick={() => save.mutate()}
        >
          Record the answer
        </button>
      </div>
      {save.isError && <ErrorNote error={save.error} />}
      {save.isSuccess && <p className="ok-line">Recorded by {save.data.by}.</p>}
    </section>
  );
}

function Reassign({ base, onDone }: { base: string; onDone: () => void }) {
  const [to, setTo] = useState("");
  const go = useMutation({
    mutationFn: () => apiPost(`${base}/reassign`, { to }),
    onSuccess: () => {
      setTo("");
      onDone();
    },
  });
  return (
    <div className="dk-actions dk-reassign">
      <input
        aria-label="reassign to"
        placeholder="hand to (e-mail)"
        value={to}
        onChange={(e) => setTo(e.target.value)}
      />
      <button type="button" disabled={go.isPending || !to.trim()} onClick={() => go.mutate()}>
        Reassign
      </button>
      {go.isError && <ErrorNote error={go.error} />}
    </div>
  );
}
