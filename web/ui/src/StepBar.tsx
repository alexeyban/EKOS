// RFC 0213 — the seven-step operator workflow across the top of a workspace's pages, from
// `ekos workflow status --json`. Each step says its state; the review step links to its page.

import { useQuery } from "@tanstack/react-query";
import { NavLink } from "react-router-dom";
import { api } from "./api/client";
import type { WorkflowStep } from "./api/types";
import { stepChip } from "./pages/review-shared";

const LINKS: Record<string, string> = { analyse: "", review: "review" };

export function StepBar({ workspace }: { workspace: string }) {
  const bar = useQuery({
    queryKey: ["workflow", workspace],
    queryFn: () => api<{ steps: WorkflowStep[] }>(`/workspaces/${workspace}/workflow`),
    staleTime: 60_000,
    retry: false,
  });
  if (!bar.data) return null;
  return (
    <ol className="step-bar" aria-label="workflow">
      {bar.data.steps.map((s) => {
        const to = LINKS[s.id];
        const body = (
          <>
            <span className="step-n">{s.n}</span> {s.title}{" "}
            <span className={stepChip(s.state)}>{s.state}</span>
          </>
        );
        return (
          <li key={s.n} title={s.detail} className={`step step-${s.state}`}>
            {to === undefined ? body : <NavLink to={`/w/${workspace}/${to}`}>{body}</NavLink>}
          </li>
        );
      })}
    </ol>
  );
}
