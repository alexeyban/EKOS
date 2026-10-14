import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { apiPost } from "./api/client";

// RFC 0189: one button that makes the open project queryable — init (in-place workspaces
// only), build, recover, resolve, compile, commit and coverage, as one run.
export function InitAndCompile({ workspace, name }: { workspace: string; name: string }) {
  const nav = useNavigate();
  const run = useMutation({
    mutationFn: () =>
      apiPost<{ run_id: string }>(`/workspaces/${encodeURIComponent(workspace)}/commands/init-and-compile`),
    onSuccess: (r) => nav(`/runs/${r.run_id}`),
  });
  return (
    <span className="ws-nav-action">
      <button
        className="primary"
        disabled={run.isPending}
        title="init → build → recover → resolve → compile → commit → coverage"
        onClick={() => {
          if (
            window.confirm(
              `Initialise and compile ${name}? This runs build, recover, resolve, compile and commit, then reports coverage.`,
            )
          ) {
            run.mutate();
          }
        }}
      >
        {run.isPending ? "Starting…" : "Init & compile"}
      </button>
      {run.isError && <span className="err"> {String(run.error)}</span>}
    </span>
  );
}
