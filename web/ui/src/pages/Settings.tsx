// RFC 0184: the console's own settings (console.toml) and the keychain, for platform owners.
// A value set by the environment is shown and cannot be changed here; a secret is never shown,
// only whether it is set and where. Every change applies at the next start.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, apiDelete, apiPost, apiPut } from "../api/client";
import { ErrorNote, PageHead, Skeleton } from "../ui";

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
  const view = useQuery({
    queryKey: ["platform-settings"],
    queryFn: () => api<SettingsView>("/platform/settings"),
    // An answer without fields reads as no settings, not a crash.
    select: (d) => ({ ...d, fields: d?.fields ?? [] }),
  });
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

  const head = <PageHead title="Settings" sub="This console's own settings, and the model keys it keeps in the keychain." />;
  if (view.isError)
    return (
      <div className="fm-page">
        {head}
        <ErrorNote error={view.error} what="Reading the settings" />
      </div>
    );
  if (!view.data)
    return (
      <div className="fm-page">
        {head}
        <Skeleton rows={6} label="loading the settings" />
      </div>
    );
  const data = view.data;
  const byName = new Map(data.fields.map((f) => [f.name, f]));
  const changed = Object.keys(draft).filter((n) => draft[n] !== display(byName.get(n)?.value ?? null));
  const nothingToSave = changed.length === 0 && Object.keys(secrets).length === 0;
  const saveAll = () =>
    save.mutate({
      values: Object.fromEntries(changed.map((n) => [n, parseInput(byName.get(n)!, draft[n])])),
      secrets: Object.fromEntries(Object.entries(secrets).filter(([, v]) => v !== "")),
    });

  return (
    <div className="fm-page">
      {head}
      <section className="card" aria-labelledby="settings-console">
        <h2 className="card-label" id="settings-console">
          Console settings
        </h2>
        <p className="fm-note">
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
          <SavedNote supervised={data.supervised} onRestart={() => restart.mutate()} restarting={restart.isPending} />
        )}
        {restart.isSuccess && <p className="fm-hint">Restarting… reload this page in a few seconds.</p>}
        {restart.isError && <ErrorNote error={restart.error} what="Restarting" />}
      </section>

      {grouped(data.fields).map(([group, fields]) => (
        <section className="card" key={group} aria-label={group}>
          <h2 className="card-label">{group}</h2>
          <div className="fm-grid">
            {fields.map((f) => (
              <SettingRow
                key={f.name}
                field={f}
                keychain={data.keychain.available}
                draft={draft[f.name]}
                secret={secrets[f.name] ?? ""}
                onDraft={(t) => setDraft({ ...draft, [f.name]: t })}
                onSecret={(v) => setSecrets({ ...secrets, [f.name]: v })}
                onRemoveSecret={() => save.mutate({ secrets: { [f.name]: null } })}
              />
            ))}
          </div>
        </section>
      ))}

      <div className="fm-actions fm-footer">
        <button className="primary" disabled={save.isPending || nothingToSave} onClick={saveAll}>
          Save settings
        </button>
        {changed.length > 0 && <span className="fm-status">{changed.length} changed</span>}
      </div>
      {save.isError && <ErrorNote error={save.error} what="Saving the settings" />}

      <Keychain />
      <Uninstall />
    </div>
  );
}

function SavedNote({
  supervised,
  restarting,
  onRestart,
}: {
  supervised: boolean;
  restarting: boolean;
  onRestart: () => void;
}) {
  return (
    <div className="fm-actions">
      <span className="fm-ok">Saved. Restart the console to apply.</span>
      {supervised ? (
        <button className="primary" onClick={onRestart} disabled={restarting}>
          Restart now
        </button>
      ) : (
        <span className="fm-status">It was not started by a launcher; restart it yourself.</span>
      )}
    </div>
  );
}

/** One setting: its name and where its value comes from above the control, its help under it. */
function SettingRow({
  field,
  keychain,
  draft,
  secret,
  onDraft,
  onSecret,
  onRemoveSecret,
}: {
  field: SettingField;
  keychain: boolean;
  draft: string | undefined;
  secret: string;
  onDraft: (t: string) => void;
  onSecret: (v: string) => void;
  onRemoveSecret: () => void;
}) {
  return (
    <div className="fm-field">
      <div className="fm-label">
        <code>{field.name}</code>
        <span className={`chip fm-chip ${field.source === "environment" ? "warn" : ""}`}>{field.source}</span>
      </div>
      {field.secret ? (
        <SecretInput field={field} keychain={keychain} value={secret} onChange={onSecret} onRemove={onRemoveSecret} />
      ) : (
        <ValueInput field={field} text={draft ?? display(field.value)} onChange={onDraft} />
      )}
      {field.help && <div className="fm-hint">{field.help}</div>}
    </div>
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
    <section className="card" aria-labelledby="settings-uninstall">
      <h2 className="card-label" id="settings-uninstall">
        Uninstall EKOS
      </h2>
      <p className="fm-note">
        Opens a page that lists what will be removed — the program, the console, its login entry, the
        PATH line and the agent entries setup added — and your projects with their size. You choose
        there whether to keep the projects, and confirm. Nothing is removed before that.
      </p>
      {view.data.available ? (
        <div className="fm-actions">
          <button className="danger-outline" onClick={() => start.mutate()} disabled={start.isPending}>
            {start.isPending ? "Opening…" : "Uninstall EKOS…"}
          </button>
        </div>
      ) : (
        <p className="fm-hint">{view.data.reason}</p>
      )}
      {start.isError && <ErrorNote error={start.error} what="Opening the uninstall page" />}
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
      <span className="fm-check">
        <input
          type="checkbox"
          aria-label={field.name}
          checked={text === "true"}
          disabled={disabled}
          onChange={(e) => onChange(String(e.target.checked))}
        />
      </span>
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
  if (!field.editable) return <span className="fm-hint">{field.set ? "set" : "not set"}</span>;
  if (!keychain)
    return <span className="fm-hint">{field.set ? "set" : "not set"} — no keychain here; use an environment variable</span>;
  return (
    <span className="fm-inline">
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
    </span>
  );
}

function Keychain() {
  const qc = useQueryClient();
  const view = useQuery({
    queryKey: ["keychain"],
    queryFn: () => api<KeychainView>("/platform/keychain"),
    select: (d) => ({ ...d, names: d?.names ?? [] }),
  });
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
    <section className="card" aria-labelledby="settings-keychain">
      <h2 className="card-label" id="settings-keychain">
        Model keys in the keychain
      </h2>
      <p className="fm-note">
        A project names the environment variable holding its key (<code>api-key-env</code> in its{" "}
        <code>ekos.toml</code>). A value stored here under that name is given to the commands of a
        tenant granted the name in Admin, while its metered calls are on. Values are never shown
        again. Keychain: {view.data.backend}.
      </p>
      {!view.data.available ? (
        <p className="fm-hint">No keychain is usable here; set the variables in the console's environment instead.</p>
      ) : (
        <>
          <ul className="fm-list">
            {view.data.names.map((n) => (
              <li key={n}>
                <code className="fm-item">{n}</code>
                <button className="linkish danger" onClick={() => del.mutate(n)}>
                  remove
                </button>
              </li>
            ))}
            {view.data.names.length === 0 && <li className="fm-hint">Nothing stored yet. Store a key below.</li>}
          </ul>
          <form
            className="fm-inline"
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
          {put.isError && <ErrorNote error={put.error} what="Storing the key" />}
        </>
      )}
    </section>
  );
}
