import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import { commandTitle, relativeTime } from "../format";
import { Empty, PageHead, QueryView } from "../ui";

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
      <PageHead
        title="History"
        sub="Every command run on this project, newest first. Open one for its stages and log."
        actions={
          <Link className="button primary" to={`/w/${id}/run`}>
            Run a command
          </Link>
        }
      />
      <section className="card" aria-label="run history">
        <QueryView
          q={runs}
          what="run history"
          rows={5}
          isEmpty={(r) => r.length === 0}
          empty={
            <Empty title="No runs yet" icon="history">
              Start one from <Link to={`/w/${id}/run`}>Run</Link>, or from the command palette.
            </Empty>
          }
        >
          {(rows) => (
            <div className="table-scroll">
              <table className="lm-table x-table">
                <thead>
                  <tr>
                    <th>command</th>
                    <th>status</th>
                    <th>started</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r) => (
                    <tr key={r.id}>
                      <td>
                        <Link to={`/runs/${r.id}`} className="x-row-link" title={commandTitle(r.command)}>
                          {r.command}
                        </Link>
                      </td>
                      <td>
                        <span className={`chip ${chip(r.status)}`}>{r.status}</span>
                      </td>
                      <td>
                        <code className="x-when" title={relativeTime(r.created_at)}>
                          {r.created_at.replace("T", " ").slice(0, 19)}
                        </code>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </QueryView>
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
