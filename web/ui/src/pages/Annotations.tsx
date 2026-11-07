// RFC 0247 — what people wrote about an entity: comments, instructions and synonyms.
//
// Everything written here goes through the console API to the human-only `ekos annotate …`; the
// MCP surface can read annotations but never write one. A model's output is never an annotation.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { Annotation, AnnotationKind } from "../api/types";
import { type Me, mayOperate, mayReview } from "../WorkspaceShell";
import {
  KIND_HELP,
  annotationOrder,
  parseRule,
  reach,
  stateChip,
  targetLabel,
  validTarget,
} from "./annotations-shared";
import { loadReviewer, saveReviewer } from "./semantics-shared";

export function Annotations() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const [state, setState] = useState("current");
  const [selected, setSelected] = useState<string | null>(null);
  const list = useQuery({
    queryKey: ["annotations", id],
    queryFn: () => api<Annotation[]>(`/workspaces/${id}/annotations`),
    enabled: id !== "",
  });
  const shown = annotationOrder(
    (list.data ?? []).filter((a) =>
      state === "all" ? true : state === "current" ? a.state !== "withdrawn" : a.state === state,
    ),
  );
  const current = shown.find((a) => a.id === selected) ?? null;

  return (
    <div className="sem-page">
      <div className="sem-views">
        <select aria-label="state" value={state} onChange={(e) => setState(e.target.value)}>
          <option value="current">active and stale</option>
          <option value="active">active only</option>
          <option value="stale">stale only</option>
          <option value="withdrawn">withdrawn</option>
          <option value="all">all</option>
        </select>
        <span className="muted sem-note">
          Written by people, never by a model. A note goes stale when the entity it describes
          changes (RFC 0247).
        </span>
      </div>
      <div className={current ? "sem-layout with-panel" : "sem-layout"}>
        <section className="card sem-list">
          {mayOperate(me) && <AddForm workspace={id} me={me} />}
          <p className="muted">{shown.length} annotation(s)</p>
          {list.isLoading && <p className="muted">loading…</p>}
          {list.isError && <p className="err">{String(list.error)}</p>}
          {list.data?.length === 0 && (
            <p className="muted">
              No annotations yet. Add a comment, an instruction or a synonym to an entity.
            </p>
          )}
          <ul className="sem-items">
            {shown.map((a) => (
              <li key={a.id} className={selected === a.id ? "selected" : ""}>
                <div className="sem-item-head">
                  <span className="chip">{a.kind}</span>
                  <button
                    type="button"
                    className="sem-item-pick"
                    aria-pressed={selected === a.id}
                    onClick={() => setSelected(a.id)}
                  >
                    {a.term ? `“${a.term}” → ` : ""}
                    {targetLabel(a.target)}
                  </button>
                  <span className={stateChip(a.state)}>{a.state}</span>
                </div>
                <div className="muted sem-item-sum">{a.text}</div>
              </li>
            ))}
          </ul>
        </section>
        {current && (
          <AnnotationPanel
            key={current.id}
            workspace={id}
            annotation={current}
            me={me}
            onClose={() => setSelected(null)}
          />
        )}
      </div>
    </div>
  );
}

function AddForm({ workspace, me }: { workspace: string; me: Me }) {
  const qc = useQueryClient();
  const [target, setTarget] = useState("");
  const [kind, setKind] = useState<AnnotationKind>("comment");
  const [text, setText] = useState("");
  const [term, setTerm] = useState("");
  const [ruleText, setRuleText] = useState("");
  const [shared, setShared] = useState(false);
  const [reviewer, setReviewer] = useState(loadReviewer());
  const rule = parseRule(ruleText);
  const needsName = me.mode === "token" && !reviewer.trim();
  const ready =
    validTarget(target) &&
    text.trim() !== "" &&
    !rule.error &&
    (kind !== "synonym" || term.trim() !== "") &&
    !needsName;
  const add = useMutation({
    mutationFn: () =>
      apiPost<{ author: string }>(`/workspaces/${workspace}/annotations`, {
        target: target.trim(),
        kind,
        text,
        rule: kind === "instruction" ? rule.rule : null,
        term: kind === "synonym" ? term.trim() : null,
        shared,
        reviewer: me.mode === "token" ? reviewer : null,
      }),
    onSuccess: () => {
      if (me.mode === "token") saveReviewer(reviewer);
      setText("");
      setTerm("");
      setRuleText("");
      void qc.invalidateQueries({ queryKey: ["annotations"] });
    },
  });

  return (
    <form
      className="sem-decide"
      aria-label="add annotation"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) add.mutate();
      }}
    >
      <strong className="sem-sub">Add</strong>
      <label className="sem-field">
        <span>Entity id, or “project”</span>
        <input aria-label="target" value={target} onChange={(e) => setTarget(e.target.value)} />
      </label>
      <label className="sem-field">
        <span>Kind</span>
        <select
          aria-label="kind"
          value={kind}
          onChange={(e) => setKind(e.target.value as AnnotationKind)}
        >
          <option value="comment">comment</option>
          <option value="instruction">instruction</option>
          <option value="synonym">synonym</option>
        </select>
      </label>
      <p className="muted">{KIND_HELP[kind]}</p>
      {kind === "synonym" && (
        <label className="sem-field">
          <span>The business word</span>
          <input aria-label="term" value={term} onChange={(e) => setTerm(e.target.value)} />
        </label>
      )}
      <label className="sem-field">
        <span>Text</span>
        <textarea aria-label="text" rows={2} value={text} onChange={(e) => setText(e.target.value)} />
      </label>
      {kind === "instruction" && (
        <label className="sem-field">
          <span>Rule (optional JSON; without one the text is a note a model reads as data)</span>
          <textarea
            aria-label="rule"
            rows={2}
            value={ruleText}
            onChange={(e) => setRuleText(e.target.value)}
          />
        </label>
      )}
      {rule.error && <p className="err">{rule.error}</p>}
      {kind !== "comment" && (
        <label className="sem-field">
          <input
            type="checkbox"
            aria-label="shared"
            checked={shared}
            onChange={(e) => setShared(e.target.checked)}
          />{" "}
          Apply to everyone once someone else approves
        </label>
      )}
      {me.mode === "token" && (
        <label className="sem-field">
          <span>Your name (token mode has no identity; recorded as token:&lt;name&gt;)</span>
          <input
            aria-label="author"
            value={reviewer}
            onChange={(e) => setReviewer(e.target.value)}
          />
        </label>
      )}
      <div className="btnrow">
        <button type="submit" disabled={!ready || add.isPending}>
          Add
        </button>
      </div>
      {add.isError && <p className="err">{String(add.error)}</p>}
      {add.isSuccess && <p className="ok-line">Recorded as {add.data.author}.</p>}
    </form>
  );
}

function AnnotationPanel({
  workspace,
  annotation: a,
  me,
  onClose,
}: {
  workspace: string;
  annotation: Annotation;
  me: Me;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [reviewer, setReviewer] = useState(loadReviewer());
  const needsName = me.mode === "token" && !reviewer.trim();
  const act = useMutation({
    mutationFn: (action: "withdraw" | "approve" | "confirm") =>
      apiPost<{ author: string }>(`/workspaces/${workspace}/annotations/${a.id}/${action}`, {
        reviewer: me.mode === "token" ? reviewer : null,
      }),
    onSuccess: () => {
      if (me.mode === "token") saveReviewer(reviewer);
      void qc.invalidateQueries({ queryKey: ["annotations"] });
    },
  });
  const open = a.state !== "withdrawn";
  const needsApproval = a.kind !== "comment" && a.scope === "shared" && !a.approved_by;

  return (
    <aside className="card sem-panel" aria-label="annotation">
      <div className="sem-panel-head">
        <strong className="sem-title">
          {a.kind} on {targetLabel(a.target)}
        </strong>
        <button className="linkish" onClick={onClose}>
          close
        </button>
      </div>
      <p className="sem-summary">
        <span className={stateChip(a.state)}>{a.state}</span> {reach(a)}
      </p>
      <dl className="sem-props">
        <div>
          <dt>about</dt>
          <dd>{a.target}</dd>
        </div>
        {a.term && (
          <div>
            <dt>word</dt>
            <dd>{a.term}</dd>
          </div>
        )}
        <div>
          <dt>by</dt>
          <dd>{a.author}</dd>
        </div>
        {a.rule && (
          <div>
            <dt>rule</dt>
            <dd>
              <code>{JSON.stringify(a.rule)}</code>
            </dd>
          </div>
        )}
      </dl>
      {/* A person's text, shown as text: React escapes it. */}
      <p className="sem-summary">{a.text}</p>
      {open && (mayOperate(me) || mayReview(me)) && (
        <div className="sem-decide">
          {me.mode === "token" && (
            <label className="sem-field">
              <span>Your name (recorded as token:&lt;name&gt;)</span>
              <input
                aria-label="reviewer"
                value={reviewer}
                onChange={(e) => setReviewer(e.target.value)}
              />
            </label>
          )}
          <div className="btnrow">
            {mayReview(me) && needsApproval && a.state === "active" && (
              <button disabled={act.isPending || needsName} onClick={() => act.mutate("approve")}>
                Approve for everyone
              </button>
            )}
            {mayReview(me) && a.state === "stale" && (
              <button disabled={act.isPending || needsName} onClick={() => act.mutate("confirm")}>
                Confirm against the entity as it is now
              </button>
            )}
            {mayOperate(me) && (
              <button disabled={act.isPending || needsName} onClick={() => act.mutate("withdraw")}>
                Withdraw
              </button>
            )}
          </div>
          <p className="muted">
            Only the author can withdraw, and nobody approves their own annotation; EKOS enforces
            both.
          </p>
          {act.isError && <p className="err">{String(act.error)}</p>}
          {act.isSuccess && <p className="ok-line">Recorded as {act.data.author}.</p>}
        </div>
      )}
    </aside>
  );
}
