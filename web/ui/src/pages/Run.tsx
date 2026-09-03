import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useNavigate, useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";

interface Me {
  role: "read" | "write";
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

  return (
    <>
      <p className="crumbs">
        <Link to={`/w/${id}`} className="linkish">
          ← dashboard
        </Link>
        <Link to={`/w/${id}/runs`} className="linkish">
          run history →
        </Link>
      </p>
      <div className="grid2">
        {commands.data?.map((c) => (
          <CommandCard key={c.name} workspace={id} command={c} role={me.role} />
        ))}
      </div>
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
    <section className="card">
      <strong>
        {command.name}
        {command.is_write && <span className="chip warn"> write</span>}
      </strong>
      <p className="muted">{command.summary}</p>
      {command.stages.length > 0 && (
        <p className="muted">stages: {command.stages.join(" → ")}</p>
      )}
      {Object.entries(command.params).map(([name, spec]) => (
        <div key={name} className="param">
          {spec.kind === "bool" ? (
            <label>
              <input
                type="checkbox"
                onChange={(e) => setParams((p) => ({ ...p, [name]: e.target.checked }))}
              />{" "}
              --{name} <span className="muted">{spec.help}</span>
            </label>
          ) : (
            <input
              placeholder={`${name}${spec.required ? " (required)" : ""}`}
              onChange={(e) => setParams((p) => ({ ...p, [name]: e.target.value }))}
            />
          )}
        </div>
      ))}
      <div className="btnrow">
        <button onClick={() => run.mutate()} disabled={blocked || run.isPending}>
          Run
        </button>
        {blocked && <span className="muted">needs the write role</span>}
      </div>
      {run.isError && <p className="err">{String(run.error)}</p>}
    </section>
  );
}
