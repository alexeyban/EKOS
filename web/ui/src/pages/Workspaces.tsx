import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router-dom";
import { api, apiDelete, apiPost } from "../api/client";
import type { Health, TenantSummary, Workspace } from "../api/types";

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
              {w.tenant_id && w.tenant_id !== "default" && (
                <span className="chip" title="tenant">
                  {w.tenant_id}
                </span>
              )}
              {w.restricted && (
                <span className="chip warn" title="you see part of this project">
                  restricted
                </span>
              )}
              {w.roles && w.roles.length > 0 && (
                <span className="muted ws-roles" title="your roles here">
                  {w.roles.join(", ")}
                </span>
              )}
              <ServerChip w={w} />
              {(!w.roles || w.roles.includes("admin")) && (
                <button
                  className="linkish danger"
                  onClick={() => remove.mutate(w.id)}
                  disabled={remove.isPending}
                >
                  remove
                </button>
              )}
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
  // RFC 0182: a project's server starts when something first needs it and stops when idle.
  if (!s)
    return (
      <span className="chip" title="its server starts when first needed">
        idle
      </span>
    );
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
  // "source": create a project in the EKOS home for a source directory (RFC 0179).
  // "path": register a directory that already holds a compiled workspace.
  const [mode, setMode] = useState<"source" | "path">("source");
  // Adding a workspace needs `admin` on its tenant (RFC 0180). With one such tenant there is
  // nothing to choose and the field is not shown.
  const tenants = useQuery({
    queryKey: ["tenants"],
    queryFn: () => api<TenantSummary[]>("/tenants"),
  });
  const adminOf = (tenants.data ?? []).filter((t) => t.roles.includes("admin"));
  const [tenant, setTenant] = useState("");
  const chosen = tenant || adminOf[0]?.id || "default";

  const add = useMutation({
    mutationFn: () => {
      const target = mode === "source" ? { source: path } : { path };
      const body = { id, name, ...target };
      return apiPost("/workspaces", chosen === "default" ? body : { ...body, tenant: chosen });
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["workspaces"] });
      setId("");
      setName("");
      setPath("");
    },
  });

  // Hidden for someone who administers no tenant. While the list is loading, or against an API
  // without tenants, the form stays: the API decides either way.
  if (tenants.data && adminOf.length === 0) return null;

  return (
    <section className="card">
      <strong>Add a workspace</strong>
      <div className="reg-mode">
        <label>
          <input
            type="radio"
            name="reg-mode"
            checked={mode === "source"}
            onChange={() => setMode("source")}
          />
          <span>New project from a source directory</span>
        </label>
        <label>
          <input
            type="radio"
            name="reg-mode"
            checked={mode === "path"}
            onChange={() => setMode("path")}
          />
          <span>Existing workspace</span>
        </label>
      </div>
      {mode === "source" ? (
        <p className="muted">
          An absolute path to the sources. The project&apos;s config and compiled data are kept in
          the EKOS home; nothing is written into the source directory. The id becomes the project
          name: lowercase letters, digits, <code>-</code> and <code>_</code>.
        </p>
      ) : (
        <p className="muted">
          An absolute path to a directory with <code>ekos.toml</code> and <code>.ekos/</code>. The
          console spawns its MCP server.
        </p>
      )}
      <form
        className="reg-form"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        {adminOf.length > 1 && (
          <select aria-label="tenant" value={chosen} onChange={(e) => setTenant(e.target.value)}>
            {adminOf.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        )}
        <input placeholder="id" value={id} onChange={(e) => setId(e.target.value)} required />
        <input
          placeholder="name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
        <input
          placeholder={mode === "source" ? "/abs/path/to/sources" : "/abs/path/to/workspace"}
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
