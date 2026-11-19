// RFC 0247 — what people wrote about an entity: comments, instructions and synonyms.
//
// Everything written here goes through the console API to the human-only `ekos annotate …`; the
// MCP surface can read annotations but never write one. A model's output is never an annotation.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { Annotation, AnnotationKind } from "../api/types";
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";
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
import { DetailPanel, PickRow, ReviewerField, SplitView } from "./decide-kit";
import { loadReviewer, saveReviewer } from "./semantics-shared";

/** Whether an annotation is listed under the chosen state filter. */
function inState(a: Annotation, state: string): boolean {
  if (state === "all") return true;
  if (state === "current") return a.state !== "withdrawn";
  return a.state === state;
}

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
  const shown = annotationOrder((list.data ?? []).filter((a) => inState(a, state)));
  const current = shown.find((a) => a.id === selected) ?? null;

  return (
    <div className="dk-page">
      <PageHead
        title="Notes"
        sub="What people wrote about an entity: comments, instructions and synonyms. Written by people, never by a model; a note goes stale when the entity it describes changes (RFC 0247)."
      />
      {mayOperate(me) && (
        <section className="card" aria-label="new note">
          <AddForm workspace={id} me={me} title="Add a note" />
        </section>
      )}
      <SplitView
        panel={
          current && (
            <AnnotationPanel
              key={current.id}
              workspace={id}
              annotation={current}
              me={me}
              onClose={() => setSelected(null)}
            />
          )
        }
      >
        <section className="card dk-list" aria-label="notes">
          <div className="dk-toolbar">
            <select aria-label="state" value={state} onChange={(e) => setState(e.target.value)}>
              <option value="current">active and stale</option>
              <option value="active">active only</option>
              <option value="stale">stale only</option>
              <option value="withdrawn">withdrawn</option>
              <option value="all">all</option>
            </select>
            <span className="dk-count">{shown.length} annotation(s)</span>
          </div>
          <QueryView
            q={list}
            what="notes"
            rows={5}
            isEmpty={(d) => d.length === 0}
            empty={
              <Empty title="No notes yet" icon="notes">
                Add a comment, an instruction or a synonym to an entity
                {mayOperate(me) ? " with the form above." : "; it needs the operator role."}
              </Empty>
            }
          >
            {() =>
              shown.length === 0 ? (
                <p className="muted">Nothing in this state.</p>
              ) : (
                <ul className="dk-rows">
                  {shown.map((a) => (
                    <PickRow
                      key={a.id}
                      selected={selected === a.id}
                      onPick={() => setSelected(a.id)}
                      title={
                        <>
                          {a.term ? `“${a.term}” → ` : ""}
                          {targetLabel(a.target)}
                        </>
                      }
                      before={<span className="chip">{a.kind}</span>}
                      after={<span className={stateChip(a.state)}>{a.state}</span>}
                      sub={a.text}
                    />
                  ))}
                </ul>
              )
            }
          </QueryView>
        </section>
      </SplitView>
    </div>
  );
}

/** The add form. RFC 0257: a step panel passes `fixedTarget` (the step's node id) and `title`. */
export function AddForm({
  workspace,
  me,
  fixedTarget,
  title = "Add",
}: {
  workspace: string;
  me: Me;
  fixedTarget?: string;
  title?: string;
}) {
  const qc = useQueryClient();
  const [typedTarget, setTarget] = useState("");
  const target = fixedTarget ?? typedTarget;
  const [kind, setKind] = useState<AnnotationKind>("comment");
  const [text, setText] = useState("");
  const [term, setTerm] = useState("");
  const [ruleText, setRuleText] = useState("");
  const [shared, setShared] = useState(false);
  const [reviewer, setReviewer] = useState(loadReviewer());
  const rule = parseRule(ruleText);
  const needsName = me.mode !== "oidc" && !reviewer.trim();
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
        reviewer: me.mode !== "oidc" ? reviewer : null,
      }),
    onSuccess: () => {
      if (me.mode !== "oidc") saveReviewer(reviewer);
      setText("");
      setTerm("");
      setRuleText("");
      void qc.invalidateQueries({ queryKey: ["annotations"] });
    },
  });

  return (
    <form
      className="dk-form"
      aria-label="add annotation"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready) add.mutate();
      }}
    >
      <h4 className="dk-sub">{title}</h4>
      {fixedTarget === undefined && (
        <label className="dk-field">
          <span>Entity id, or “project”</span>
          <input aria-label="target" value={target} onChange={(e) => setTarget(e.target.value)} />
        </label>
      )}
      <label className="dk-field">
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
      <p className="muted small dk-help">{KIND_HELP[kind]}</p>
      {kind === "synonym" && (
        <label className="dk-field">
          <span>The business word</span>
          <input aria-label="term" value={term} onChange={(e) => setTerm(e.target.value)} />
        </label>
      )}
      <label className="dk-field">
        <span>Text</span>
        <textarea aria-label="text" rows={2} value={text} onChange={(e) => setText(e.target.value)} />
      </label>
      {kind === "instruction" && (
        <label className="dk-field">
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
        <label className="dk-check">
          <input
            type="checkbox"
            aria-label="shared"
            checked={shared}
            onChange={(e) => setShared(e.target.checked)}
          />{" "}
          Apply to everyone once someone else approves
        </label>
      )}
      {me.mode !== "oidc" && (
        <ReviewerField who="Your name" ariaLabel="author" value={reviewer} onChange={setReviewer} />
      )}
      <div className="dk-actions">
        <button type="submit" disabled={!ready || add.isPending}>
          Add
        </button>
      </div>
      {add.isError && <ErrorNote error={add.error} />}
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
  const needsName = me.mode !== "oidc" && !reviewer.trim();
  const act = useMutation({
    mutationFn: (action: "withdraw" | "approve" | "confirm") =>
      apiPost<{ author: string }>(`/workspaces/${workspace}/annotations/${a.id}/${action}`, {
        reviewer: me.mode !== "oidc" ? reviewer : null,
      }),
    onSuccess: () => {
      if (me.mode !== "oidc") saveReviewer(reviewer);
      void qc.invalidateQueries({ queryKey: ["annotations"] });
    },
  });
  const open = a.state !== "withdrawn";
  const needsApproval = a.kind !== "comment" && a.scope === "shared" && !a.approved_by;

  return (
    <DetailPanel
      label="annotation"
      title={
        <h3 className="dk-title">
          {a.kind} on {targetLabel(a.target)}
        </h3>
      }
      onClose={onClose}
    >
      <p className="dk-summary">
        <span className={stateChip(a.state)}>{a.state}</span> {reach(a)}
      </p>
      <dl className="dk-props">
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
      <p className="dk-quote">{a.text}</p>
      {open && (mayOperate(me) || mayReview(me)) && (
        <section className="dk-section" aria-label="decision">
          {me.mode !== "oidc" && (
            <ReviewerField
              who="Your name"
              ariaLabel="reviewer"
              value={reviewer}
              onChange={setReviewer}
            />
          )}
          <div className="dk-actions">
            {mayReview(me) && needsApproval && a.state === "active" && (
              <button
                type="button"
                className="primary"
                disabled={act.isPending || needsName}
                onClick={() => act.mutate("approve")}
              >
                Approve for everyone
              </button>
            )}
            {mayReview(me) && a.state === "stale" && (
              <button
                type="button"
                className="primary"
                disabled={act.isPending || needsName}
                onClick={() => act.mutate("confirm")}
              >
                Confirm against the entity as it is now
              </button>
            )}
            {mayOperate(me) && (
              <button
                type="button"
                className="dk-danger"
                disabled={act.isPending || needsName}
                onClick={() => act.mutate("withdraw")}
              >
                Withdraw
              </button>
            )}
          </div>
          <p className="muted small">
            Only the author can withdraw, and nobody approves their own annotation; EKOS enforces
            both.
          </p>
          {act.isError && <ErrorNote error={act.error} />}
          {act.isSuccess && <p className="ok-line">Recorded as {act.data.author}.</p>}
        </section>
      )}
    </DetailPanel>
  );
}
