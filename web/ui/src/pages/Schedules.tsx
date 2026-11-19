import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useNavigate, useOutletContext } from "react-router-dom";
import { api, apiDelete, apiPost, apiPut } from "../api/client";
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";

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

/** The chip colour of a run's last status. */
function chipOf(s: string | null): string {
  if (s === "succeeded") return "ok";
  if (s && ["failed", "timed_out", "interrupted"].includes(s)) return "bad";
  return "";
}

const triggerText = (s: Schedule) =>
  s.trigger_kind === "interval" ? `every ${s.trigger_expr}s` : `cron ${s.trigger_expr}`;

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
    return (
      <div className="fm-page">
        <PageHead title="Schedules" sub="Pipeline runs that start on a timer." />
        <Empty title="Schedules need the write role" icon="schedules">
          Schedules run write commands — sign in with the write token.
        </Empty>
      </div>
    );

  return (
    <div className="fm-page">
      <PageHead
        title="Schedules"
        sub="Commands that run on a timer, on any project of this console. A failed run is POSTed to the notify URL."
      />

      <section className="card" aria-labelledby="schedules-list">
        <h2 className="card-label" id="schedules-list">
          Schedules
        </h2>
        <QueryView
          q={rows}
          what="schedules"
          isEmpty={(d) => d.length === 0}
          empty={
            <Empty title="No schedules yet" icon="schedules">
              Schedule a pipeline run below: pick a project and a command, then how often it runs.
            </Empty>
          }
        >
          {(list) => (
            <div className="table-scroll">
              <table className="fm-table">
                <thead>
                  <tr>
                    <th>Command</th>
                    <th>Project</th>
                    <th>When</th>
                    <th>Last run</th>
                    <th>
                      <span className="fm-sr">Actions</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {list.map((s) => (
                    <tr key={s.id}>
                      <td>
                        <code>{s.command}</code>
                      </td>
                      <td>{s.workspace_id}</td>
                      <td>{triggerText(s)}</td>
                      <td>
                        <LastRun s={s} />
                      </td>
                      <td>
                        <span className="fm-item-actions">
                          <button className="linkish" onClick={() => toggle.mutate(s)}>
                            {s.enabled ? "disable" : "enable"}
                          </button>
                          <RunNow id={s.id} />
                          <button className="linkish danger" onClick={() => del.mutate(s.id)}>
                            delete
                          </button>
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </QueryView>
      </section>

      <CreateForm workspaces={workspaces.data ?? []} />
    </div>
  );
}

function LastRun({ s }: { s: Schedule }) {
  if (!s.last_status) return <span className="muted">not run yet</span>;
  return (
    <>
      <span className={`chip ${chipOf(s.last_status)}`}>{s.last_status}</span>{" "}
      {s.last_run_id && (
        <Link to={`/runs/${s.last_run_id}`} className="linkish">
          log
        </Link>
      )}
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
      void qc.invalidateQueries({ queryKey: ["schedules"] });
      setCmd("");
      setUrl("");
      setParams({});
    },
  });

  const cmdDef = commands.data?.find((c) => c.name === cmd);

  return (
    <section className="card" aria-labelledby="schedules-new">
      <h2 className="card-label" id="schedules-new">
        New schedule
      </h2>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate();
        }}
      >
        <div className="fm-grid">
          <label className="fm-field">
            <span className="fm-label">Project</span>
            <select value={ws} onChange={(e) => setWs(e.target.value)} required>
              <option value="">workspace…</option>
              {workspaces.map((w) => (
                <option key={w.id} value={w.id}>
                  {w.name}
                </option>
              ))}
            </select>
          </label>
          <label className="fm-field">
            <span className="fm-label">Command</span>
            <select value={cmd} onChange={(e) => setCmd(e.target.value)} required>
              <option value="">command…</option>
              {commands.data?.map((c) => (
                <option key={c.name} value={c.name}>
                  {c.name} — {c.summary}
                </option>
              ))}
            </select>
          </label>
          {cmdDef &&
            Object.entries(cmdDef.params).map(([name, spec]) => (
              <label key={name} className="fm-field">
                <span className="fm-label">
                  <code>{name}</code>
                  {spec.required && <span className="muted">required</span>}
                </span>
                <input
                  placeholder={`${name}${spec.required ? " (required)" : ""}`}
                  onChange={(e) => setParams((p) => ({ ...p, [name]: e.target.value }))}
                />
                {spec.help && <span className="fm-hint">{spec.help}</span>}
              </label>
            ))}
          <label className="fm-field">
            <span className="fm-label">Trigger</span>
            <select value={kind} onChange={(e) => setKind(e.target.value as "interval" | "cron")}>
              <option value="interval">interval (seconds)</option>
              <option value="cron">cron (UTC)</option>
            </select>
          </label>
          <label className="fm-field">
            <span className="fm-label">{kind === "cron" ? "Cron expression" : "Every (seconds)"}</span>
            <input
              value={expr}
              onChange={(e) => setExpr(e.target.value)}
              placeholder={kind === "cron" ? "0 3 * * *" : "3600"}
              required
            />
          </label>
          <label className="fm-field fm-field-wide">
            <span className="fm-label">Notify URL</span>
            <input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="notify_url (https://… — POSTed on a failed run)"
              required
            />
            <span className="fm-hint">POSTed to when a run fails.</span>
          </label>
        </div>
        <div className="fm-actions">
          <button type="submit" disabled={create.isPending}>
            Create
          </button>
        </div>
        {create.isError && <ErrorNote error={create.error} what="Creating the schedule" />}
      </form>
    </section>
  );
}
