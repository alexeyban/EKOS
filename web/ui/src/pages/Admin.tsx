// Tenant administration (RFC 0180): who holds which role, groups, API tokens and the audit log
// of one tenant, and for a platform owner creating tenants and setting their source roots.
// Everything shown here is what the API allowed the caller to read; hiding a control is a
// convenience, never the check.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext } from "react-router-dom";
import { api, apiDelete, apiPatch, apiPost } from "../api/client";
import type { RoleName, TenantSummary, Workspace } from "../api/types";

interface Me {
  owner?: boolean;
}

interface Binding {
  id: string;
  principal_kind: "user" | "group" | "token";
  principal: string;
  role: RoleName;
  workspace_id: string | null;
  // RFC 0181: a rule narrower than the workspace.
  effect?: "allow" | "deny";
  source?: string | null;
  path?: string | null;
  object?: string | null;
  warning?: string;
}

// How a rule reads in the table: "only source payments, path src/**" or "except path secrets/**".
function describeRule(b: Binding): string | null {
  const parts = [
    b.source ? `source ${b.source}` : null,
    b.path ? `path ${b.path}` : null,
    b.object ? `object ${b.object}` : null,
  ].filter(Boolean);
  if (parts.length === 0) return null;
  return `${b.effect === "deny" ? "except" : "only"} ${parts.join(", ")}`;
}

interface Group {
  id: string;
  name: string;
  oidc_value: string;
  members: { id: string; email: string }[];
}

interface Token {
  id: string;
  name: string;
  prefix: string;
  roles: { role: string; workspace_id: string | null }[];
  expires_at: string | null;
  last_used_at: string | null;
  revoked: boolean;
  token?: string;
}

interface AuditEntry {
  at: string;
  actor: string;
  action: string;
  workspace_id: string;
  outcome: string;
}

const ROLES: RoleName[] = ["viewer", "reviewer", "operator", "admin"];
const ROLE_HELP: Record<RoleName, string> = {
  viewer: "read everything",
  reviewer: "read, and decide semantics and conflicts",
  operator: "read, run commands, schedules, edit the config",
  admin: "everything in the tenant, including members and tokens",
};

const when = (iso: string | null) => (iso ? iso.replace("T", " ").slice(0, 16) : "never");

export function Admin() {
  const me = useOutletContext<Me>();
  const tenants = useQuery({
    queryKey: ["tenants"],
    queryFn: () => api<TenantSummary[]>("/tenants"),
  });
  const manageable = (tenants.data ?? []).filter((t) => me.owner || t.roles.includes("admin"));
  const [picked, setPicked] = useState("");
  const tenant = manageable.find((t) => t.id === picked) ?? manageable[0];

  if (tenants.isError) return <p className="err">{String(tenants.error)}</p>;
  if (!tenants.data) return <p className="muted">loading…</p>;

  return (
    <>
      <section className="card">
        <strong>Administration</strong>
        {manageable.length === 0 ? (
          <p className="muted">You do not administer any tenant.</p>
        ) : (
          <div className="reg-form">
            <label htmlFor="admin-tenant">Tenant</label>
            <select
              id="admin-tenant"
              value={tenant?.id ?? ""}
              onChange={(e) => setPicked(e.target.value)}
            >
              {manageable.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name} ({t.id})
                </option>
              ))}
            </select>
          </div>
        )}
      </section>

      {me.owner && <Tenants tenant={tenant} />}
      {tenant && (
        <>
          <Bindings tenant={tenant.id} />
          <Groups tenant={tenant.id} />
          <Tokens tenant={tenant.id} />
          <Audit tenant={tenant.id} />
        </>
      )}
    </>
  );
}

// ── tenants (platform owner) ──────────────────────────────────────────────────

function Tenants({ tenant }: { tenant: TenantSummary | undefined }) {
  const qc = useQueryClient();
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [roots, setRoots] = useState<string | null>(null);
  const shownRoots = roots ?? (tenant?.source_roots ?? []).join("\n");
  const lines = (text: string) =>
    text
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);

  const create = useMutation({
    mutationFn: () => apiPost("/tenants", { id, name: name || id }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["tenants"] });
      setId("");
      setName("");
    },
  });
  const saveRoots = useMutation({
    mutationFn: () => apiPatch(`/tenants/${tenant?.id}`, { source_roots: lines(shownRoots) }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["tenants"] });
      setRoots(null);
    },
  });

  return (
    <section className="card">
      <strong>Tenants</strong>
      <p className="muted">Only a platform owner creates a tenant or sets its source roots.</p>
      <form
        className="reg-form"
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate();
        }}
      >
        <input
          placeholder="tenant id"
          aria-label="tenant id"
          value={id}
          onChange={(e) => setId(e.target.value)}
          required
        />
        <input
          placeholder="display name"
          aria-label="tenant display name"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <button type="submit" disabled={create.isPending}>
          Create tenant
        </button>
      </form>
      {create.isError && <p className="err">{String(create.error)}</p>}

      {tenant && (
        <>
          <label htmlFor="tenant-roots" className="admin-label">
            Source roots of {tenant.name}
          </label>
          <p className="muted">
            One absolute directory per line. A project of this tenant may only read directories
            under these.
          </p>
          <textarea
            id="tenant-roots"
            className="admin-roots"
            rows={3}
            value={shownRoots}
            onChange={(e) => setRoots(e.target.value)}
          />
          <div>
            <button
              onClick={() => saveRoots.mutate()}
              disabled={saveRoots.isPending || roots === null}
            >
              Save source roots
            </button>
          </div>
          {saveRoots.isError && <p className="err">{String(saveRoots.error)}</p>}
        </>
      )}
    </section>
  );
}

// ── role bindings ─────────────────────────────────────────────────────────────

function WorkspaceSelect({
  tenant,
  value,
  onChange,
}: {
  tenant: string;
  value: string;
  onChange: (v: string) => void;
}) {
  const workspaces = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => api<Workspace[]>("/workspaces"),
  });
  const mine = (workspaces.data ?? []).filter((w) => (w.tenant_id ?? "default") === tenant);
  return (
    <select aria-label="scope" value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="">whole tenant</option>
      {mine.map((w) => (
        <option key={w.id} value={w.id}>
          {w.name}
        </option>
      ))}
    </select>
  );
}

function Bindings({ tenant }: { tenant: string }) {
  const qc = useQueryClient();
  const key = ["bindings", tenant];
  const bindings = useQuery({
    queryKey: key,
    queryFn: () => api<Binding[]>(`/tenants/${tenant}/bindings`),
  });
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<RoleName>("viewer");
  const [workspace, setWorkspace] = useState("");
  // RFC 0181: limit a viewer to part of one workspace, or hide part of it from them.
  const [limit, setLimit] = useState<"" | "allow" | "deny">("");
  const [source, setSource] = useState("");
  const [path, setPath] = useState("");
  const canLimit = role === "viewer" && workspace !== "";
  const rule = canLimit && limit && (source || path);

  const add = useMutation({
    mutationFn: () =>
      apiPost<Binding>(`/tenants/${tenant}/bindings`, {
        email,
        role,
        ...(workspace ? { workspace_id: workspace } : {}),
        ...(rule
          ? { effect: limit, ...(source ? { source } : {}), ...(path ? { path } : {}) }
          : {}),
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: key });
      setEmail("");
      setSource("");
      setPath("");
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => apiDelete(`/tenants/${tenant}/bindings/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  });

  return (
    <section className="card">
      <strong>Roles</strong>
      {bindings.isError && <p className="err">{String(bindings.error)}</p>}
      {bindings.data?.length === 0 && <p className="muted">nobody has a role here yet</p>}
      {bindings.data && bindings.data.length > 0 && (
        <table className="lm-table">
          <thead>
            <tr>
              <th>Who</th>
              <th>Role</th>
              <th>On</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {bindings.data.map((b) => (
              <tr key={b.id}>
                <td>
                  {b.principal}
                  {b.principal_kind !== "user" && (
                    <span className="chip admin-kind">{b.principal_kind}</span>
                  )}
                </td>
                <td>{b.role}</td>
                <td>
                  {b.workspace_id ?? "whole tenant"}
                  {describeRule(b) && <span className="muted admin-rule">{describeRule(b)}</span>}
                </td>
                <td>
                  <button
                    className="linkish danger"
                    aria-label={`remove ${b.role} from ${b.principal}`}
                    onClick={() => remove.mutate(b.id)}
                  >
                    remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <form
        className="reg-form"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        <input
          type="email"
          placeholder="person@example.com"
          aria-label="e-mail"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
        />
        <select aria-label="role" value={role} onChange={(e) => setRole(e.target.value as RoleName)}>
          {ROLES.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        {role !== "admin" && (
          <WorkspaceSelect tenant={tenant} value={workspace} onChange={setWorkspace} />
        )}
        {canLimit && (
          <>
            <select
              aria-label="limit"
              value={limit}
              onChange={(e) => setLimit(e.target.value as "" | "allow" | "deny")}
            >
              <option value="">all of it</option>
              <option value="allow">only…</option>
              <option value="deny">all except…</option>
            </select>
            {limit && (
              <>
                <input
                  placeholder="source name"
                  aria-label="source"
                  value={source}
                  onChange={(e) => setSource(e.target.value)}
                />
                <input
                  placeholder="path, e.g. secrets/**"
                  aria-label="path"
                  value={path}
                  onChange={(e) => setPath(e.target.value)}
                />
              </>
            )}
          </>
        )}
        <button type="submit" disabled={add.isPending || (canLimit && limit !== "" && !rule)}>
          Grant
        </button>
      </form>
      {canLimit && limit && (
        <p className="muted">
          Someone limited this way sees the graph and search only, filtered to what the rule
          leaves. Operators and admins cannot be limited: running the pipeline shows every
          source.
        </p>
      )}
      {add.data?.warning && <p className="err">{add.data.warning}</p>}
      <p className="muted">
        {role}: {ROLE_HELP[role]}. A person who has not signed in yet gets the role at their first
        sign-in with this address.
      </p>
      {add.isError && <p className="err">{String(add.error)}</p>}
    </section>
  );
}

// ── groups ────────────────────────────────────────────────────────────────────

function Groups({ tenant }: { tenant: string }) {
  const qc = useQueryClient();
  const key = ["groups", tenant];
  const groups = useQuery({
    queryKey: key,
    queryFn: () => api<Group[]>(`/tenants/${tenant}/groups`),
  });
  const [name, setName] = useState("");
  const [claim, setClaim] = useState("");
  const [role, setRole] = useState<RoleName>("viewer");
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: key });
    void qc.invalidateQueries({ queryKey: ["bindings", tenant] });
  };

  const create = useMutation({
    mutationFn: async () => {
      const group = await apiPost<Group>(`/tenants/${tenant}/groups`, {
        name,
        oidc_value: claim,
      });
      await apiPost(`/tenants/${tenant}/bindings`, { group_id: group.id, role });
    },
    onSuccess: () => {
      refresh();
      setName("");
      setClaim("");
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => apiDelete(`/tenants/${tenant}/groups/${id}`),
    onSuccess: refresh,
  });

  return (
    <section className="card">
      <strong>Groups</strong>
      <p className="muted">
        A group holds a role for all its members. With a claim value, everyone whose sign-in
        carries that value is a member without being listed.
      </p>
      {groups.isError && <p className="err">{String(groups.error)}</p>}
      <ul className="admin-list">
        {groups.data?.map((g) => (
          <li key={g.id}>
            <span>
              {g.name}
              {g.oidc_value && <code className="path">{g.oidc_value}</code>}
            </span>
            <GroupMembers tenant={tenant} group={g} onChange={refresh} />
            <button
              className="linkish danger"
              aria-label={`delete group ${g.name}`}
              onClick={() => remove.mutate(g.id)}
            >
              delete
            </button>
          </li>
        ))}
      </ul>
      <form
        className="reg-form"
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate();
        }}
      >
        <input
          placeholder="group name"
          aria-label="group name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
        <input
          placeholder="claim value (optional)"
          aria-label="claim value"
          value={claim}
          onChange={(e) => setClaim(e.target.value)}
        />
        <select
          aria-label="group role"
          value={role}
          onChange={(e) => setRole(e.target.value as RoleName)}
        >
          {ROLES.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
        <button type="submit" disabled={create.isPending}>
          Create group
        </button>
      </form>
      {create.isError && <p className="err">{String(create.error)}</p>}
    </section>
  );
}

function GroupMembers({
  tenant,
  group,
  onChange,
}: {
  tenant: string;
  group: Group;
  onChange: () => void;
}) {
  const [email, setEmail] = useState("");
  const base = `/tenants/${tenant}/groups/${group.id}/members`;
  const add = useMutation({
    mutationFn: () => apiPost(base, { email }),
    onSuccess: () => {
      onChange();
      setEmail("");
    },
  });
  const remove = useMutation({
    mutationFn: (userId: string) => apiDelete(`${base}/${userId}`),
    onSuccess: onChange,
  });
  return (
    <span className="admin-members">
      {group.members.map((m) => (
        <span key={m.id} className="chip">
          {m.email}{" "}
          <button
            className="linkish"
            aria-label={`remove ${m.email} from ${group.name}`}
            onClick={() => remove.mutate(m.id)}
          >
            ×
          </button>
        </span>
      ))}
      <form
        className="admin-inline"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        <input
          type="email"
          placeholder="add member by e-mail"
          aria-label={`add member to ${group.name}`}
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
        />
        <button type="submit" disabled={add.isPending}>
          Add
        </button>
      </form>
      {add.isError && <span className="err">{String(add.error)}</span>}
    </span>
  );
}

// ── API tokens ────────────────────────────────────────────────────────────────

function Tokens({ tenant }: { tenant: string }) {
  const qc = useQueryClient();
  const key = ["tokens", tenant];
  const tokens = useQuery({
    queryKey: key,
    queryFn: () => api<Token[]>(`/tenants/${tenant}/tokens`),
  });
  const [name, setName] = useState("");
  const [role, setRole] = useState<"viewer" | "operator">("viewer");
  const [workspace, setWorkspace] = useState("");
  const [days, setDays] = useState("90");
  const [created, setCreated] = useState<Token | null>(null);

  const create = useMutation({
    mutationFn: () =>
      apiPost<Token>(`/tenants/${tenant}/tokens`, {
        name,
        role,
        ...(workspace ? { workspace_id: workspace } : {}),
        ...(days ? { expires_days: Number(days) } : {}),
      }),
    onSuccess: (token) => {
      setCreated(token);
      setName("");
      void qc.invalidateQueries({ queryKey: key });
    },
  });
  const revoke = useMutation({
    mutationFn: (id: string) => apiDelete(`/tenants/${tenant}/tokens/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  });

  return (
    <section className="card">
      <strong>API tokens</strong>
      <p className="muted">
        For agents and scripts: <code>Authorization: Bearer &lt;token&gt;</code>. A token can view
        or operate. It can never review or administer; those need a signed-in person.
      </p>
      <p className="muted">
        An MCP client connects to{" "}
        <code>
          {window.location.origin}/mcp/{tenant}/&lt;project&gt;
        </code>{" "}
        with that header. It gets the tools its role allows, limited to what its rules let it
        see, and every call is in the audit log below.
      </p>
      {created?.token && (
        <div className="admin-secret" role="status">
          <p>
            Token <strong>{created.name}</strong> created. Copy it now: it is not stored and will
            not be shown again.
          </p>
          <code>{created.token}</code>
          <div>
            <button onClick={() => setCreated(null)}>I have copied it</button>
          </div>
        </div>
      )}
      {tokens.isError && <p className="err">{String(tokens.error)}</p>}
      {tokens.data && tokens.data.length > 0 && (
        <table className="lm-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Starts with</th>
              <th>Role</th>
              <th>Expires</th>
              <th>Last used</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {tokens.data.map((t) => (
              <tr key={t.id}>
                <td>{t.name}</td>
                <td>
                  <code>{t.prefix}…</code>
                </td>
                <td>
                  {t.roles.map((r) => `${r.role}${r.workspace_id ? ` on ${r.workspace_id}` : ""}`).join(", ")}
                </td>
                <td>{when(t.expires_at)}</td>
                <td>{when(t.last_used_at)}</td>
                <td>
                  {t.revoked ? (
                    <span className="chip bad">revoked</span>
                  ) : (
                    <button
                      className="linkish danger"
                      aria-label={`revoke ${t.name}`}
                      onClick={() => revoke.mutate(t.id)}
                    >
                      revoke
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <form
        className="reg-form"
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate();
        }}
      >
        <input
          placeholder="token name"
          aria-label="token name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
        <select
          aria-label="token role"
          value={role}
          onChange={(e) => setRole(e.target.value as "viewer" | "operator")}
        >
          <option value="viewer">viewer</option>
          <option value="operator">operator</option>
        </select>
        <WorkspaceSelect tenant={tenant} value={workspace} onChange={setWorkspace} />
        <input
          type="number"
          min={1}
          max={3650}
          placeholder="days"
          aria-label="expires after days"
          value={days}
          onChange={(e) => setDays(e.target.value)}
        />
        <button type="submit" disabled={create.isPending}>
          Create token
        </button>
      </form>
      {create.isError && <p className="err">{String(create.error)}</p>}
    </section>
  );
}

// ── audit log ─────────────────────────────────────────────────────────────────

function Audit({ tenant }: { tenant: string }) {
  const audit = useQuery({
    queryKey: ["audit", tenant],
    queryFn: () => api<AuditEntry[]>(`/tenants/${tenant}/audit?limit=100`),
  });
  return (
    <section className="card">
      <strong>Audit log</strong>
      <p className="muted">
        Every request that changes something, and whether it was allowed. It records that a
        request was permitted, not that the command it started succeeded: that is in the run
        history.
      </p>
      {audit.isError && <p className="err">{String(audit.error)}</p>}
      {audit.data?.length === 0 && <p className="muted">nothing recorded yet</p>}
      {audit.data && audit.data.length > 0 && (
        <div className="admin-scroll">
          <table className="lm-table">
            <thead>
              <tr>
                <th>When (UTC)</th>
                <th>Who</th>
                <th>What</th>
                <th>Workspace</th>
                <th>Outcome</th>
              </tr>
            </thead>
            <tbody>
              {audit.data.map((e, i) => (
                <tr key={`${e.at}-${i}`}>
                  <td>{when(e.at)}</td>
                  <td>{e.actor}</td>
                  <td>{e.action}</td>
                  <td>{e.workspace_id}</td>
                  <td>
                    <span className={e.outcome === "denied" ? "chip bad" : "chip ok"}>
                      {e.outcome}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
