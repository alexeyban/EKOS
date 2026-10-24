import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Link, useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import { type RunEvent, modelCalls, stepRows } from "./run-events";

interface Me {
  role: "read" | "write";
}
interface RunOut {
  id: string;
  workspace_id: string;
  command: string;
  status: string;
  stages: { name: string; status: string; exit_code: number | null }[];
  exit_code: number | null;
  log_tail: string[];
}

const TERMINAL = new Set([
  "succeeded",
  "failed",
  "cancelled",
  "timed_out",
  "interrupted",
]);

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
    preRef.current?.scrollTo(0, preRef.current.scrollHeight);
  }, [lines]);

  const cancel = useMutation({
    mutationFn: () => apiPost(`/runs/${runId}/cancel`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["run", runId] }),
  });

  const status = run.data?.status ?? "…";
  const nonTerminal = !TERMINAL.has(status);

  return (
    <>
      <p className="crumbs">
        <Link to={run.data ? `/w/${run.data.workspace_id}/runs` : "/"} className="linkish">
          ← runs
        </Link>
      </p>
      <section className="card">
        <strong>
          {run.data?.command ?? "run"} <span className={`chip ${chipClass(status)}`}>{status}</span>
          {run.data?.exit_code != null && (
            <span className="muted"> · exit {run.data.exit_code}</span>
          )}
        </strong>
        {run.data && run.data.stages.length > 0 && (
          <ul className="checks">
            {run.data.stages.map((s) => (
              <li key={s.name}>
                <span className={`dot ${dotClass(s.status)}`} /> {s.name} — {s.status}
                {s.exit_code != null && ` (exit ${s.exit_code})`}
              </li>
            ))}
          </ul>
        )}
        <div className="btnrow">
          {nonTerminal && me.role === "write" && (
            <button className="danger-btn" onClick={() => cancel.mutate()} disabled={cancel.isPending}>
              Cancel
            </button>
          )}
          {live && <span className="muted">streaming…</span>}
        </div>
        {events.data && events.data.length > 0 && <Steps events={events.data} />}
        <pre className="log" ref={preRef}>
          {lines.join("\n") || (run.data?.log_tail ?? []).join("\n")}
        </pre>
      </section>
    </>
  );
}

function Steps({ events }: { events: RunEvent[] }) {
  const calls = modelCalls(events);
  return (
    <>
      <h3>Steps</h3>
      {calls && (
        <p>
          Model calls:{" "}
          <span className={`chip ${calls.completions + calls.embeddings === 0 ? "ok" : "warn"}`}>
            {calls.completions} completion(s), {calls.embeddings} embedding(s)
          </span>
        </p>
      )}
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
    </>
  );
}

function chipClass(s: string): string {
  if (s === "succeeded") return "ok";
  if (["failed", "timed_out", "interrupted"].includes(s)) return "bad";
  if (s === "cancelled") return "warn";
  return "";
}
function dotClass(s: string): string {
  if (s === "succeeded") return "ok";
  if (s === "failed") return "fail";
  return "warn";
}
