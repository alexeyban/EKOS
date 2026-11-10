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
import { type Me, mayOperate, mayReview } from "../WorkspaceShell";
import {
  QUESTION_TYPES,
  isMine,
  questionChip,
  revealedRows,
} from "./people-shared";
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
    <div className="sem-page">
      <div className="sem-views">
        <select
          aria-label="status"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
        >
          <option value="open">open</option>
          <option value="answered">answered</option>
          <option value="closed">closed</option>
          <option value="obsolete">obsolete</option>
          <option value="all">any status</option>
        </select>
        <select
          aria-label="type"
          value={type}
          onChange={(e) => setType(e.target.value)}
        >
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
        <span className="muted sem-note">
          What EKOS cannot find in code or data, highest priority first. An
          answer is a person's testimony; the review still decides.
        </span>
      </div>
      <div className={current ? "sem-layout with-panel" : "sem-layout"}>
        <section className="card sem-list">
          <p className="muted">{shown.length} question(s)</p>
          {list.isLoading && <p className="muted">loading…</p>}
          {list.isError && <p className="err">{String(list.error)}</p>}
          {list.data?.length === 0 && (
            <p className="muted">No questions with these filters.</p>
          )}
          <ul className="sem-items">
            {shown.map((q) => (
              <li key={q.id} className={selected === q.id ? "selected" : ""}>
                <div className="sem-item-head">
                  <span className="chip">
                    {QUESTION_TYPES[q.type] ?? q.type}
                  </span>
                  <button
                    type="button"
                    className="sem-item-pick"
                    aria-pressed={selected === q.id}
                    onClick={() => setSelected(q.id)}
                  >
                    {q.text}
                  </button>
                  <span className={questionChip(q.status)}>{q.status}</span>
                  {q.blocking && <span className="chip bad">blocking</span>}
                </div>
                <div className="muted sem-item-sum">
                  {q.table}
                  {q.column ? `.${q.column}` : ""} · ask{" "}
                  {q.addressee ?? "nobody yet"}
                </div>
              </li>
            ))}
          </ul>
        </section>
        {current && (
          <QuestionPanel
            key={current.id}
            workspace={id}
            question={current}
            me={me}
            onClose={() => setSelected(null)}
          />
        )}
      </div>
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
  const who = me.mode === "token" ? reviewer : null;
  const named = me.mode !== "token" || reviewer.trim() !== "";
  const base = `/workspaces/${workspace}/questions/${q.id}`;
  const refresh = () => {
    if (me.mode === "token") saveReviewer(reviewer);
    void qc.invalidateQueries({ queryKey: ["questions", workspace] });
  };
  const reveal = useMutation({
    mutationFn: () => api<Question>(`${base}/reveal`),
  });
  const notMe = useMutation({
    mutationFn: () => apiPost(`${base}/decline`, { reviewer: who }),
    onSuccess: refresh,
  });

  return (
    <aside className="card sem-panel" aria-label="question">
      <div className="sem-panel-head">
        <strong className="sem-title">{q.text}</strong>
        <button className="linkish" onClick={onClose}>
          close
        </button>
      </div>
      <p className="sem-summary">
        <span className={questionChip(q.status)}>{q.status}</span>{" "}
        {QUESTION_TYPES[q.type] ?? q.type} · with {q.addressee ?? "nobody yet"}
      </p>
      {q.addressees && q.addressees.length > 1 && (
        <p className="muted small">
          Next who may know: {q.addressees.join(", ")}
        </p>
      )}
      {q.context != null && (
        <details>
          <summary>Context</summary>
          <pre>{JSON.stringify(q.context, null, 2)}</pre>
        </details>
      )}
      {q.answers.length > 0 && (
        <>
          <strong className="sem-sub">Answers</strong>
          <ul className="rel-list">
            {q.answers.map((a) => (
              <li key={a.testimony}>
                <span className="muted">
                  {a.author} ({a.channel}, {a.sent_at.slice(0, 10)})
                </span>
                <div>{a.text}</div>
              </li>
            ))}
          </ul>
        </>
      )}
      {q.type === "unexplained_values" && (
        <div>
          <button disabled={reveal.isPending} onClick={() => reveal.mutate()}>
            Reveal values
          </button>{" "}
          <span className="muted small">
            read live from the source now; not stored, counts only for personal
            data
          </span>
          {reveal.isError && <p className="err">{String(reveal.error)}</p>}
          {reveal.data && <Revealed value={reveal.data.revealed} />}
        </div>
      )}
      {me.mode === "token" && (
        <label className="sem-field">
          <span>
            Your name (token mode has no identity; recorded as
            token:&lt;name&gt;)
          </span>
          <input
            aria-label="reviewer"
            value={reviewer}
            onChange={(e) => setReviewer(e.target.value)}
          />
        </label>
      )}
      {isMine(q, me, reviewer) && (
        <div className="btnrow">
          <button disabled={notMe.isPending} onClick={() => notMe.mutate()}>
            Not me
          </button>
          <span className="muted small">
            it moves to the next person who may know
          </span>
          {notMe.isError && <p className="err">{String(notMe.error)}</p>}
        </div>
      )}
      {mayReview(me) && (
        <RecordAnswer
          base={base}
          reviewer={who}
          named={named}
          onDone={refresh}
        />
      )}
      {mayOperate(me) && <Reassign base={base} onDone={refresh} />}
    </aside>
  );
}

function Revealed({ value }: { value: unknown }) {
  const shown = revealedRows(value);
  if ("note" in shown) return <p className="muted">{shown.note}</p>;
  const cols = Array.from(new Set(shown.rows.flatMap((r) => Object.keys(r))));
  return (
    <table className="lm-table" aria-label="revealed values">
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
    <div className="sem-decide">
      <strong className="sem-sub">Record an answer heard elsewhere</strong>
      <p className="muted small">
        Recorded as your testimony, naming who said it — never as theirs.
      </p>
      <label className="sem-field">
        <span>Who said it</span>
        <input
          aria-label="said by"
          value={saidBy}
          onChange={(e) => setSaidBy(e.target.value)}
        />
      </label>
      <label className="sem-field">
        <span>What they said</span>
        <textarea
          aria-label="answer"
          rows={3}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      </label>
      <button
        disabled={!named || save.isPending || !text.trim() || !saidBy.trim()}
        onClick={() => save.mutate()}
      >
        Record the answer
      </button>
      {save.isError && <p className="err">{String(save.error)}</p>}
      {save.isSuccess && <p className="ok-line">Recorded by {save.data.by}.</p>}
    </div>
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
    <div className="btnrow">
      <input
        aria-label="reassign to"
        placeholder="hand to (e-mail)"
        value={to}
        onChange={(e) => setTo(e.target.value)}
      />
      <button disabled={go.isPending || !to.trim()} onClick={() => go.mutate()}>
        Reassign
      </button>
      {go.isError && <p className="err">{String(go.error)}</p>}
    </div>
  );
}
