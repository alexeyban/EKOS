import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, getToken, setToken } from "./api/client";
import type { Health, Stats, Workspace } from "./api/types";

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

      <TokenCard />

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
            {String(workspaces.error)} — set a console token above.
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
                <WorkspaceStats id={w.id} />
              </li>
            ))}
          </ul>
        )}
      </section>
    </main>
  );
}

function TokenCard() {
  const qc = useQueryClient();
  const [value, setValue] = useState(getToken());
  const saved = getToken();

  return (
    <section className="card">
      <strong>Console token</strong>
      <p className="muted">
        Sent as <code>Authorization: Bearer …</code> on every call except <code>/api/health</code>.
        Stored in <code>localStorage</code> for the skeleton.
      </p>
      <form
        className="token-row"
        onSubmit={(e) => {
          e.preventDefault();
          setToken(value.trim());
          qc.invalidateQueries();
        }}
      >
        <input
          type="password"
          value={value}
          placeholder="console token"
          onChange={(e) => setValue(e.target.value)}
          aria-label="console token"
        />
        <button type="submit">Save</button>
      </form>
      <p className="muted">{saved ? "token set" : "no token set"}</p>
    </section>
  );
}

function WorkspaceStats({ id }: { id: string }) {
  const stats = useQuery({
    queryKey: ["stats", id],
    queryFn: () => api<Stats>(`/workspaces/${id}/stats`),
    retry: false,
  });

  if (stats.isError) return <span className="err"> — stats: {String(stats.error)}</span>;
  if (!stats.data) return <span className="muted"> — loading stats…</span>;
  return (
    <span className="muted">
      {" "}
      — {stats.data.entries.toLocaleString()} entries · {stats.data.objects.toLocaleString()}{" "}
      objects · {(stats.data.relationships ?? 0).toLocaleString()} relationships
    </span>
  );
}
