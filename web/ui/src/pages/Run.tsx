import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useNavigate, useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import { Icon } from "../Icon";
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";

interface Me {
  role: "read" | "write";
  canOperate?: boolean;
}
interface CommandDef {
  name: string;
  summary: string;
  is_write: boolean;
  stages: string[];
  params: Record<string, { kind: "bool" | "string"; required: boolean; help: string }>;
}

export function Run() {
  const { id = "" } = useParams();
  const me = useOutletContext<Me>();
  const commands = useQuery({
    queryKey: ["commands"],
    queryFn: () => api<CommandDef[]>("/commands"),
  });

  const role = (me.canOperate ?? me.role === "write") ? "write" : "read";

  return (
    <>
      <PageHead
        title="Run"
        sub="Start a compiler stage or a pipeline on this project. Each run opens its own page with stages and a live log."
        actions={
          <Link className="button" to={`/w/${id}/runs`}>
            <Icon name="history" /> History
          </Link>
        }
      />
      <QueryView
        q={commands}
        what="commands"
        tiles={4}
        isEmpty={(c) => c.length === 0}
        empty={<Empty title="No commands offered">The console lists no runnable commands for this project.</Empty>}
      >
        {(list) => (
          <div className="x-run-grid">
            {list.map((c) => (
              <CommandCard key={c.name} workspace={id} command={c} role={role} />
            ))}
          </div>
        )}
      </QueryView>
    </>
  );
}

function CommandCard({
  workspace,
  command,
  role,
}: {
  workspace: string;
  command: CommandDef;
  role: "read" | "write";
}) {
  const nav = useNavigate();
  const [params, setParams] = useState<Record<string, unknown>>({});
  const blocked = command.is_write && role !== "write";

  const run = useMutation({
    mutationFn: () =>
      apiPost<{ run_id: string }>(`/workspaces/${workspace}/commands/${command.name}`, params),
    onSuccess: (r) => nav(`/runs/${r.run_id}`),
  });

  return (
    <section className="card x-command" aria-label={command.name}>
      <div className="x-command-head">
        <h2 className="card-title">{command.name}</h2>
        {command.is_write && <span className="chip warn">write</span>}
      </div>
      <p className="muted">{command.summary}</p>
      {command.stages.length > 0 && (
        <ol className="x-stage-trail" aria-label="stages">
          {command.stages.map((s) => (
            <li key={s}>
              <code>{s}</code>
            </li>
          ))}
        </ol>
      )}
      {Object.keys(command.params).length > 0 && (
        <div className="x-fields">
          {Object.entries(command.params).map(([name, spec]) => (
            <ParamField
              key={name}
              id={`${command.name}-${name}`}
              name={name}
              spec={spec}
              onChange={(v) => setParams((p) => ({ ...p, [name]: v }))}
            />
          ))}
        </div>
      )}
      <div className="x-form-foot">
        <button className="primary" onClick={() => run.mutate()} disabled={blocked || run.isPending}>
          <Icon name="run" /> Run
        </button>
        {blocked && <span className="muted">needs the write role</span>}
      </div>
      {run.isError && <ErrorNote error={run.error} what={`run ${command.name}`} />}
    </section>
  );
}

function ParamField({
  id,
  name,
  spec,
  onChange,
}: {
  id: string;
  name: string;
  spec: CommandDef["params"][string];
  onChange: (v: unknown) => void;
}) {
  if (spec.kind === "bool") {
    return (
      <div className="x-field x-field-check">
        <label htmlFor={id}>
          <input id={id} type="checkbox" onChange={(e) => onChange(e.target.checked)} /> <code>--{name}</code>
        </label>
        {spec.help && <span className="x-hint">{spec.help}</span>}
      </div>
    );
  }
  return (
    <div className="x-field">
      <label htmlFor={id}>
        <code>--{name}</code>
        {spec.required && <span className="x-req"> required</span>}
      </label>
      <input
        id={id}
        placeholder={`${name}${spec.required ? " (required)" : ""}`}
        onChange={(e) => onChange(e.target.value)}
      />
      {spec.help && <span className="x-hint">{spec.help}</span>}
    </div>
  );
}
