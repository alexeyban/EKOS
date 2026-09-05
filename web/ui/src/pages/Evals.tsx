import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import type { EvalReportSummary } from "../api/types";

function pct(v: number | null): string {
  return v === null ? "n/a" : `${(v * 100).toFixed(1)}%`;
}

function chip(passed: boolean): string {
  return passed ? "ok" : "bad";
}

export function Evals() {
  const { id = "" } = useParams();
  const reports = useQuery({
    queryKey: ["evals", id],
    queryFn: () => api<EvalReportSummary[]>(`/workspaces/${id}/evals/reports`),
    refetchInterval: 10000,
  });

  return (
    <section className="card">
      <strong>Eval history</strong>
      <p className="muted">
        Every saved <code>ekos eval run</code> report (RFC 0138) — answer correctness, evidence
        groundedness, and hallucination resistance against the checked-in scenario suite. Trigger
        a new run from the <Link to={`/w/${id}/run`}>Run</Link> tab (<code>eval-run</code>).
      </p>
      {reports.isError && <p className="err">{String(reports.error)}</p>}
      {reports.data?.length === 0 && <p className="muted">no eval runs saved yet</p>}
      <ul className="ws-list">
        {reports.data
          ?.slice()
          .reverse()
          .map((r) => (
            <li key={r.file}>
              <Link to={`/w/${id}/evals/${r.file}`}>{r.dataset}</Link>
              <span className="muted">{r.agent}</span>
              <span className={`chip ${chip(r.status_pass)}`}>
                {r.status_pass ? "PASS" : "FAIL"}
              </span>
              <span className="muted">
                {r.passed}/{r.scenarios} passed
              </span>
              <span className="muted">answer {pct(r.answer_correctness)}</span>
              <span className="muted">halluc {pct(r.hallucination_rate)}</span>
              <code className="path">{r.generated_at.replace("T", " ").slice(0, 19)}</code>
            </li>
          ))}
      </ul>
    </section>
  );
}
