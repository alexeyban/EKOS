// RFC 0186: every setting of `ekos.toml` as a form, built from the schema the binary prints.
// Changes are applied to the text by the server (comments kept) and land in the editor below,
// where they are validated and saved like a hand edit.
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api, apiPost } from "../api/client";
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

  if (schema.isError) return <p className="err">{String(schema.error)}</p>;
  if (!schema.data) return <p className="muted">loading the settings…</p>;
  const root = schema.data;
  const sections = Object.entries(root.properties ?? {});
  const count = Object.keys(edits).length;

  const field = (path: string[], node: Schema) => {
    const kind = kindOf(root, node);
    const key = path.join(".");
    const current = valueAt(values, path);
    const resolved = resolve(root, node);
    if (kind.kind === "section") {
      return (
        <details key={key} className="cfg-sub">
          <summary>
            <code>{path[path.length - 1]}</code>
          </summary>
          {resolved.description && <p className="muted small">{resolved.description}</p>}
          {Object.entries(kind.schema.properties ?? {}).map(([k, child]) => field([...path, k], child))}
        </details>
      );
    }
    const text = edits[key]?.text ?? showField(kind, current);
    const set = (t: string) => setEdits({ ...edits, [key]: { path, text: t, kind } });
    const def = resolved.default;
    const hint = def !== undefined && def !== null && typeof def !== "object" ? `default: ${String(def)}` : "";
    return (
      <div key={key} className="cfg-field">
        <label htmlFor={`cfg-${key}`}>
          <code>{path[path.length - 1]}</code>
          {current !== undefined && <span className="chip small">set</span>}
          {edits[key] && <span className="chip warn small">changed</span>}
        </label>
        {kind.kind === "complex" ? (
          <span className="muted small">edit this one in the text below</span>
        ) : kind.kind === "bool" ? (
          <select id={`cfg-${key}`} value={text} onChange={(e) => set(e.target.value)}>
            <option value="">{hint || "not set"}</option>
            <option value="true">true</option>
            <option value="false">false</option>
          </select>
        ) : kind.kind === "choice" ? (
          <select id={`cfg-${key}`} value={text} onChange={(e) => set(e.target.value)}>
            <option value="">{hint || "not set"}</option>
            {kind.options.map((o) => (
              <option key={o}>{o}</option>
            ))}
          </select>
        ) : kind.kind === "list" ? (
          <textarea id={`cfg-${key}`} placeholder="one per line" value={text} onChange={(e) => set(e.target.value)} />
        ) : (
          <input
            id={`cfg-${key}`}
            type={kind.kind === "number" ? "number" : "text"}
            placeholder={hint}
            value={text}
            onChange={(e) => set(e.target.value)}
          />
        )}
        {resolved.description && <div className="muted small">{resolved.description}</div>}
      </div>
    );
  };

  return (
    <section className="card">
      <strong>Settings</strong>
      <p className="muted">
        Every setting of this project, from the schema of the running EKOS. Empty means not set (the default applies).
        "Apply to the text" puts the changes into the file below, keeping its comments; validate and save it there.
      </p>
      {sections.map(([name, node]) => {
        const kind = kindOf(root, node);
        const resolved = resolve(root, node);
        return (
          <details key={name} className="cfg-section">
            <summary>
              <code>[{name}]</code>
            </summary>
            {resolved.description && <p className="muted small">{resolved.description}</p>}
            {kind.kind === "section" ? (
              Object.entries(kind.schema.properties ?? {}).map(([k, child]) => field([name, k], child))
            ) : (
              <p className="muted small">edit this one in the text below</p>
            )}
          </details>
        );
      })}
      <div className="btnrow">
        <button disabled={count === 0 || apply.isPending} onClick={() => apply.mutate()}>
          Apply {count || ""} change{count === 1 ? "" : "s"} to the text
        </button>
        {count > 0 && (
          <button className="linkish" onClick={() => setEdits({})}>
            discard
          </button>
        )}
      </div>
      {apply.isError && <p className="err">{String(apply.error)}</p>}
    </section>
  );
}
