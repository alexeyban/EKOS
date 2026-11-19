import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import type { EvalReport } from "../api/types";
import { PageHead, QueryView } from "../ui";

function pct(v: number | null | undefined): string {
  return v === null || v === undefined ? "n/a" : `${(v * 100).toFixed(1)}%`;
}

function ms(v: number | null | undefined): string {
  if (v === null || v === undefined) return "n/a";
  return v >= 1000 ? `${(v / 1000).toFixed(1)}s` : `${Math.round(v)}ms`;
}

function tokens(v: number | null | undefined): string {
  return v === null || v === undefined ? "n/a" : Math.round(v).toLocaleString();
}

function kb(v: number | null | undefined): string {
  if (v === null || v === undefined) return "n/a";
  return v >= 1024 ? `${(v / 1024).toFixed(1)} MB` : `${v} KB`;
}

export function EvalDetail() {
  const { id = "", file = "" } = useParams();
  const report = useQuery({
    queryKey: ["evals", id, file],
    queryFn: () => api<EvalReport>(`/workspaces/${id}/evals/reports/${file}`),
  });

  return (
    <>
      <p className="crumbs">
        <Link to={`/w/${id}/evals`} className="linkish">
          ← Evals
        </Link>
      </p>
      <QueryView q={report} what="eval report" tiles={6}>
        {(r) => <ReportView r={r} />}
      </QueryView>
    </>
  );
}

function ReportView({ r }: { r: EvalReport }) {
  return (
    <>
      <PageHead
        title={r.dataset}
        sub={
          <>
            agent <code>{r.agent}</code> · runtime <code>{r.runtime}</code> ·{" "}
            {r.generated_at.replace("T", " ").slice(0, 19)} · {r.metrics.passed}/{r.metrics.scenarios} scenarios
            passed
          </>
        }
        actions={
          <span className={`chip ${r.metrics.status_pass ? "ok" : "bad"}`}>
            {r.metrics.status_pass ? "PASS" : "FAIL"}
          </span>
        }
      />

      <section className="tiles" aria-label="metrics">
        <Tile label="answer correctness" value={pct(r.metrics.answer_correctness)} />
        <Tile label="evidence groundedness" value={pct(r.metrics.evidence_groundedness)} />
        <Tile label="completeness" value={pct(r.metrics.completeness)} />
        <Tile label="recall@10" value={pct(r.metrics.recall_at_10)} />
        <Tile label="hallucination rate" value={pct(r.metrics.hallucination_rate)} />
        <Tile label="avg tokens" value={tokens(r.metrics.avg_tokens)} />
        <Tile label="p95 latency" value={ms(r.metrics.p95_latency_ms)} />
        <Tile label="cache hits" value={`${r.metrics.cache_hits}/${r.metrics.cache_hits + r.metrics.cache_misses}`} />
        <Tile label="tokens saved" value={tokens(r.metrics.tokens_saved)} />
        <Tile label="peak RSS" value={kb(r.metrics.peak_rss_kb)} />
        <Tile label="CPU time" value={ms(r.metrics.total_cpu_time_ms)} />
      </section>

      <section className="card" aria-label="scenarios">
        <h2 className="card-label">Scenarios</h2>
        <div className="table-scroll">
          <table className="lm-table x-table">
            <thead>
              <tr>
                <th>scenario</th>
                <th>result</th>
                <th>answer</th>
                <th>ground</th>
                <th>complete</th>
                <th>recall</th>
                <th>latency</th>
              </tr>
            </thead>
            <tbody>
              {r.scenarios.map((s) => (
                <tr key={s.id}>
                  <td>
                    <code>{s.id}</code>
                  </td>
                  <td>
                    <span className="x-chips">
                      <span className={`chip ${s.passed ? "ok" : "bad"}`}>{s.passed ? "pass" : "fail"}</span>
                      {s.hallucinated && <span className="chip bad">hallucinated</span>}
                    </span>
                  </td>
                  <td className="x-num">{pct(s.answer_score)}</td>
                  <td className="x-num">{pct(s.groundedness_score)}</td>
                  <td className="x-num">{pct(s.completeness_score)}</td>
                  <td className="x-num">{s.retrieval_recall === null ? "" : pct(s.retrieval_recall)}</td>
                  <td className="x-num">{s.error ? <span className="err">{s.error}</span> : ms(s.latency_ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </>
  );
}

function Tile({ label, value }: { label: string; value: string }) {
  return (
    <div className="tile">
      <div className="tile-value">{value}</div>
      <div className="tile-label">{label}</div>
    </div>
  );
}
