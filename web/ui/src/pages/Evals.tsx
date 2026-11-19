import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import type { EvalReportSummary } from "../api/types";
import { Empty, PageHead, QueryView } from "../ui";

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
    <>
      <PageHead
        title="Evals"
        sub={
          <>
            Every saved <code>ekos eval run</code> report (RFC 0138): answer correctness, evidence
            groundedness and hallucination resistance against the checked-in scenario suite.
          </>
        }
        actions={
          <Link className="button" to={`/w/${id}/run`}>
            Start an eval run
          </Link>
        }
      />
      <section className="card" aria-label="eval history">
        <h2 className="card-label">Eval history</h2>
        <QueryView
          q={reports}
          what="eval reports"
          rows={4}
          isEmpty={(r) => r.length === 0}
          empty={
            <Empty title="No eval runs saved yet" icon="evals">
              Run <code>ekos eval run</code>, or <code>eval-run</code> from the{" "}
              <Link to={`/w/${id}/run`}>Run</Link> page. Each saved report shows up here.
            </Empty>
          }
        >
          {(rows) => <EvalTable id={id} rows={rows.slice().reverse()} />}
        </QueryView>
      </section>
    </>
  );
}

function EvalTable({ id, rows }: { id: string; rows: EvalReportSummary[] }) {
  return (
    <div className="table-scroll">
      <table className="lm-table x-table">
        <thead>
          <tr>
            <th>dataset</th>
            <th>agent</th>
            <th>gate</th>
            <th>scenarios</th>
            <th>answer</th>
            <th>hallucination</th>
            <th>generated</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.file}>
              <td>
                <Link to={`/w/${id}/evals/${r.file}`} className="x-row-link">
                  {r.dataset}
                </Link>
              </td>
              <td className="muted">{r.agent}</td>
              <td>
                <span className={`chip ${chip(r.status_pass)}`}>{r.status_pass ? "PASS" : "FAIL"}</span>
              </td>
              <td className="x-num">
                {r.passed}/{r.scenarios} passed
              </td>
              <td className="x-num">answer {pct(r.answer_correctness)}</td>
              <td className="x-num">halluc {pct(r.hallucination_rate)}</td>
              <td>
                <code className="x-when">{r.generated_at.replace("T", " ").slice(0, 19)}</code>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
