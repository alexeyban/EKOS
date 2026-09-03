import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router-dom";
import { api, apiDelete, apiPost } from "../api/client";
import type { Health, Workspace } from "../api/types";

export function Workspaces() {
  const qc = useQueryClient();
  const health = useQuery({ queryKey: ["health"], queryFn: () => api<Health>("/health") });
  const workspaces = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => api<Workspace[]>("/workspaces"),
    refetchInterval: 4000,
  });

  const remove = useMutation({
    mutationFn: (id: string) => apiDelete(`/workspaces/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["workspaces"] }),
  });

  return (
    <>
      <section className="card">
        <strong>API</strong>{" "}
        {health.isError ? (
          <span className="err">unreachable: {String(health.error)}</span>
        ) : health.data ? (
          <span className="muted">
            {health.data.service} <code>v{health.data.version}</code> — {health.data.status}
          </span>
        ) : (
          <span className="muted">checking…</span>
        )}
      </section>

      <section className="card">
        <strong>Workspaces</strong>
        {workspaces.isError && (
          <p className="err">
            {String(workspaces.error)} — set a console token above.
          </p>
        )}
        {workspaces.data?.length === 0 && <p className="muted">none registered yet</p>}
        <ul className="ws-list">
          {workspaces.data?.map((w) => (
            <li key={w.id}>
              <Link to={`/w/${w.id}`}>{w.name}</Link>
              <code className="path">{w.path}</code>
              <ServerChip w={w} />
              <button
                className="linkish danger"
                onClick={() => remove.mutate(w.id)}
                disabled={remove.isPending}
              >
                remove
              </button>
            </li>
          ))}
        </ul>
      </section>

      <RegisterForm />
    </>
  );
}

function ServerChip({ w }: { w: Workspace }) {
  const s = w.server;
  if (!s) return <span className="chip">no server</span>;
  const cls =
    s.state === "ready" ? "chip ok" : s.state === "failed" ? "chip bad" : "chip warn";
  const label =
    s.state === "failed" && s.retries ? `failed (${s.retries} retries)` : s.state;
  return (
    <span className={cls} title={s.detail || undefined}>
      {label}
    </span>
  );
}

function RegisterForm() {
  const qc = useQueryClient();
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [path, setPath] = useState("");

  const add = useMutation({
    mutationFn: () => apiPost("/workspaces", { id, name, path }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["workspaces"] });
      setId("");
      setName("");
      setPath("");
    },
  });

  return (
    <section className="card">
      <strong>Register a workspace</strong>
      <p className="muted">
        An absolute path to a directory with <code>ekos.toml</code> and <code>.ekos/</code>. The
        console spawns its MCP server.
      </p>
      <form
        className="reg-form"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        <input placeholder="id" value={id} onChange={(e) => setId(e.target.value)} required />
        <input
          placeholder="name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
        <input
          placeholder="/abs/path/to/workspace"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          required
        />
        <button type="submit" disabled={add.isPending}>
          Add
        </button>
      </form>
      {add.isError && <p className="err">{String(add.error)}</p>}
    </section>
  );
}
