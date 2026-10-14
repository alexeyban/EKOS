// RFC 0184: the console's own settings (console.toml) and the keychain, for platform owners.
// A value set by the environment is shown and cannot be changed here; a secret is never shown,
// only whether it is set and where. Every change applies at the next start.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, apiDelete, apiPost, apiPut } from "../api/client";

export interface SettingField {
  name: string;
  group: string;
  help: string;
  secret: boolean;
  editable: boolean;
  source: "environment" | "keychain" | "file" | "default";
  set: boolean;
  value: string | number | boolean | null;
  type: string;
}

interface SettingsView {
  fields: SettingField[];
  file: string | null;
  error: string;
  keychain: { available: boolean; backend: string };
  supervised: boolean;
}

interface KeychainView {
  available: boolean;
  backend: string;
  names: string[];
}

// Choices for a field whose type is a fixed set of words.
const CHOICES: Record<string, string[]> = { mode: ["local", "team"] };

/** Groups in the order the server lists them, each with its fields. */
export function grouped(fields: SettingField[]): [string, SettingField[]][] {
  const out = new Map<string, SettingField[]>();
  for (const f of fields) {
    const list = out.get(f.group) ?? [];
    list.push(f);
    out.set(f.group, list);
  }
  return [...out.entries()];
}

/** The typed value an input's text stands for, or `null` to remove the key (empty text). */
export function parseInput(field: SettingField, text: string): string | number | boolean | null {
  if (field.type === "bool") return text === "true";
  if (text.trim() === "") return null;
  if (field.type === "int") {
    const n = Number(text);
    return Number.isInteger(n) ? n : text; // the server says what is wrong with "soon"
  }
  return text;
}

const display = (v: SettingField["value"]) => (v === null ? "" : String(v));

export function Settings() {
  const qc = useQueryClient();
  const view = useQuery({ queryKey: ["platform-settings"], queryFn: () => api<SettingsView>("/platform/settings") });
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [secrets, setSecrets] = useState<Record<string, string>>({});
  const [saved, setSaved] = useState(false);

  const save = useMutation({
    mutationFn: (body: { values?: Record<string, unknown>; secrets?: Record<string, string | null> }) =>
      apiPut("/platform/settings", body),
    onSuccess: async () => {
      setDraft({});
      setSecrets({});
      setSaved(true);
      await qc.invalidateQueries({ queryKey: ["platform-settings"] });
    },
  });
  const restart = useMutation({ mutationFn: () => apiPost("/platform/restart") });

  if (view.isError) return <p className="err">{String(view.error)}</p>;
  if (!view.data) return <p className="muted">loading…</p>;
  const data = view.data;
  const byName = new Map(data.fields.map((f) => [f.name, f]));
  const changed = Object.keys(draft).filter((n) => draft[n] !== display(byName.get(n)?.value ?? null));

  return (
    <>
      <section className="card">
        <strong>Console settings</strong>
        <p className="muted">
          {data.file ? (
            <>
              Saved to <code>{data.file}</code>. A value set by the environment wins and is shown
              here read-only. Every change applies when the console starts again.
            </>
          ) : (
            <>This console has no EKOS home, so its settings come from the environment only.</>
          )}
        </p>
        {data.error && <p className="err">The saved settings do not start: {data.error}</p>}
        {saved && (
          <p className="ok-msg">
            Saved. Restart the console to apply.{" "}
            {data.supervised ? (
              <button onClick={() => restart.mutate()} disabled={restart.isPending}>
                Restart now
              </button>
            ) : (
              <span className="muted">It was not started by a launcher; restart it yourself.</span>
            )}
          </p>
        )}
        {restart.isSuccess && <p className="muted">Restarting… reload this page in a few seconds.</p>}
        {restart.isError && <p className="err">{String(restart.error)}</p>}
      </section>

      {grouped(data.fields).map(([group, fields]) => (
        <section className="card" key={group}>
          <strong>{group}</strong>
          <table className="settings">
            <tbody>
              {fields.map((f) => (
                <tr key={f.name}>
                  <td>
                    <code>{f.name}</code>
                    <div className="muted small">{f.help}</div>
                  </td>
                  <td>
                    {f.secret ? (
                      <SecretInput
                        field={f}
                        keychain={data.keychain.available}
                        value={secrets[f.name] ?? ""}
                        onChange={(v) => setSecrets({ ...secrets, [f.name]: v })}
                        onRemove={() => save.mutate({ secrets: { [f.name]: null } })}
                      />
                    ) : (
                      <ValueInput
                        field={f}
                        text={draft[f.name] ?? display(f.value)}
                        onChange={(t) => setDraft({ ...draft, [f.name]: t })}
                      />
                    )}
                  </td>
                  <td>
                    <span className={`chip ${f.source === "environment" ? "warn" : ""}`}>{f.source}</span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ))}

      <section className="card">
        <button
          disabled={save.isPending || (changed.length === 0 && Object.keys(secrets).length === 0)}
          onClick={() =>
            save.mutate({
              values: Object.fromEntries(changed.map((n) => [n, parseInput(byName.get(n)!, draft[n])])),
              secrets: Object.fromEntries(Object.entries(secrets).filter(([, v]) => v !== "")),
            })
          }
        >
          Save settings
        </button>{" "}
        {changed.length > 0 && <span className="muted">{changed.length} changed</span>}
        {save.isError && <p className="err">{String(save.error)}</p>}
      </section>

      <Keychain />
      <Uninstall />
    </>
  );
}

// RFC 0188: the uninstall page is followed only to this machine's loopback, whatever the
// server answers.
export function uninstallTarget(url: string | undefined): string | null {
  return url && /^http:\/\/127\.0\.0\.1:\d+\//.test(url) ? url : null;
}

function Uninstall() {
  const view = useQuery({
    queryKey: ["uninstall"],
    queryFn: () => api<{ available: boolean; reason: string | null }>("/platform/uninstall"),
  });
  const start = useMutation({
    mutationFn: () => apiPost<{ url: string }>("/platform/uninstall"),
    onSuccess: (r) => {
      const target = uninstallTarget(r.url);
      if (target) window.location.assign(target);
    },
  });
  if (!view.data) return null;
  return (
    <section className="card">
      <strong>Uninstall EKOS</strong>
      <p className="muted">
        Opens a page that lists what will be removed — the program, the console, its login entry, the
        PATH line and the agent entries setup added — and your projects with their size. You choose
        there whether to keep the projects, and confirm. Nothing is removed before that.
      </p>
      {view.data.available ? (
        <button onClick={() => start.mutate()} disabled={start.isPending}>
          {start.isPending ? "Opening…" : "Uninstall EKOS…"}
        </button>
      ) : (
        <p className="muted">{view.data.reason}</p>
      )}
      {start.isError && <p className="err">{String(start.error)}</p>}
      {start.isSuccess && !uninstallTarget(start.data.url) && (
        <p className="err">The uninstall page answered with an address off this machine; not opened.</p>
      )}
    </section>
  );
}

function ValueInput({
  field,
  text,
  onChange,
}: {
  field: SettingField;
  text: string;
  onChange: (t: string) => void;
}) {
  const disabled = !field.editable;
  if (field.type === "bool") {
    return (
      <input
        type="checkbox"
        aria-label={field.name}
        checked={text === "true"}
        disabled={disabled}
        onChange={(e) => onChange(String(e.target.checked))}
      />
    );
  }
  const choices = CHOICES[field.name];
  if (choices) {
    return (
      <select aria-label={field.name} value={text} disabled={disabled} onChange={(e) => onChange(e.target.value)}>
        {choices.map((c) => (
          <option key={c}>{c}</option>
        ))}
      </select>
    );
  }
  return (
    <input
      aria-label={field.name}
      type={field.type === "int" ? "number" : "text"}
      value={text}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

function SecretInput({
  field,
  keychain,
  value,
  onChange,
  onRemove,
}: {
  field: SettingField;
  keychain: boolean;
  value: string;
  onChange: (v: string) => void;
  onRemove: () => void;
}) {
  if (!field.editable) return <span className="muted">{field.set ? "set" : "not set"}</span>;
  if (!keychain)
    return <span className="muted">{field.set ? "set" : "not set"} — no keychain here; use an environment variable</span>;
  return (
    <>
      <input
        type="password"
        aria-label={field.name}
        placeholder={field.set ? "set — type to replace" : "not set"}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
      {field.source === "keychain" && (
        <button className="linkish" onClick={onRemove}>
          remove
        </button>
      )}
    </>
  );
}

function Keychain() {
  const qc = useQueryClient();
  const view = useQuery({ queryKey: ["keychain"], queryFn: () => api<KeychainView>("/platform/keychain") });
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const done = () => qc.invalidateQueries({ queryKey: ["keychain"] });
  const put = useMutation({
    mutationFn: () => apiPut(`/platform/keychain/${encodeURIComponent(name.trim())}`, { value }),
    onSuccess: async () => {
      setName("");
      setValue("");
      await done();
    },
  });
  const del = useMutation({
    mutationFn: (n: string) => apiDelete(`/platform/keychain/${encodeURIComponent(n)}`),
    onSuccess: done,
  });
  if (!view.data) return null;

  return (
    <section className="card">
      <strong>Model keys in the keychain</strong>
      <p className="muted">
        A project names the environment variable holding its key (<code>api-key-env</code> in its{" "}
        <code>ekos.toml</code>). A value stored here under that name is given to the commands of a
        tenant granted the name in Admin, while its metered calls are on. Values are never shown
        again. Keychain: {view.data.backend}.
      </p>
      {!view.data.available ? (
        <p className="muted">No keychain is usable here; set the variables in the console's environment instead.</p>
      ) : (
        <>
          <ul>
            {view.data.names.map((n) => (
              <li key={n}>
                <code>{n}</code>{" "}
                <button className="linkish" onClick={() => del.mutate(n)}>
                  remove
                </button>
              </li>
            ))}
            {view.data.names.length === 0 && <li className="muted">nothing stored</li>}
          </ul>
          <form
            className="token-row"
            onSubmit={(e) => {
              e.preventDefault();
              put.mutate();
            }}
          >
            <input aria-label="variable name" placeholder="ANTHROPIC_API_KEY" value={name} onChange={(e) => setName(e.target.value)} />
            <input type="password" aria-label="value" placeholder="value" value={value} onChange={(e) => setValue(e.target.value)} />
            <button type="submit" disabled={!name.trim() || !value || put.isPending}>
              Store
            </button>
          </form>
          {put.isError && <p className="err">{String(put.error)}</p>}
        </>
      )}
    </section>
  );
}
