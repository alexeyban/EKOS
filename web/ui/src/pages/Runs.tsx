import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";

interface RunRow {
  id: string;
  command: string;
  status: string;
  created_at: string;
}

export function Runs() {
  const { id = "" } = useParams();
  const runs = useQuery({
    queryKey: ["runs", id],
    queryFn: () => api<RunRow[]>(`/runs?workspace=${id}&limit=100`),
    refetchInterval: 3000,
  });

  return (
    <>
      <section className="card">
        <strong>Run history</strong>
        {runs.data?.length === 0 && <p className="muted">nothing run yet</p>}
        <ul className="ws-list">
          {runs.data?.map((r) => (
            <li key={r.id}>
              <Link to={`/runs/${r.id}`}>{r.command}</Link>
              <span className={`chip ${chip(r.status)}`}>{r.status}</span>
              <code className="path">{r.created_at.replace("T", " ").slice(0, 19)}</code>
            </li>
          ))}
        </ul>
      </section>
    </>
  );
}

function chip(s: string): string {
  if (s === "succeeded") return "ok";
  if (["failed", "timed_out", "interrupted"].includes(s)) return "bad";
  if (s === "cancelled") return "warn";
  return "";
}
