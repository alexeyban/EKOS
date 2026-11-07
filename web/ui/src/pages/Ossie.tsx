// RFC 0245 — the compiled schema as an Apache Ossie semantic model: a browser over its datasets,
// downloads (YAML / JSON) for BI tools and semantic layers, and the review round trip — an edited
// export's `concepts` / `enum_meanings` entries become review decisions through
// `ekos import ossie`, validated as a dry run first and applied as the signed-in reviewer.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type {
  ImportPlan,
  OssieAiContext,
  OssieDataset,
  OssieExtension,
  OssieJson,
  OssieModel,
} from "../api/types";
import type { Me } from "../WorkspaceShell";
import { PlanView } from "./Linkml";
import { loadReviewer, saveReviewer } from "./semantics-shared";

type Filter = "all" | "hypothesis" | "confirmed";

/** The parsed data of an element's `EKOS` extension, or an empty object. */
export function ekosData(exts: OssieExtension[] | undefined): Record<string, unknown> {
  const e = exts?.find((x) => x.vendor_name === "EKOS");
  if (!e) return {};
  try {
    const v: unknown = JSON.parse(e.data);
    return v && typeof v === "object" ? (v as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

function ctx(c: string | OssieAiContext | undefined): OssieAiContext {
  if (!c) return {};
  return typeof c === "string" ? { instructions: c } : c;
}

/** How many reviewable meanings a dataset carries (concepts + code meanings). */
export function meaningCount(d: OssieDataset): number {
  const concepts = (ekosData(d.custom_extensions).concepts as unknown[] | undefined)?.length ?? 0;
  const codes = (d.fields ?? []).reduce(
    (n, f) =>
      n + ((ekosData(f.custom_extensions).enum_meanings as unknown[] | undefined)?.length ?? 0),
    0,
  );
  return concepts + codes;
}

function download(name: string, text: string, type: string) {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}

export function Ossie({ me }: { me: Me }) {
  const { id = "" } = useParams();
  const [filter, setFilter] = useState<Filter>("all");
  const q = useQuery({
    queryKey: ["ossie", id, filter],
    queryFn: () => api<OssieJson>(`/workspaces/${id}/semantics/ossie?status=${filter}`),
    enabled: id !== "",
  });
  const yamlNow = async () => {
    const y = await api<{ yaml: string }>(`/workspaces/${id}/semantics/ossie/yaml?status=${filter}`);
    download(`${id}.ossie.yaml`, y.yaml, "application/yaml");
  };
  return (
    <>
      <div className="sem-filters lm-bar">
        <select
          aria-label="export status"
          value={filter}
          onChange={(e) => setFilter(e.target.value as Filter)}
        >
          <option value="all">all meanings (each marked with its status)</option>
          <option value="hypothesis">unreviewed meanings only</option>
          <option value="confirmed">confirmed meanings only</option>
        </select>
        <button onClick={() => void yamlNow()} disabled={!q.data?.model}>
          Download .ossie.yaml
        </button>
        <button
          onClick={() =>
            q.data?.model &&
            download(`${id}.ossie.json`, JSON.stringify(q.data.model, null, 2), "application/json")
          }
          disabled={!q.data?.model}
        >
          Download .json
        </button>
        <span className="muted sem-note">
          Apache Ossie spec {q.data?.model?.version ?? "0.2.0.dev0"} — tables are observed facts;
          business meanings carry their review status.
        </span>
      </div>
      {q.isLoading && <p className="muted">loading…</p>}
      {q.isError && <p className="err">{String(q.error)}</p>}
      {q.data?.empty && (
        <section className="card">
          <strong>Nothing to show</strong>
          <p className="muted">{q.data.reason}</p>
        </section>
      )}
      {q.data?.model && <Browser model={q.data.model} />}
      <ImportCard me={me} />
    </>
  );
}

function Browser({ model }: { model: OssieModel }) {
  const [text, setText] = useState("");
  const [sel, setSel] = useState<string | null>(null);
  const needle = text.trim().toLowerCase();
  const shown = useMemo(
    () => model.datasets.filter((d) => !needle || d.name.toLowerCase().includes(needle)),
    [model, needle],
  );
  const current = model.datasets.find((d) => d.name === sel) ?? null;
  return (
    <div className="lm-layout">
      <section className="card lm-tree">
        <strong>{model.name}</strong>
        <p className="muted lm-desc">
          {model.datasets.length} dataset(s), {model.relationships?.length ?? 0} relationship(s)
        </p>
        <input
          aria-label="filter datasets"
          placeholder="filter datasets"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <ul>
          {shown.map((d) => {
            const n = meaningCount(d);
            return (
              <li key={d.name}>
                <button
                  className={sel === d.name ? "lm-node active" : "lm-node"}
                  onClick={() => setSel(d.name)}
                >
                  {d.name}
                  {n > 0 && <span className="chip warn">{n} meaning(s)</span>}
                  {(d.metrics?.length ?? 0) > 0 && (
                    <span className="chip ok">{d.metrics!.length} metric(s)</span>
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      </section>
      <section className="card lm-detail">
        {!current && <p className="muted">Pick a dataset.</p>}
        {current && <DatasetView d={current} model={model} />}
      </section>
    </div>
  );
}

function DatasetView({ d, model }: { d: OssieDataset; model: OssieModel }) {
  const c = ctx(d.ai_context);
  const ext = ekosData(d.custom_extensions);
  const rels = (model.relationships ?? []).filter((r) => r.from === d.name || r.to === d.name);
  return (
    <>
      <div className="sem-item-head">
        <span className="chip">{String(ext.kind ?? "dataset")}</span>
        {typeof ext.origin === "string" && <span className="chip">{ext.origin}</span>}
      </div>
      <h3 className="sem-title">{d.name}</h3>
      <p className="muted">
        source <code>{d.source}</code>
        {d.primary_key && (
          <>
            {" "}
            · key <code>{d.primary_key.join(", ")}</code>
          </>
        )}
      </p>
      {d.description && <p className="lm-pre">{d.description}</p>}
      {c.synonyms && c.synonyms.length > 0 && <p className="muted">also called: {c.synonyms.join(", ")}</p>}
      {c.instructions && <p className="lm-pre">{c.instructions}</p>}
      <strong className="sem-sub">Fields</strong>
      <table className="lm-table">
        <thead>
          <tr>
            <th>field</th>
            <th>type</th>
            <th>description / meanings</th>
          </tr>
        </thead>
        <tbody>
          {(d.fields ?? []).map((f) => {
            const fc = ctx(f.ai_context);
            return (
              <tr key={f.name}>
                <td>
                  <code>{f.name}</code>
                </td>
                <td>{f.datatype ?? <span className="muted">—</span>}</td>
                <td className="muted">
                  {f.description}
                  {fc.instructions && <div>{fc.instructions}</div>}
                  {fc.synonyms && fc.synonyms.length > 0 && <div>also: {fc.synonyms.join(", ")}</div>}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {(d.metrics?.length ?? 0) > 0 && (
        <>
          <strong className="sem-sub">Metrics (from Ossie files in the sources)</strong>
          <table className="lm-table">
            <tbody>
              {d.metrics!.map((m) => (
                <tr key={m.name}>
                  <td>
                    <code>{m.name}</code>
                  </td>
                  <td>
                    <code>{m.expression.sql ?? m.expression.dialects?.[0]?.expression}</code>
                  </td>
                  <td className="muted">{m.description}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
      {rels.length > 0 && (
        <>
          <strong className="sem-sub">Relationships</strong>
          <ul>
            {rels.map((r) => (
              <li key={r.name}>
                <code>
                  {r.from}({r.from_columns.join(", ")}) → {r.to}({r.to_columns.join(", ")})
                </code>
              </li>
            ))}
          </ul>
        </>
      )}
    </>
  );
}

function ImportCard({ me }: { me: Me }) {
  const { id = "" } = useParams();
  const qc = useQueryClient();
  const [text, setText] = useState("");
  const [plan, setPlan] = useState<ImportPlan | null>(null);
  const [dirty, setDirty] = useState(true);
  const [reviewer, setReviewer] = useState(loadReviewer());

  const validate = useMutation({
    mutationFn: () =>
      apiPost<ImportPlan>(`/workspaces/${id}/semantics/ossie/validate`, { yaml: text }),
    onSuccess: (p) => {
      setPlan(p);
      setDirty(false);
    },
  });
  const apply = useMutation({
    mutationFn: () =>
      apiPost<ImportPlan>(`/workspaces/${id}/semantics/ossie/import`, {
        yaml: text,
        reviewer: me.mode === "token" ? reviewer : null,
      }),
    onSuccess: (p) => {
      setPlan(p);
      if (me.mode === "token") saveReviewer(reviewer);
      void qc.invalidateQueries({ queryKey: ["semantics"] });
      void qc.invalidateQueries({ queryKey: ["ossie"] });
      void qc.invalidateQueries({ queryKey: ["linkml"] });
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
      <strong>Import an edited export — edits become review decisions</strong>
      <p className="muted">
        In a downloaded <code>.ossie.yaml</code>, edit the <code>EKOS</code> extensions:
        a concept&apos;s <code>name</code>, <code>description</code> or <code>status</code>, a
        code meaning&apos;s <code>label</code> or <code>status</code> (<code>confirmed</code> /{" "}
        <code>rejected</code>, with a <code>note</code>). Entries are matched by{" "}
        <code>ekos_id</code>. Edits to fields, types and metrics are observed facts and are not
        imported.
      </p>
      <textarea
        aria-label="edited Ossie model"
        className="toml lm-yaml"
        spellCheck={false}
        value={text}
        placeholder="paste or load an edited .ossie.yaml"
        onChange={(e) => {
          setText(e.target.value);
          setDirty(true);
        }}
      />
      {me.role === "write" && me.mode === "token" && (
        <label className="sem-field">
          <span>Reviewer (recorded as token:&lt;name&gt;)</span>
          <input value={reviewer} onChange={(e) => setReviewer(e.target.value)} />
        </label>
      )}
      <div className="btnrow">
        <label className="lm-upload">
          <span>Load file…</span>
          <input
            type="file"
            accept=".yaml,.yml,.json"
            onChange={(e) => upload(e.target.files?.[0])}
          />
        </label>
        <button onClick={() => validate.mutate()} disabled={validate.isPending || !text.trim()}>
          Validate
        </button>
        <button className="save" onClick={() => apply.mutate()} disabled={!canApply}>
          Apply {plan && !dirty ? `${plan.decisions.length} decision(s)` : ""}
        </button>
        {me.role !== "write" && <span className="muted">applying needs the write role</span>}
        {plan && dirty && <span className="muted">edited since validation — validate again</span>}
      </div>
      {validate.isError && <p className="err">{String(validate.error)}</p>}
      {apply.isError && <p className="err">{String(apply.error)}</p>}
      {plan && <PlanView plan={plan} />}
    </section>
  );
}
