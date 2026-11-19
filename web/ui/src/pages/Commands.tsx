// RFC 0186: every `ekos` command, from the binary's own catalogue, with a form built from its
// arguments. The server turns the form into an argv and checks every value; this page only
// shows what will run and who may run it.
import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import { Icon } from "../Icon";
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";

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
    // An answer without the list reads as no commands offered, not a crash.
    select: (d) => ({ commands: d?.commands ?? [] }),
    enabled: id !== "",
  });
  const [filter, setFilter] = useState("");
  // RFC 0261: the command palette opens a command's form with `?cmd=<path>`.
  const [searchParams] = useSearchParams();
  const [picked, setPicked] = useState<string>(() => searchParams.get("cmd") ?? "");
  const [values, setValues] = useState<Values>({});
  const [confirm, setConfirm] = useState("");
  const asked = searchParams.get("cmd");
  useEffect(() => {
    if (!asked) return;
    setPicked(asked);
    setValues({});
    setConfirm("");
  }, [asked]);
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

  const choose = (name: string) => {
    setPicked(name);
    setValues({});
    setConfirm("");
    run.reset();
  };

  return (
    <>
      <PageHead
        title="Commands"
        sub="Every ekos command your role may run here, with a form built from its arguments. Nothing runs until you press Run."
      />
      <QueryView q={list} what="the command catalogue" tiles={2}>
        {(data) => (
          <div className="commands-page">
            <CommandList commands={data.commands} filter={filter} setFilter={setFilter} picked={picked} choose={choose} />
            <section className="card commands-form" aria-label="command form">
              {cmd ? (
                <CommandForm
                  cmd={cmd}
                  values={values}
                  setValues={setValues}
                  confirm={confirm}
                  setConfirm={setConfirm}
                  run={run}
                />
              ) : (
                <Empty title="Choose a command" icon="commands">
                  Each one runs on the server as the project's job, like the pipeline, and its output appears in the
                  run's log.
                </Empty>
              )}
            </section>
          </div>
        )}
      </QueryView>
    </>
  );
}

function CommandList({
  commands,
  filter,
  setFilter,
  picked,
  choose,
}: {
  commands: CommandEntry[];
  filter: string;
  setFilter: (v: string) => void;
  picked: string;
  choose: (name: string) => void;
}) {
  const groups = groupCommands(commands, filter);
  return (
    <section className="card commands-list" aria-label="commands">
      <input
        type="search"
        aria-label="filter commands"
        placeholder="filter: query, semantics, ledger…"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      {groups.length === 0 && <p className="muted small">No command matches that filter.</p>}
      {groups.map(([group, cmds]) => (
        <div key={group} className="x-cmd-group">
          <p className="card-label">{group}</p>
          <ul>
            {cmds.map((c) => {
              const name = c.path.join(" ");
              return (
                <li key={name}>
                  <button
                    type="button"
                    className={`linkish x-cmd ${picked === name ? "on" : ""}`}
                    aria-current={picked === name ? "true" : undefined}
                    onClick={() => choose(name)}
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
  );
}

function classChip(c: CommandEntry["class"]): string {
  if (c === "read") return "ok";
  if (c === "write") return "";
  return "warn";
}

function CommandForm({
  cmd,
  values,
  setValues,
  confirm,
  setConfirm,
  run,
}: {
  cmd: CommandEntry;
  values: Values;
  setValues: (v: Values) => void;
  confirm: string;
  setConfirm: (v: string) => void;
  run: { mutate: () => void; isPending: boolean; isError: boolean; error: unknown };
}) {
  const name = cmd.path.join(" ");
  return (
    <>
      <h2 className="card-title">
        <code>ekos {name}</code>
      </h2>
      <p>{cmd.about}</p>
      <p className="x-class-line">
        <span className={`chip ${classChip(cmd.class)}`}>{cmd.class}</span>{" "}
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
          <ArgInput a={a} value={values[a.id]} set={(v) => setValues({ ...values, [a.id]: v })} />
          {a.help && <div className="muted small">{a.help}</div>}
        </div>
      ))}
      <div className="card-label">Will run</div>
      <pre className="argv">{preview(cmd, values)}</pre>
      {cmd.confirm && (
        <div className="arg">
          <label htmlFor="confirm">
            Type <code>{name}</code> to confirm
          </label>
          <input id="confirm" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        </div>
      )}
      <div className="x-form-foot">
        <button
          className="primary"
          disabled={!cmd.allowed || run.isPending || (cmd.confirm && confirm !== name)}
          onClick={() => run.mutate()}
        >
          <Icon name="run" /> Run
        </button>
        {!cmd.allowed && <span className="muted"> Your role here does not allow this command.</span>}
      </div>
      {run.isError && <ErrorNote error={run.error} what={`run ekos ${name}`} />}
    </>
  );
}

function ArgInput({ a, value, set }: { a: ArgSpec; value: string | boolean | undefined; set: (v: string | boolean) => void }) {
  const id = `arg-${a.id}`;
  if (a.kind === "flag") {
    return <input id={id} type="checkbox" checked={value === true} onChange={(e) => set(e.target.checked)} />;
  }
  const text = String(value ?? "");
  if (a.possible.length > 0) {
    return (
      <select id={id} value={text} onChange={(e) => set(e.target.value)}>
        <option value="">{a.default.length ? `(default: ${a.default.join(", ")})` : "—"}</option>
        {a.possible.map((p) => (
          <option key={p}>{p}</option>
        ))}
      </select>
    );
  }
  if (a.multiple) {
    return <textarea id={id} placeholder="one per line" value={text} onChange={(e) => set(e.target.value)} />;
  }
  return (
    <input
      id={id}
      placeholder={a.default.length ? `default: ${a.default.join(", ")}` : ""}
      value={text}
      onChange={(e) => set(e.target.value)}
    />
  );
}
