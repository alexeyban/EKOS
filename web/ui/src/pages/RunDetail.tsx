import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Link, useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import { Icon } from "../Icon";
import { commandTitle, relativeTime } from "../format";
import { type RunEvent, clock, modelCalls, stageTimes, stepRows } from "./run-events";

interface Me {
  role: "read" | "write";
}
export interface RunOut {
  id: string;
  workspace_id: string;
  command: string;
  status: string;
  stages: { name: string; status: string; exit_code: number | null }[];
  exit_code: number | null;
  log_tail: string[];
  created_at?: string;
  started_at?: string | null;
  ended_at?: string | null;
}

const TERMINAL = new Set(["succeeded", "failed", "cancelled", "timed_out", "interrupted"]);

/** The header chip: the status, and for a staged run in progress where it is. */
export function runSummary(run: Pick<RunOut, "status" | "stages">): string {
  const n = run.stages.length;
  if (run.status === "running" && n > 0) {
    const at = run.stages.findIndex((s) => s.status === "running");
    if (at >= 0) return `running · stage ${at + 1} of ${n}`;
  }
  if (run.status === "succeeded" && n > 0) return `succeeded · all ${n} stages done`;
  return run.status.replace("_", " ");
}

export function RunDetail() {
  const { runId = "" } = useParams();
  const me = useOutletContext<Me>();
  const qc = useQueryClient();

  const run = useQuery({
    queryKey: ["run", runId],
    queryFn: () => api<RunOut>(`/runs/${runId}`),
    refetchInterval: (q) => (q.state.data && TERMINAL.has(q.state.data.status) ? false : 1000),
  });

  // RFC 0220: the CLI's structured log of this run, refreshed while it runs.
  const events = useQuery({
    queryKey: ["run-events", runId],
    queryFn: () => api<RunEvent[]>(`/runs/${runId}/events`),
    refetchInterval: () => (run.data && TERMINAL.has(run.data.status) ? false : 2000),
  });

  const [lines, setLines] = useState<string[]>([]);
  const [live, setLive] = useState(true);
  const [follow, setFollow] = useState(true);
  const preRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    const es = new EventSource(`/api/runs/${runId}/logs`, { withCredentials: true });
    es.onmessage = (e) => setLines((l) => [...l, e.data]);
    es.addEventListener("end", () => {
      setLive(false);
      es.close();
      void qc.invalidateQueries({ queryKey: ["run", runId] });
    });
    es.onerror = () => {
      setLive(false);
      es.close();
    };
    return () => es.close();
  }, [runId, qc]);

  useEffect(() => {
    if (follow) preRef.current?.scrollTo(0, preRef.current.scrollHeight);
  }, [lines, follow]);

  const cancel = useMutation({
    mutationFn: () => apiPost(`/runs/${runId}/cancel`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["run", runId] }),
  });

  const status = run.data?.status ?? "…";
  const nonTerminal = !TERMINAL.has(status);
  const shown = lines.length > 0 ? lines : (run.data?.log_tail ?? []);
  const times = stageTimes(events.data ?? []);

  return (
    <div className="run-page">
      <p className="crumbs">
        <Link to={run.data ? `/w/${run.data.workspace_id}/runs` : "/"} className="linkish">
          ← History
        </Link>
      </p>
      <div className="page-head">
        <div>
          <h1 className="page-title">{run.data ? commandTitle(run.data.command) : "Run"}</h1>
          {run.data && (
            <p className="page-sub">
              <span className={`chip ${chipClass(status)}`}>{runSummary(run.data)}</span>
              {run.data.exit_code != null && <span className="muted">exit {run.data.exit_code}</span>}
              {run.data.started_at && (
                <span className="muted">started {relativeTime(run.data.started_at)}</span>
              )}
            </p>
          )}
        </div>
        {nonTerminal && me.role === "write" && (
          <button className="danger-outline" onClick={() => cancel.mutate()} disabled={cancel.isPending}>
            <Icon name="close" /> Cancel run
          </button>
        )}
      </div>
      {cancel.isError && <p className="err">{String(cancel.error)}</p>}

      <div className={run.data && run.data.stages.length > 0 ? "run-grid" : undefined}>
        {run.data && run.data.stages.length > 0 && (
          <section className="card stages-card" aria-label="stages">
            <div className="card-label">Stages</div>
            <ol className="stage-list">
              {run.data.stages.map((s) => (
                <li key={s.name} className={`stage stage-${s.status}`}>
                  <StageMark status={s.status} />
                  <code className="stage-name">{s.name}</code>
                  <span className="stage-time">
                    {s.status === "pending" || s.status === "queued"
                      ? "queued"
                      : times[s.name] !== undefined
                        ? clock(times[s.name])
                        : s.status}
                    {s.exit_code != null && s.exit_code !== 0 && ` · exit ${s.exit_code}`}
                  </span>
                </li>
              ))}
            </ol>
          </section>
        )}

        <section className="card log-card" aria-label="log">
          <div className="log-head">
            <strong>Log</strong>
            {live && nonTerminal && <span className="chip ok">live</span>}
            <span className="topbar-gap" />
            <button type="button" aria-pressed={follow} onClick={() => setFollow(!follow)}>
              {follow ? "Following" : "Follow"}
            </button>
            <button type="button" onClick={() => saveLog(runId, shown)} disabled={shown.length === 0}>
              <Icon name="download" /> Save log…
            </button>
          </div>
          <pre className="log" ref={preRef}>
            {shown.join("\n") || (nonTerminal ? "working…" : "")}
          </pre>
        </section>
      </div>

      {events.data && events.data.length > 0 && <Steps events={events.data} />}
    </div>
  );
}

function StageMark({ status }: { status: string }) {
  if (status === "succeeded" || status === "done")
    return (
      <span className="stage-mark done" aria-label="done">
        <Icon name="check" size={12} />
      </span>
    );
  if (status === "running") return <span className="stage-mark running" aria-label="running" />;
  if (["failed", "timed_out", "interrupted", "cancelled"].includes(status))
    return (
      <span className="stage-mark failed" aria-label={status}>
        <Icon name="close" size={12} />
      </span>
    );
  return <span className="stage-mark" aria-label="queued" />;
}

function saveLog(runId: string, lines: string[]) {
  const url = URL.createObjectURL(new Blob([lines.join("\n") + "\n"], { type: "text/plain" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = `run-${runId}.log`;
  a.click();
  URL.revokeObjectURL(url);
}

function Steps({ events }: { events: RunEvent[] }) {
  const calls = modelCalls(events);
  return (
    <section className="card">
      <div className="card-label">Steps</div>
      {calls && (
        <p>
          Model calls:{" "}
          <span className={`chip ${calls.completions + calls.embeddings === 0 ? "ok" : "warn"}`}>
            {calls.completions} completion(s), {calls.embeddings} embedding(s)
          </span>
        </p>
      )}
      <div className="table-scroll">
        <table className="lm-table">
          <thead>
            <tr>
              <th>command</th>
              <th>stage</th>
              <th>what</th>
              <th>counts</th>
              <th>ms</th>
            </tr>
          </thead>
          <tbody>
            {stepRows(events).map((r, i) => (
              <tr key={i}>
                <td>{r.cmd}</td>
                <td>{r.tone ? <span className={`chip ${r.tone}`}>{r.stage}</span> : r.stage}</td>
                <td>{r.msg}</td>
                <td className="small">{r.counts}</td>
                <td className="small">{r.tMs ?? ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function chipClass(s: string): string {
  if (s === "succeeded") return "ok";
  if (["failed", "timed_out", "interrupted"].includes(s)) return "bad";
  if (s === "cancelled") return "warn";
  if (s === "running") return "accent";
  return "";
}
