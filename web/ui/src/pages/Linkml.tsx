// RFC 0170 — the LinkML schema EKOS exports, as a browser (viewer) and as YAML (editor).
//
// The viewer reads `ekos export linkml --json`. The editor edits the YAML and round-trips it
// through `ekos import linkml`: Validate is a dry run that shows the review decisions the edits
// stand for (renames, descriptions, code labels, `ekos_status: confirmed|rejected`); Apply records
// them, all or nothing, as the signed-in reviewer. The YAML is never stored as a second source of
// truth — the ledger is.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type {
  ImportPlan,
  LinkmlClass,
  LinkmlEnum,
  LinkmlJson,
  LinkmlSchema,
  LinkmlYaml,
} from "../api/types";
import type { Me } from "../WorkspaceShell";
import { sideBySide } from "./line-diff";
import { SemanticsItemPanel } from "./SemanticsItemPanel";
import { annotation, loadReviewer, saveReviewer, statusChip } from "./semantics-shared";

type Filter = "all" | "hypothesis" | "confirmed";
type Selection = { section: "class" | "enum"; name: string } | null;

export function Linkml({ me }: { me: Me }) {
  const [mode, setMode] = useState<"view" | "edit">("view");
  const [filter, setFilter] = useState<Filter>("all");
  return (
    <>
      <div className="sem-filters lm-bar">
        <button
          className={mode === "view" ? "pill active" : "pill"}
          onClick={() => setMode("view")}
        >
          Viewer
        </button>
        <button
          className={mode === "edit" ? "pill active" : "pill"}
          onClick={() => setMode("edit")}
        >
          YAML editor
        </button>
        <select
          aria-label="export status"
          value={filter}
          onChange={(e) => setFilter(e.target.value as Filter)}
        >
          <option value="all">everything (annotated with status)</option>
          <option value="hypothesis">hypotheses only</option>
          <option value="confirmed">confirmed only</option>
        </select>
      </div>
      {mode === "view" ? <Viewer me={me} filter={filter} /> : <Editor me={me} filter={filter} />}
    </>
  );
}

// ── viewer ───────────────────────────────────────────────────────────────────────────────────

/** Sections of the schema tree: concepts (classes EKOS recovered, with an `ekos_id`), the tables
 * they refine, and the enums. */
export function sections(schema: LinkmlSchema) {
  const classes = Object.entries(schema.classes ?? {});
  return {
    concepts: classes.filter(([, c]) => annotation(c.annotations, "ekos_id")),
    tables: classes.filter(([, c]) => !annotation(c.annotations, "ekos_id")),
    enums: Object.entries(schema.enums ?? {}),
  };
}

function Viewer({ me, filter }: { me: Me; filter: Filter }) {
  const { id = "" } = useParams();
  const [sel, setSel] = useState<Selection>(null);
  const [review, setReview] = useState<string | null>(null);
  const [text, setText] = useState("");
  const q = useQuery({
    queryKey: ["linkml", id, filter],
    queryFn: () => api<LinkmlJson>(`/workspaces/${id}/semantics/linkml?status=${filter}`),
    enabled: id !== "",
  });
  const schema = q.data?.schema;
  const sec = useMemo(() => (schema ? sections(schema) : null), [schema]);
  const needle = text.trim().toLowerCase();
  const match = (n: string) => !needle || n.toLowerCase().includes(needle);

  if (q.isLoading) return <p className="muted">loading…</p>;
  if (q.isError) return <p className="err">{String(q.error)}</p>;
  if (q.data?.empty || !schema || !sec)
    return (
      <section className="card">
        <strong>Nothing to show</strong>
        <p className="muted">{q.data?.reason}</p>
      </section>
    );

  return (
    <div className={review ? "lm-layout with-panel" : "lm-layout"}>
      <section className="card lm-tree">
        <strong title={schema.id}>{schema.name}</strong>
        <p className="muted lm-desc">{schema.description}</p>
        <input
          aria-label="filter schema"
          placeholder="filter elements"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <TreeGroup
          title="Concepts"
          entries={sec.concepts.filter(([n]) => match(n))}
          selected={sel}
          section="class"
          onSelect={setSel}
          status={(c) => annotation((c as LinkmlClass).annotations, "ekos_status")}
        />
        <TreeGroup
          title="Enums"
          entries={sec.enums.filter(([n]) => match(n))}
          selected={sel}
          section="enum"
          onSelect={setSel}
        />
        <TreeGroup
          title="Tables"
          entries={sec.tables.filter(([n]) => match(n))}
          selected={sel}
          section="class"
          onSelect={setSel}
        />
      </section>
      <section className="card lm-detail">
        {!sel && <p className="muted">Pick a concept, enum or table.</p>}
        {sel?.section === "class" && schema.classes?.[sel.name] && (
          <ClassView
            name={sel.name}
            cls={schema.classes[sel.name]}
            onOpen={(n) => setSel({ section: n.startsWith("enum:") ? "enum" : "class", name: n.replace(/^enum:/, "") })}
            onReview={setReview}
          />
        )}
        {sel?.section === "enum" && schema.enums?.[sel.name] && (
          <EnumView name={sel.name} en={schema.enums[sel.name]} onReview={setReview} />
        )}
      </section>
      {review && (
        <SemanticsItemPanel
          workspace={id}
          itemId={review}
          me={me}
          onClose={() => setReview(null)}
        />
      )}
    </div>
  );
}

function TreeGroup({
  title,
  entries,
  selected,
  section,
  onSelect,
  status,
}: {
  title: string;
  entries: [string, unknown][];
  selected: Selection;
  section: "class" | "enum";
  onSelect: (s: Selection) => void;
  status?: (v: unknown) => string | undefined;
}) {
  const [open, setOpen] = useState(title !== "Tables");
  return (
    <div className="lm-group">
      <button className="linkish lm-group-head" onClick={() => setOpen(!open)}>
        {open ? "▾" : "▸"} {title} ({entries.length})
      </button>
      {open && (
        <ul>
          {entries.map(([n, v]) => {
            const st = status?.(v);
            const active = selected?.section === section && selected.name === n;
            return (
              <li key={n}>
                <button
                  className={active ? "lm-node active" : "lm-node"}
                  onClick={() => onSelect({ section, name: n })}
                >
                  {n}
                  {st && <span className={statusChip(st)}>{st.replace("_", " ")}</span>}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function Annotations({ a }: { a: LinkmlClass["annotations"] }) {
  const rows = Object.entries(a ?? {}).filter(([k]) => k !== "ekos_id");
  if (rows.length === 0) return null;
  return (
    <dl className="sem-props">
      {rows.map(([k]) => (
        <div key={k}>
          <dt>{k.replace(/^ekos_/, "").replace(/_/g, " ")}</dt>
          <dd>{annotation(a, k)}</dd>
        </div>
      ))}
    </dl>
  );
}

function ClassView({
  name,
  cls,
  onOpen,
  onReview,
}: {
  name: string;
  cls: LinkmlClass;
  onOpen: (name: string) => void;
  onReview: (id: string) => void;
}) {
  const id = annotation(cls.annotations, "ekos_id");
  const st = annotation(cls.annotations, "ekos_status");
  return (
    <>
      <div className="sem-item-head">
        <span className="chip">{id ? "concept" : "table"}</span>
        {st && <span className={statusChip(st)}>{st.replace("_", " ")}</span>}
        {id && (
          <button className="lm-review" onClick={() => onReview(id)}>
            Review…
          </button>
        )}
      </div>
      <h3 className="sem-title">{name}</h3>
      {cls.is_a && (
        <p className="muted">
          is_a{" "}
          <button className="linkish" onClick={() => onOpen(cls.is_a!)}>
            {cls.is_a}
          </button>
        </p>
      )}
      {cls.description && <p className="lm-pre">{cls.description}</p>}
      {cls.comments?.map((c, n) => (
        <p key={n} className={c.startsWith("GAP") || c.startsWith("CONFLICT") ? "warn-line" : "muted"}>
          {c}
        </p>
      ))}
      <Annotations a={cls.annotations} />
      {cls.attributes && (
        <SlotTable title="Attributes" slots={cls.attributes} onOpen={onOpen} onReview={onReview} />
      )}
      {cls.slot_usage && (
        <SlotTable
          title="Constraints (slot_usage)"
          slots={cls.slot_usage}
          onOpen={onOpen}
          onReview={onReview}
        />
      )}
    </>
  );
}

function SlotTable({
  title,
  slots,
  onOpen,
  onReview,
}: {
  title: string;
  slots: NonNullable<LinkmlClass["attributes"]>;
  onOpen: (name: string) => void;
  onReview: (id: string) => void;
}) {
  return (
    <>
      <strong className="sem-sub">{title}</strong>
      <table className="lm-table">
        <thead>
          <tr>
            <th>slot</th>
            <th>range</th>
            <th>rules</th>
            <th>description / evidence</th>
          </tr>
        </thead>
        <tbody>
          {Object.entries(slots).map(([n, s]) => {
            const en = annotation(s.annotations, "ekos_enum");
            const sid = annotation(s.annotations, "ekos_id");
            const rules = [
              s.identifier && "identifier",
              s.required && "required",
              s.minimum_value !== undefined && `≥ ${s.minimum_value}`,
              s.maximum_value !== undefined && `≤ ${s.maximum_value}`,
              s.pattern && `pattern ${s.pattern}`,
            ].filter(Boolean);
            return (
              <tr key={n}>
                <td>
                  <code>{n}</code>
                </td>
                <td>
                  {s.range ?? annotation(s.annotations, "ekos_sql_type")}
                  {en && (
                    <>
                      {" "}
                      <button className="linkish" onClick={() => onOpen(`enum:${en}`)}>
                        codes: {en}
                      </button>
                    </>
                  )}
                </td>
                <td>{rules.join(", ")}</td>
                <td className="muted">
                  {s.description ??
                    annotation(s.annotations, "ekos_constraint") ??
                    annotation(s.annotations, "ekos_sql_type")}
                  {sid && (
                    <>
                      {" "}
                      <button className="linkish" onClick={() => onReview(sid)}>
                        review
                      </button>
                    </>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </>
  );
}

function EnumView({
  name,
  en,
  onReview,
}: {
  name: string;
  en: LinkmlEnum;
  onReview: (id: string) => void;
}) {
  return (
    <>
      <div className="sem-item-head">
        <span className="chip">enum</span>
      </div>
      <h3 className="sem-title">{name}</h3>
      <p className="muted">{en.description}</p>
      <table className="lm-table">
        <thead>
          <tr>
            <th>code</th>
            <th>meaning</th>
            <th>status</th>
            <th>source</th>
            <th>used</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {Object.entries(en.permissible_values ?? {}).map(([code, pv]) => {
            const st = annotation(pv.annotations, "ekos_status") ?? "";
            const gap = annotation(pv.annotations, "ekos_gap");
            const id = annotation(pv.annotations, "ekos_id");
            return (
              <tr key={code}>
                <td>
                  <code>{code}</code>
                </td>
                <td>{pv.description ?? <span className="warn-line">unknown</span>}</td>
                <td>
                  <span className={statusChip(st)}>{st.replace("_", " ")}</span>
                </td>
                <td className="muted">{annotation(pv.annotations, "ekos_source")}</td>
                <td className="muted">{annotation(pv.annotations, "ekos_usage_sites")}</td>
                <td>
                  {id && (
                    <button className="linkish" onClick={() => onReview(id)} title={gap}>
                      review
                    </button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </>
  );
}

// ── editor ───────────────────────────────────────────────────────────────────────────────────

function Editor({ me, filter }: { me: Me; filter: Filter }) {
  const { id = "" } = useParams();
  const qc = useQueryClient();
  const src = useQuery({
    queryKey: ["linkml-yaml", id, filter],
    queryFn: () => api<LinkmlYaml>(`/workspaces/${id}/semantics/linkml/yaml?status=${filter}`),
    enabled: id !== "",
  });
  const [text, setText] = useState("");
  const [plan, setPlan] = useState<ImportPlan | null>(null);
  const [dirty, setDirty] = useState(true);
  const [reviewer, setReviewer] = useState(loadReviewer());
  const [pane, setPane] = useState<"edit" | "diff">("edit");
  const baseline = src.data?.yaml ?? "";
  const diff = useMemo(
    () => (pane === "diff" ? sideBySide(baseline, text) : null),
    [pane, baseline, text],
  );

  useEffect(() => {
    if (src.data) {
      setText(src.data.yaml);
      setPlan(null);
      setDirty(true);
    }
  }, [src.data]);

  const validate = useMutation({
    mutationFn: () =>
      apiPost<ImportPlan>(`/workspaces/${id}/semantics/linkml/validate`, { yaml: text }),
    onSuccess: (p) => {
      setPlan(p);
      setDirty(false);
    },
  });
  const apply = useMutation({
    mutationFn: () =>
      apiPost<ImportPlan>(`/workspaces/${id}/semantics/linkml/import`, {
        yaml: text,
        reviewer: me.mode === "token" ? reviewer : null,
      }),
    onSuccess: (p) => {
      setPlan(p);
      if (me.mode === "token") saveReviewer(reviewer);
      void qc.invalidateQueries({ queryKey: ["semantics"] });
      void qc.invalidateQueries({ queryKey: ["semantics-gaps"] });
      void qc.invalidateQueries({ queryKey: ["linkml"] });
      if (p.applied > 0) void qc.invalidateQueries({ queryKey: ["linkml-yaml"] });
    },
  });

  const canApply =
    me.role === "write" &&
    plan !== null &&
    !dirty &&
    plan.errors.length === 0 &&
    plan.decisions.length > 0 &&
    plan.applied === 0 &&
    !(me.mode === "token" && !reviewer.trim()) &&
    !apply.isPending;

  const download = () => {
    const url = URL.createObjectURL(new Blob([text], { type: "application/yaml" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = `${id}-semantics.linkml.yaml`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const upload = (file: File | undefined) => {
    if (!file) return;
    void file.text().then((t) => {
      setText(t);
      setDirty(true);
      setPlan(null);
    });
  };

  return (
    <section className="card">
      <strong>Edit the schema — edits become review decisions</strong>
      <p className="muted">
        Rename a concept class, rewrite a <code>description</code>, relabel a code (a permissible
        value&apos;s <code>description</code>), or set <code>ekos_status: confirmed</code> /{" "}
        <code>rejected</code> (with an <code>ekos_review_note</code>). Elements are matched by{" "}
        <code>ekos_id</code>, so keep those annotations. New classes and deletions are not
        imported: EKOS records decisions about what it recovered.
      </p>
      {src.isError && <p className="err">{String(src.error)}</p>}
      {src.data?.empty && <p className="muted">{src.data.reason}</p>}
      <div className="sem-filters">
        <button
          className={pane === "edit" ? "pill active" : "pill"}
          onClick={() => setPane("edit")}
        >
          Edit
        </button>
        <button
          className={pane === "diff" ? "pill active" : "pill"}
          onClick={() => setPane("diff")}
        >
          Diff vs current export{text !== baseline ? " •" : ""}
        </button>
      </div>
      {pane === "diff" && diff && <YamlDiff rows={diff.rows} changes={diff.changes} />}
      <textarea
        hidden={pane !== "edit"}
        className="toml lm-yaml"
        spellCheck={false}
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setDirty(true);
        }}
        onKeyDown={(e) => {
          // A tab is two spaces: YAML forbids tab indentation.
          if (e.key === "Tab") {
            e.preventDefault();
            const t = e.currentTarget;
            const { selectionStart: a, selectionEnd: b } = t;
            const next = `${text.slice(0, a)}  ${text.slice(b)}`;
            setText(next);
            setDirty(true);
            requestAnimationFrame(() => t.setSelectionRange(a + 2, a + 2));
          }
        }}
      />
      {me.role === "write" && me.mode === "token" && (
        <label className="sem-field">
          Reviewer (recorded as token:&lt;name&gt;)
          <input value={reviewer} onChange={(e) => setReviewer(e.target.value)} />
        </label>
      )}
      <div className="btnrow">
        <button onClick={() => validate.mutate()} disabled={validate.isPending || !text.trim()}>
          Validate
        </button>
        <button className="save" onClick={() => apply.mutate()} disabled={!canApply}>
          Apply {plan && !dirty ? `${plan.decisions.length} decision(s)` : ""}
        </button>
        <button onClick={() => void src.refetch()}>Reset</button>
        <button onClick={download} disabled={!text}>
          Download .yaml
        </button>
        <label className="lm-upload">
          Load file…
          <input
            type="file"
            accept=".yaml,.yml"
            onChange={(e) => upload(e.target.files?.[0])}
          />
        </label>
        {me.role !== "write" && <span className="muted">applying needs the write role</span>}
        {plan && dirty && <span className="muted">edited since validation — validate again</span>}
      </div>
      {validate.isError && <p className="err">{String(validate.error)}</p>}
      {apply.isError && <p className="err">{String(apply.error)}</p>}
      {plan && <PlanView plan={plan} />}
    </section>
  );
}

function PlanView({ plan }: { plan: ImportPlan }) {
  return (
    <div className="findings">
      {plan.applied > 0 && (
        <p className="ok-line">Applied {plan.applied} decision(s) to the ledger.</p>
      )}
      {plan.errors.map((e, n) => (
        <p key={`e${n}`} className="err">
          {e}
        </p>
      ))}
      {plan.warnings.map((w, n) => (
        <p key={`w${n}`} className="warn-line">
          {w}
        </p>
      ))}
      {plan.decisions.length === 0 && plan.errors.length === 0 && (
        <p className="muted">No decisions: the file matches the ledger.</p>
      )}
      {plan.decisions.length > 0 && (
        <table className="lm-table">
          <thead>
            <tr>
              <th>decision</th>
              <th>element</th>
              <th>changes</th>
              <th>note</th>
            </tr>
          </thead>
          <tbody>
            {plan.decisions.map((d) => (
              <tr key={d.id}>
                <td>
                  <span
                    className={
                      d.action === "reject" ? "chip bad" : d.action === "edit" ? "chip warn" : "chip ok"
                    }
                  >
                    {d.action}
                  </span>
                </td>
                <td>
                  <code>{d.path}</code>
                </td>
                <td>
                  {Object.entries(d.changes).map(([k, [a, b]]) => (
                    <div key={k}>
                      {k}: <s className="muted">{a ?? "∅"}</s> → {b ?? "∅"}
                    </div>
                  ))}
                </td>
                <td className="muted">{d.note}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

/** Side by side: the current export on the left, the edited text on the right. */
function YamlDiff({ rows, changes }: { rows: ReturnType<typeof sideBySide>["rows"]; changes: number }) {
  if (changes === 0) return <p className="muted">No edits: the text matches the current export.</p>;
  return (
    <>
      <p className="muted">
        {changes} changed block(s). Left: current export · right: your edits.
      </p>
      <div className="lm-diff" role="table" aria-label="diff vs current export">
        {rows.map((r, i) =>
          r.kind === "gap" ? (
            <div key={i} className="lm-diff-gap" role="row">
              ⋯ {r.hidden} unchanged line(s)
            </div>
          ) : (
            <div key={i} className={`lm-diff-row ${r.kind}`} role="row">
              <span className={r.left && r.kind !== "same" ? "lm-ln lm-old-mark" : "lm-ln"}>
                {r.left?.n ?? ""}
              </span>
              <code className={r.left && r.kind !== "same" ? "lm-old" : ""}>{r.left?.text ?? ""}</code>
              <span className={r.right && r.kind !== "same" ? "lm-ln lm-new-mark" : "lm-ln"}>
                {r.right?.n ?? ""}
              </span>
              <code className={r.right && r.kind !== "same" ? "lm-new" : ""}>{r.right?.text ?? ""}</code>
            </div>
          ),
        )}
      </div>
    </>
  );
}
