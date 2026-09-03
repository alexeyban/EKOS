import { useQuery } from "@tanstack/react-query";
import { api } from "./api/client";
import type { Health, Workspace } from "./api/types";

// RFC 0128 Phase 0 skeleton: prove the API + MCP wiring renders real data. The graph views,
// dashboard, config UX, and command runner are RFC 0127 Phases 1-7.
export function App() {
  const health = useQuery({ queryKey: ["health"], queryFn: () => api<Health>("/health") });
  const workspaces = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => api<Workspace[]>("/workspaces"),
  });

  return (
    <main>
      <h1>
        <span>EKOS</span> Console
      </h1>
      <p className="muted">RFC 0127 / RFC 0128 — Phase 0 skeleton.</p>

      <section className="card">
        <strong>API</strong>
        {health.isLoading && <p className="muted">checking…</p>}
        {health.isError && <p className="err">unreachable: {String(health.error)}</p>}
        {health.data && (
          <p className="muted">
            {health.data.service} <code>v{health.data.version}</code> — {health.data.status}
          </p>
        )}
      </section>

      <section className="card">
        <strong>Workspaces</strong>
        {workspaces.isError && (
          <p className="err">
            {String(workspaces.error)} — set a console token via{" "}
            <code>localStorage.setItem('ekos-console-token', '…')</code>
          </p>
        )}
        {workspaces.data && workspaces.data.length === 0 && (
          <p className="muted">none configured (EKOS_CONSOLE_WORKSPACES_JSON)</p>
        )}
        {workspaces.data && workspaces.data.length > 0 && (
          <ul>
            {workspaces.data.map((w) => (
              <li key={w.id}>
                {w.name} — <code>{w.path}</code>
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}
