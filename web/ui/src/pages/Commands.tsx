// RFC 0186: every `ekos` command, from the binary's own catalogue, with a form built from its
// arguments. The server turns the form into an argv and checks every value; this page only
// shows what will run and who may run it.
import { useMutation, useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";

export interface ArgSpec {
  id: string;
  long: string | null;
  index: number | null;
  kind: "flag" | "value" | "path";
  multiple: boolean;
  required: boolean;
  possible: string[];
  default: string[];
  value_name: string | null;
  help: string;
}

export interface CommandEntry {
  path: string[];
  about: string;
  class: "read" | "write" | "review" | "admin" | "external";
  metered: boolean;
  allowed: boolean;
  confirm: boolean;
  args: ArgSpec[];
}

const CLASS_HELP: Record<CommandEntry["class"], string> = {
  read: "reads the project",
  write: "changes the project (operator)",
  review: "a human decision (reviewer; recorded under your name)",
  admin: "destructive (admin; type the command to confirm)",
  external: "reaches a live system outside EKOS (admin; type the command to confirm)",
};

/** Commands grouped by their first word, in catalogue order. */
export function groupCommands(commands: CommandEntry[], filter: string): [string, CommandEntry[]][] {
  const q = filter.trim().toLowerCase();
  const out = new Map<string, CommandEntry[]>();
  for (const c of commands) {
    const name = c.path.join(" ");
    if (q && !name.includes(q) && !c.about.toLowerCase().includes(q)) continue;
    const g = c.path.length > 1 ? c.path[0] : "project";
    out.set(g, [...(out.get(g) ?? []), c]);
  }
  return [...out.entries()];
}

type Values = Record<string, string | boolean>;

/** The arguments a person fills in. For a review the reviewer is the signed-in person, set by
 * the server, so `by` is not a field. */
export function visibleArgs(cmd: CommandEntry): ArgSpec[] {
  return cmd.args.filter((a) => !(cmd.class === "review" && a.id === "by"));
}

/** The request body's `args` for the form's values: flags as booleans, a multiple argument as
 * one value per line, empty fields left out. */
export function formArgs(cmd: CommandEntry, values: Values): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const a of visibleArgs(cmd)) {
    const v = values[a.id];
    if (a.kind === "flag") {
      if (v === true) out[a.id] = true;
      continue;
    }
    if (typeof v !== "string" || v.trim() === "") continue;
    out[a.id] = a.multiple
      ? v
          .split("\n")
          .map((s) => s.trim())
          .filter(Boolean)
      : v.trim();
  }
  return out;
}

/** What will run, for showing before it does (the server builds the real argv). */
export function preview(cmd: CommandEntry, values: Values): string {
  const parts = ["ekos", ...cmd.path];
  const args = formArgs(cmd, values);
  const positional: string[] = [];
  for (const a of cmd.args) {
    const v = args[a.id];
    if (v === undefined) continue;
    const items = Array.isArray(v) ? v : [v];
    if (a.long) {
      for (const i of items) parts.push(i === true ? `--${a.long}` : `--${a.long}=${String(i)}`);
    } else positional.push(...items.map(String));
  }
  return [...parts, ...positional].join(" ");
}

export function Commands() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const list = useQuery({
    queryKey: ["cli", id],
    queryFn: () => api<{ commands: CommandEntry[] }>(`/workspaces/${id}/cli`),
    enabled: id !== "",
  });
  const [filter, setFilter] = useState("");
  const [picked, setPicked] = useState<string>("");
  const [values, setValues] = useState<Values>({});
  const [confirm, setConfirm] = useState("");
  const cmd = useMemo(
    () => list.data?.commands.find((c) => c.path.join(" ") === picked),
    [list.data, picked],
  );
  const run = useMutation({
    mutationFn: () =>
      apiPost<{ run_id: string }>(`/workspaces/${id}/cli`, {
        command: cmd!.path,
        args: formArgs(cmd!, values),
        confirm: confirm || null,
      }),
    onSuccess: (r) => navigate(`/runs/${r.run_id}`),
  });

  if (list.isError) return <p className="err">{String(list.error)}</p>;
  if (!list.data) return <p className="muted">loading…</p>;

  return (
    <div className="commands-page">
      <section className="card commands-list">
        <input
          aria-label="filter commands"
          placeholder="filter: query, semantics, ledger…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        {groupCommands(list.data.commands, filter).map(([group, cmds]) => (
          <div key={group}>
            <p className="muted small">{group}</p>
            <ul>
              {cmds.map((c) => {
                const name = c.path.join(" ");
                return (
                  <li key={name}>
                    <button
                      className={`linkish ${picked === name ? "on" : ""}`}
                      onClick={() => {
                        setPicked(name);
                        setValues({});
                        setConfirm("");
                        run.reset();
                      }}
                    >
                      {name}
                    </button>
                    {!c.allowed && <span className="muted small"> · not for you</span>}
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </section>

      <section className="card commands-form">
        {!cmd ? (
          <p className="muted">Choose a command. Each one runs on the server as the project's job, like the pipeline, and its output appears in the run's log.</p>
        ) : (
          <>
            <strong>ekos {cmd.path.join(" ")}</strong>
            <p>{cmd.about}</p>
            <p>
              <span className={`chip ${cmd.class === "read" ? "ok" : cmd.class === "write" ? "" : "warn"}`}>{cmd.class}</span>{" "}
              <span className="muted">{CLASS_HELP[cmd.class]}</span>
            </p>
            {cmd.metered && (
              <p className="warn-line">
                May call the model configured in this project's <code>ekos.toml</code>. A hosted model is paid per call; a
                local one (Ollama) is not.
              </p>
            )}
            {visibleArgs(cmd).map((a) => (
              <div key={a.id} className="arg">
                <label htmlFor={`arg-${a.id}`}>
                  {a.long ? `--${a.long}` : (a.value_name ?? a.id).toLowerCase()}
                  {a.required && " *"}
                  {a.kind === "path" && <span className="muted small"> (a path in the project or its sources)</span>}
                </label>
                {a.kind === "flag" ? (
                  <input
                    id={`arg-${a.id}`}
                    type="checkbox"
                    checked={values[a.id] === true}
                    onChange={(e) => setValues({ ...values, [a.id]: e.target.checked })}
                  />
                ) : a.possible.length > 0 ? (
                  <select
                    id={`arg-${a.id}`}
                    value={String(values[a.id] ?? "")}
                    onChange={(e) => setValues({ ...values, [a.id]: e.target.value })}
                  >
                    <option value="">{a.default.length ? `(default: ${a.default.join(", ")})` : "—"}</option>
                    {a.possible.map((p) => (
                      <option key={p}>{p}</option>
                    ))}
                  </select>
                ) : a.multiple ? (
                  <textarea
                    id={`arg-${a.id}`}
                    placeholder="one per line"
                    value={String(values[a.id] ?? "")}
                    onChange={(e) => setValues({ ...values, [a.id]: e.target.value })}
                  />
                ) : (
                  <input
                    id={`arg-${a.id}`}
                    placeholder={a.default.length ? `default: ${a.default.join(", ")}` : ""}
                    value={String(values[a.id] ?? "")}
                    onChange={(e) => setValues({ ...values, [a.id]: e.target.value })}
                  />
                )}
                {a.help && <div className="muted small">{a.help}</div>}
              </div>
            ))}
            <pre className="argv">{preview(cmd, values)}</pre>
            {cmd.confirm && (
              <div className="arg">
                <label htmlFor="confirm">
                  Type <code>{cmd.path.join(" ")}</code> to confirm
                </label>
                <input id="confirm" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
              </div>
            )}
            <button
              disabled={!cmd.allowed || run.isPending || (cmd.confirm && confirm !== cmd.path.join(" "))}
              onClick={() => run.mutate()}
            >
              Run
            </button>
            {!cmd.allowed && <span className="muted"> Your role here does not allow this command.</span>}
            {run.isError && <p className="err">{String(run.error)}</p>}
          </>
        )}
      </section>
    </div>
  );
}
