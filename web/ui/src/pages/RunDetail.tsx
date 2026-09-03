import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Link, useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";

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

  const [lines, setLines] = useState<string[]>([]);
  const [live, setLive] = useState(true);
  const preRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    const es = new EventSource(`/api/runs/${runId}/logs`, { withCredentials: true });
    es.onmessage = (e) => setLines((l) => [...l, e.data]);
    es.addEventListener("end", () => {
      setLive(false);
      es.close();
      qc.invalidateQueries({ queryKey: ["run", runId] });
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
        <pre className="log" ref={preRef}>
          {lines.join("\n") || (run.data?.log_tail ?? []).join("\n")}
        </pre>
      </section>
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
