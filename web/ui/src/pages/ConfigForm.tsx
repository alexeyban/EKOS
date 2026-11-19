// RFC 0186: every setting of `ekos.toml` as a form, built from the schema the binary prints.
// Changes are applied to the text by the server (comments kept) and land in the editor below,
// where they are validated and saved like a hand edit.
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api, apiPost } from "../api/client";
import { ErrorNote, Skeleton } from "../ui";
import { type Kind, type Schema, kindOf, parseField, resolve, showField, valueAt } from "./config-schema";

type Edits = Record<string, { path: string[]; text: string; kind: Kind }>;

export function ConfigForm({
  id,
  raw,
  values,
  onApplied,
}: {
  id: string;
  raw: string;
  values: Record<string, unknown>;
  onApplied: (raw: string) => void;
}) {
  const schema = useQuery({ queryKey: ["config-schema"], queryFn: () => api<Schema>("/config-schema"), staleTime: Infinity });
  const [edits, setEdits] = useState<Edits>({});
  const apply = useMutation({
    mutationFn: () =>
      apiPost<{ raw: string }>(`/workspaces/${id}/config/edit`, {
        raw,
        changes: Object.values(edits).map((e) => ({ path: e.path, value: parseField(e.kind, e.text) })),
      }),
    onSuccess: (r) => {
      onApplied(r.raw);
      setEdits({});
    },
  });

  if (schema.isError) return <ErrorNote error={schema.error} what="Reading the settings schema" />;
  if (!schema.data)
    return (
      <section className="card">
        <Skeleton rows={5} label="loading the settings" />
      </section>
    );
  const root = schema.data;
  const sections = Object.entries(root.properties ?? {});
  const count = Object.keys(edits).length;

  const field = (path: string[], node: Schema) => {
    const kind = kindOf(root, node);
    const key = path.join(".");
    const resolved = resolve(root, node);
    if (kind.kind === "section") {
      return (
        <details key={key} className="fm-section">
          <summary>
            <code>{path[path.length - 1]}</code>
          </summary>
          {resolved.description && <p className="fm-hint">{resolved.description}</p>}
          {children(path, kind.schema)}
        </details>
      );
    }
    const current = valueAt(values, path);
    const text = edits[key]?.text ?? showField(kind, current);
    const set = (t: string) => setEdits({ ...edits, [key]: { path, text: t, kind } });
    return (
      <div key={key} className="fm-field">
        <label className="fm-label" htmlFor={`cfg-${key}`}>
          <code>{path[path.length - 1]}</code>
          {current !== undefined && <span className="chip fm-chip">set</span>}
          {edits[key] && <span className="chip warn fm-chip">changed</span>}
        </label>
        <FieldControl id={`cfg-${key}`} kind={kind} text={text} hint={defaultHint(resolved.default)} onChange={set} />
        {resolved.description && <div className="fm-hint">{resolved.description}</div>}
      </div>
    );
  };

  // A section's plain settings in a grid, then its subsections, each folded.
  const children = (path: string[], schemaNode: Schema) => {
    const entries = Object.entries(schemaNode.properties ?? {});
    const isSection = ([, child]: [string, Schema]) => kindOf(root, child).kind === "section";
    const plain = entries.filter((e) => !isSection(e));
    const nested = entries.filter(isSection);
    return (
      <>
        {plain.length > 0 && <div className="fm-grid">{plain.map(([k, child]) => field([...path, k], child))}</div>}
        {nested.map(([k, child]) => field([...path, k], child))}
      </>
    );
  };

  return (
    <section className="card" aria-labelledby="cfg-settings">
      <h2 className="card-label" id="cfg-settings">
        Settings
      </h2>
      <p className="fm-note">
        Every setting of this project, from the schema of the running EKOS. Empty means not set (the default applies).
        "Apply to the text" puts the changes into the file below, keeping its comments; validate and save it there.
      </p>
      {sections.map(([name, node]) => {
        const kind = kindOf(root, node);
        const resolved = resolve(root, node);
        return (
          <details key={name} className="fm-section">
            <summary>
              <code>[{name}]</code>
            </summary>
            {resolved.description && <p className="fm-hint">{resolved.description}</p>}
            {kind.kind === "section" ? (
              children([name], kind.schema)
            ) : (
              <p className="fm-hint">edit this one in the text below</p>
            )}
          </details>
        );
      })}
      <div className="fm-actions">
        <button className="primary" disabled={count === 0 || apply.isPending} onClick={() => apply.mutate()}>
          Apply {count || ""} change{count === 1 ? "" : "s"} to the text
        </button>
        {count > 0 && <button onClick={() => setEdits({})}>Discard</button>}
      </div>
      {apply.isError && <ErrorNote error={apply.error} what="Applying the changes" />}
    </section>
  );
}

/** "default: …" for a scalar default, nothing otherwise. */
function defaultHint(def: unknown): string {
  return def !== undefined && def !== null && typeof def !== "object" ? `default: ${String(def)}` : "";
}

/** The input for one setting, by its kind; a complex value is edited in the text instead. */
function FieldControl({
  id,
  kind,
  text,
  hint,
  onChange,
}: {
  id: string;
  kind: Kind;
  text: string;
  hint: string;
  onChange: (text: string) => void;
}) {
  switch (kind.kind) {
    case "complex":
      return <span className="fm-hint">edit this one in the text below</span>;
    case "bool":
      return (
        <select id={id} value={text} onChange={(e) => onChange(e.target.value)}>
          <option value="">{hint || "not set"}</option>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      );
    case "choice":
      return (
        <select id={id} value={text} onChange={(e) => onChange(e.target.value)}>
          <option value="">{hint || "not set"}</option>
          {kind.options.map((o) => (
            <option key={o}>{o}</option>
          ))}
        </select>
      );
    case "list":
      return <textarea id={id} placeholder="one per line" value={text} onChange={(e) => onChange(e.target.value)} />;
    default:
      return (
        <input
          id={id}
          type={kind.kind === "number" ? "number" : "text"}
          placeholder={hint}
          value={text}
          onChange={(e) => onChange(e.target.value)}
        />
      );
  }
}
