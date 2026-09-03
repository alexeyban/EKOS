import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useNavigate, useOutletContext } from "react-router-dom";
import { api, apiDelete, apiPost, apiPut } from "../api/client";

interface Me {
  role: "read" | "write";
}
interface Workspace {
  id: string;
  name: string;
}
interface CommandDef {
  name: string;
  summary: string;
  is_write: boolean;
  params: Record<string, { kind: "bool" | "string"; required: boolean; help: string }>;
}
interface Schedule {
  id: string;
  workspace_id: string;
  command: string;
  trigger_kind: "cron" | "interval";
  trigger_expr: string;
  notify_url: string;
  enabled: boolean;
  last_run_at: string | null;
  last_run_id: string | null;
  last_status: string | null;
}

const chip = (s: string | null) =>
  s === "succeeded" ? "ok" : s && ["failed", "timed_out", "interrupted"].includes(s) ? "bad" : "";

export function Schedules() {
  const me = useOutletContext<Me>();
  const qc = useQueryClient();
  const rows = useQuery({
    queryKey: ["schedules"],
    queryFn: () => api<Schedule[]>("/schedules"),
    refetchInterval: 5000,
  });
  const workspaces = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => api<Workspace[]>("/workspaces"),
  });

  const toggle = useMutation({
    mutationFn: (s: Schedule) => apiPut(`/schedules/${s.id}`, { enabled: !s.enabled }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["schedules"] }),
  });
  const del = useMutation({
    mutationFn: (id: string) => apiDelete(`/schedules/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["schedules"] }),
  });

  if (me.role !== "write")
    return <p className="muted">Schedules run write commands — sign in with the write token.</p>;

  return (
    <>
      <p className="crumbs">
        <Link to="/" className="linkish">
          ← workspaces
        </Link>
      </p>

      <section className="card">
        <strong>Schedules</strong>
        {rows.data?.length === 0 && <p className="muted">none yet</p>}
        <ul className="ws-list">
          {rows.data?.map((s) => (
            <li key={s.id}>
              <code>{s.command}</code>
              <span className="muted">{s.workspace_id}</span>
              <span className="muted">
                {s.trigger_kind === "interval" ? `every ${s.trigger_expr}s` : `cron ${s.trigger_expr}`}
              </span>
              <span className="path muted">
                {s.last_status ? (
                  <>
                    <span className={`chip ${chip(s.last_status)}`}>{s.last_status}</span>{" "}
                    {s.last_run_id && (
                      <Link to={`/runs/${s.last_run_id}`} className="linkish">
                        log
                      </Link>
                    )}
                  </>
                ) : (
                  "not run yet"
                )}
              </span>
              <button className="linkish" onClick={() => toggle.mutate(s)}>
                {s.enabled ? "disable" : "enable"}
              </button>
              <RunNow id={s.id} />
              <button className="linkish danger" onClick={() => del.mutate(s.id)}>
                delete
              </button>
            </li>
          ))}
        </ul>
      </section>

      <CreateForm workspaces={workspaces.data ?? []} />
    </>
  );
}

function RunNow({ id }: { id: string }) {
  const nav = useNavigate();
  const m = useMutation({
    mutationFn: () => apiPost<{ run_id: string }>(`/schedules/${id}/run-now`),
    onSuccess: (r) => nav(`/runs/${r.run_id}`),
  });
  return (
    <button className="linkish" onClick={() => m.mutate()} disabled={m.isPending}>
      run now
    </button>
  );
}

function CreateForm({ workspaces }: { workspaces: Workspace[] }) {
  const qc = useQueryClient();
  const commands = useQuery({
    queryKey: ["commands"],
    queryFn: () => api<CommandDef[]>("/commands"),
  });
  const [ws, setWs] = useState("");
  const [cmd, setCmd] = useState("");
  const [kind, setKind] = useState<"interval" | "cron">("interval");
  const [expr, setExpr] = useState("3600");
  const [url, setUrl] = useState("");
  const [params, setParams] = useState<Record<string, unknown>>({});

  const create = useMutation({
    mutationFn: () =>
      apiPost("/schedules", {
        workspace_id: ws,
        command: cmd,
        params,
        trigger_kind: kind,
        trigger_expr: expr,
        notify_url: url,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["schedules"] });
      setCmd("");
      setUrl("");
      setParams({});
    },
  });

  const cmdDef = commands.data?.find((c) => c.name === cmd);

  return (
    <section className="card">
      <strong>New schedule</strong>
      <form
        className="reg-form"
        style={{ flexDirection: "column", alignItems: "stretch" }}
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate();
        }}
      >
        <select value={ws} onChange={(e) => setWs(e.target.value)} required>
          <option value="">workspace…</option>
          {workspaces.map((w) => (
            <option key={w.id} value={w.id}>
              {w.name}
            </option>
          ))}
        </select>
        <select value={cmd} onChange={(e) => setCmd(e.target.value)} required>
          <option value="">command…</option>
          {commands.data?.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name} — {c.summary}
            </option>
          ))}
        </select>
        {cmdDef &&
          Object.entries(cmdDef.params).map(([name, spec]) => (
            <input
              key={name}
              placeholder={`${name}${spec.required ? " (required)" : ""}`}
              onChange={(e) => setParams((p) => ({ ...p, [name]: e.target.value }))}
            />
          ))}
        <div className="reg-form">
          <select value={kind} onChange={(e) => setKind(e.target.value as "interval" | "cron")}>
            <option value="interval">interval (seconds)</option>
            <option value="cron">cron (UTC)</option>
          </select>
          <input
            value={expr}
            onChange={(e) => setExpr(e.target.value)}
            placeholder={kind === "cron" ? "0 3 * * *" : "3600"}
            required
          />
        </div>
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="notify_url (https://… — POSTed on a failed run)"
          required
        />
        <button type="submit" disabled={create.isPending}>
          Create
        </button>
      </form>
      {create.isError && <p className="err">{String(create.error)}</p>}
    </section>
  );
}
