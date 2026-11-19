// Tenant administration (RFC 0180): who holds which role, groups, API tokens and the audit log
// of one tenant, and for a platform owner creating tenants and setting their source roots.
// Everything shown here is what the API allowed the caller to read; hiding a control is a
// convenience, never the check.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext } from "react-router-dom";
import { api, apiDelete, apiPatch, apiPost } from "../api/client";
import type { RoleName, TenantSummary, Workspace } from "../api/types";
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";

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

  return (
    <div className="fm-page">
      <PageHead
        title="Admin"
        sub="Who holds which role in a tenant, its groups and API tokens, and the audit log of what was asked."
        actions={
          manageable.length > 0 && (
            <label className="fm-inline">
              <span className="fm-label">Tenant</span>
              <select id="admin-tenant" value={tenant?.id ?? ""} onChange={(e) => setPicked(e.target.value)}>
                {manageable.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name} ({t.id})
                  </option>
                ))}
              </select>
            </label>
          )
        }
      />
      <QueryView q={tenants} what="tenants" rows={4}>
        {() => (
          <>
            {manageable.length === 0 && (
              <Empty title="You do not administer any tenant." icon="admin">
                A platform owner or a tenant&apos;s admin can give you the admin role.
              </Empty>
            )}
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
        )}
      </QueryView>
    </div>
  );
}

const lines = (text: string) =>
  text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);

// ── tenants (platform owner) ──────────────────────────────────────────────────

function Tenants({ tenant }: { tenant: TenantSummary | undefined }) {
  return (
    <section className="card" aria-labelledby="admin-tenants">
      <h2 className="card-label" id="admin-tenants">
        Tenants
      </h2>
      <p className="fm-note">Only a platform owner creates a tenant or sets its source roots.</p>
      <CreateTenant />
      {tenant && <TenantRoots tenant={tenant} />}
      {tenant && <TenantModels tenant={tenant} />}
    </section>
  );
}

function CreateTenant() {
  const qc = useQueryClient();
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const create = useMutation({
    mutationFn: () => apiPost("/tenants", { id, name: name || id }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["tenants"] });
      setId("");
      setName("");
    },
  });
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        create.mutate();
      }}
    >
      <h3 className="fm-sub-title">New tenant</h3>
      <div className="fm-grid">
        <label className="fm-field">
          <span className="fm-label">Id</span>
          <input
            placeholder="tenant id"
            aria-label="tenant id"
            value={id}
            onChange={(e) => setId(e.target.value)}
            required
          />
        </label>
        <label className="fm-field">
          <span className="fm-label">Display name</span>
          <input
            placeholder="display name"
            aria-label="tenant display name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <span className="fm-hint">The id is used when left empty.</span>
        </label>
      </div>
      <div className="fm-actions">
        <button type="submit" disabled={create.isPending}>
          Create tenant
        </button>
      </div>
      {create.isError && <ErrorNote error={create.error} what="Creating the tenant" />}
    </form>
  );
}

function TenantRoots({ tenant }: { tenant: TenantSummary }) {
  const qc = useQueryClient();
  const [roots, setRoots] = useState<string | null>(null);
  const shownRoots = roots ?? (tenant.source_roots ?? []).join("\n");
  const saveRoots = useMutation({
    mutationFn: () => apiPatch(`/tenants/${tenant.id}`, { source_roots: lines(shownRoots) }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["tenants"] });
      setRoots(null);
    },
  });
  return (
    <div className="fm-sub">
      <div className="fm-stack">
        <div className="fm-field">
          <label htmlFor="tenant-roots" className="fm-label">
            Source roots of {tenant.name}
          </label>
          <textarea
            id="tenant-roots"
            className="fm-mono"
            rows={3}
            value={shownRoots}
            onChange={(e) => setRoots(e.target.value)}
          />
          <p className="fm-hint">
            One absolute directory per line. A project of this tenant may only read directories
            under these.
          </p>
        </div>
      </div>
      <div className="fm-actions">
        <button className="primary" onClick={() => saveRoots.mutate()} disabled={saveRoots.isPending || roots === null}>
          Save source roots
        </button>
      </div>
      {saveRoots.isError && <ErrorNote error={saveRoots.error} what="Saving the source roots" />}
    </div>
  );
}

// RFC 0183: which model-key variables this tenant's commands receive.
function TenantModels({ tenant }: { tenant: TenantSummary }) {
  const qc = useQueryClient();
  const [envNames, setEnvNames] = useState<string | null>(null);
  const [metered, setMetered] = useState<boolean | null>(null);
  const shownEnv = envNames ?? (tenant.model_env ?? []).map((v) => v.name).join("\n");
  const shownMetered = metered ?? tenant.allow_metered ?? false;
  const saveModels = useMutation({
    mutationFn: () =>
      apiPatch(`/tenants/${tenant.id}`, {
        model_env: lines(shownEnv),
        allow_metered: shownMetered,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["tenants"] });
      setEnvNames(null);
      setMetered(null);
    },
  });
  const unset = (tenant.model_env ?? []).filter((v) => !v.set).map((v) => v.name);
  return (
    <div className="fm-sub">
      <div className="fm-stack">
        <div className="fm-field">
          <label htmlFor="tenant-env" className="fm-label">
            Model credentials of {tenant.name}
          </label>
          <textarea
            id="tenant-env"
            className="fm-mono"
            rows={2}
            value={shownEnv}
            onChange={(e) => setEnvNames(e.target.value)}
          />
          {unset.length > 0 && (
            <p className="fm-error">Not set in the console&apos;s environment: {unset.join(", ")}</p>
          )}
          <p className="fm-hint">
            Names of environment variables, one per line: the ones this tenant&apos;s projects
            name in <code>ekos.toml</code> (for example <code>ANTHROPIC_API_KEY</code>). Never a
            key itself. Its pipeline and MCP server receive these and no other tenant&apos;s.
          </p>
        </div>
        <label className="fm-check">
          <input type="checkbox" checked={shownMetered} onChange={(e) => setMetered(e.target.checked)} />
          <span>Allow metered model calls (pass these variables to its commands)</span>
        </label>
      </div>
      <div className="fm-actions">
        <button
          className="primary"
          onClick={() => saveModels.mutate()}
          disabled={saveModels.isPending || (envNames === null && metered === null)}
        >
          Save model credentials
        </button>
      </div>
      {saveModels.isError && <ErrorNote error={saveModels.error} what="Saving the model credentials" />}
    </div>
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
    <label className="fm-field">
      <span className="fm-label">On</span>
      <select aria-label="scope" value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">whole tenant</option>
        {mine.map((w) => (
          <option key={w.id} value={w.id}>
            {w.name}
          </option>
        ))}
      </select>
    </label>
  );
}

function RoleOptions() {
  return (
    <>
      {ROLES.map((r) => (
        <option key={r} value={r}>
          {r}
        </option>
      ))}
    </>
  );
}

function BindingsTable({ bindings, onRemove }: { bindings: Binding[]; onRemove: (id: string) => void }) {
  return (
    <div className="table-scroll">
      <table className="fm-table">
        <thead>
          <tr>
            <th>Who</th>
            <th>Role</th>
            <th>On</th>
            <th>
              <span className="fm-sr">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {bindings.map((b) => (
            <tr key={b.id}>
              <td>
                {b.principal}
                {b.principal_kind !== "user" && <span className="chip fm-kind">{b.principal_kind}</span>}
              </td>
              <td>{b.role}</td>
              <td>
                {b.workspace_id ?? "whole tenant"}
                {describeRule(b) && <span className="fm-rule">{describeRule(b)}</span>}
              </td>
              <td>
                <button
                  className="linkish danger"
                  aria-label={`remove ${b.role} from ${b.principal}`}
                  onClick={() => onRemove(b.id)}
                >
                  remove
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Bindings({ tenant }: { tenant: string }) {
  const qc = useQueryClient();
  const key = ["bindings", tenant];
  const bindings = useQuery({
    queryKey: key,
    queryFn: () => api<Binding[]>(`/tenants/${tenant}/bindings`),
  });
  const remove = useMutation({
    mutationFn: (id: string) => apiDelete(`/tenants/${tenant}/bindings/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  });

  return (
    <section className="card" aria-labelledby="admin-roles">
      <h2 className="card-label" id="admin-roles">
        Roles
      </h2>
      <QueryView
        q={bindings}
        what="role bindings"
        isEmpty={(d) => d.length === 0}
        empty={<p className="fm-hint">Nobody has a role here yet. Grant one below.</p>}
      >
        {(list) => <BindingsTable bindings={list} onRemove={(id) => remove.mutate(id)} />}
      </QueryView>
      <GrantForm tenant={tenant} onGranted={() => qc.invalidateQueries({ queryKey: key })} />
    </section>
  );
}

function GrantForm({ tenant, onGranted }: { tenant: string; onGranted: () => void }) {
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<RoleName>("viewer");
  const [workspace, setWorkspace] = useState("");
  // RFC 0181: limit a viewer to part of one workspace, or hide part of it from them.
  const [limit, setLimit] = useState<"" | "allow" | "deny">("");
  const [source, setSource] = useState("");
  const [path, setPath] = useState("");
  const canLimit = role === "viewer" && workspace !== "";
  const rule = canLimit && limit && (source || path);
  const limited = canLimit && limit !== "";

  const add = useMutation({
    mutationFn: () =>
      apiPost<Binding>(`/tenants/${tenant}/bindings`, {
        email,
        role,
        ...(workspace ? { workspace_id: workspace } : {}),
        ...(rule ? { effect: limit, ...(source ? { source } : {}), ...(path ? { path } : {}) } : {}),
      }),
    onSuccess: () => {
      onGranted();
      setEmail("");
      setSource("");
      setPath("");
    },
  });

  return (
    <form
      className="fm-sub"
      onSubmit={(e) => {
        e.preventDefault();
        add.mutate();
      }}
    >
      <h3 className="fm-sub-title">Grant a role</h3>
      <div className="fm-grid">
        <label className="fm-field">
          <span className="fm-label">E-mail</span>
          <input
            type="email"
            placeholder="person@example.com"
            aria-label="e-mail"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
          />
        </label>
        <label className="fm-field">
          <span className="fm-label">Role</span>
          <select aria-label="role" value={role} onChange={(e) => setRole(e.target.value as RoleName)}>
            <RoleOptions />
          </select>
          <span className="fm-hint">
            {role}: {ROLE_HELP[role]}.
          </span>
        </label>
        {role !== "admin" && <WorkspaceSelect tenant={tenant} value={workspace} onChange={setWorkspace} />}
        {canLimit && (
          <label className="fm-field">
            <span className="fm-label">Limit</span>
            <select aria-label="limit" value={limit} onChange={(e) => setLimit(e.target.value as "" | "allow" | "deny")}>
              <option value="">all of it</option>
              <option value="allow">only…</option>
              <option value="deny">all except…</option>
            </select>
          </label>
        )}
        {limited && <RuleFields source={source} path={path} onSource={setSource} onPath={setPath} />}
      </div>
      {limited && (
        <p className="fm-hint">
          Someone limited this way sees the graph and search only, filtered to what the rule
          leaves. Operators and admins cannot be limited: running the pipeline shows every
          source.
        </p>
      )}
      <div className="fm-actions">
        <button type="submit" disabled={add.isPending || (limited && !rule)}>
          Grant
        </button>
        <span className="fm-status">
          A person who has not signed in yet gets the role at their first sign-in with this address.
        </span>
      </div>
      {add.data?.warning && <p className="fm-error">{add.data.warning}</p>}
      {add.isError && <ErrorNote error={add.error} what="Granting the role" />}
    </form>
  );
}

function RuleFields({
  source,
  path,
  onSource,
  onPath,
}: {
  source: string;
  path: string;
  onSource: (v: string) => void;
  onPath: (v: string) => void;
}) {
  return (
    <>
      <label className="fm-field">
        <span className="fm-label">Source</span>
        <input placeholder="source name" aria-label="source" value={source} onChange={(e) => onSource(e.target.value)} />
      </label>
      <label className="fm-field">
        <span className="fm-label">Path</span>
        <input
          placeholder="path, e.g. secrets/**"
          aria-label="path"
          value={path}
          onChange={(e) => onPath(e.target.value)}
        />
      </label>
    </>
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
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: key });
    void qc.invalidateQueries({ queryKey: ["bindings", tenant] });
  };
  const remove = useMutation({
    mutationFn: (id: string) => apiDelete(`/tenants/${tenant}/groups/${id}`),
    onSuccess: refresh,
  });

  return (
    <section className="card" aria-labelledby="admin-groups">
      <h2 className="card-label" id="admin-groups">
        Groups
      </h2>
      <p className="fm-note">
        A group holds a role for all its members. With a claim value, everyone whose sign-in
        carries that value is a member without being listed.
      </p>
      <QueryView
        q={groups}
        what="groups"
        rows={2}
        isEmpty={(d) => d.length === 0}
        empty={<p className="fm-hint">No groups yet. Create one below.</p>}
      >
        {(list) => (
          <ul className="fm-list">
            {list.map((g) => (
              <li key={g.id}>
                <span className="fm-item">
                  <strong>{g.name}</strong>
                  {g.oidc_value && (
                    <>
                      {" "}
                      <code className="path">{g.oidc_value}</code>
                    </>
                  )}
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
        )}
      </QueryView>
      <CreateGroup tenant={tenant} onCreated={refresh} />
    </section>
  );
}

function CreateGroup({ tenant, onCreated }: { tenant: string; onCreated: () => void }) {
  const [name, setName] = useState("");
  const [claim, setClaim] = useState("");
  const [role, setRole] = useState<RoleName>("viewer");
  const create = useMutation({
    mutationFn: async () => {
      const group = await apiPost<Group>(`/tenants/${tenant}/groups`, {
        name,
        oidc_value: claim,
      });
      await apiPost(`/tenants/${tenant}/bindings`, { group_id: group.id, role });
    },
    onSuccess: () => {
      onCreated();
      setName("");
      setClaim("");
    },
  });
  return (
    <form
      className="fm-sub"
      onSubmit={(e) => {
        e.preventDefault();
        create.mutate();
      }}
    >
      <h3 className="fm-sub-title">New group</h3>
      <div className="fm-grid">
        <label className="fm-field">
          <span className="fm-label">Name</span>
          <input
            placeholder="group name"
            aria-label="group name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
        </label>
        <label className="fm-field">
          <span className="fm-label">Claim value</span>
          <input
            placeholder="claim value (optional)"
            aria-label="claim value"
            value={claim}
            onChange={(e) => setClaim(e.target.value)}
          />
          <span className="fm-hint">Optional: the sign-in group claim that makes someone a member.</span>
        </label>
        <label className="fm-field">
          <span className="fm-label">Role</span>
          <select aria-label="group role" value={role} onChange={(e) => setRole(e.target.value as RoleName)}>
            <RoleOptions />
          </select>
        </label>
      </div>
      <div className="fm-actions">
        <button type="submit" disabled={create.isPending}>
          Create group
        </button>
      </div>
      {create.isError && <ErrorNote error={create.error} what="Creating the group" />}
    </form>
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
    <span className="fm-members">
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
        className="fm-inline"
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
      {add.isError && <span className="fm-error">{String(add.error)}</span>}
    </span>
  );
}

// ── API tokens ────────────────────────────────────────────────────────────────

function TokenTable({ tokens, onRevoke }: { tokens: Token[]; onRevoke: (id: string) => void }) {
  return (
    <div className="table-scroll">
      <table className="fm-table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Starts with</th>
            <th>Role</th>
            <th>Expires</th>
            <th>Last used</th>
            <th>
              <span className="fm-sr">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {tokens.map((t) => (
            <tr key={t.id}>
              <td>{t.name}</td>
              <td>
                <code>{t.prefix}…</code>
              </td>
              <td>{t.roles.map((r) => `${r.role}${r.workspace_id ? ` on ${r.workspace_id}` : ""}`).join(", ")}</td>
              <td>{when(t.expires_at)}</td>
              <td>{when(t.last_used_at)}</td>
              <td>
                {t.revoked ? (
                  <span className="chip bad">revoked</span>
                ) : (
                  <button className="linkish danger" aria-label={`revoke ${t.name}`} onClick={() => onRevoke(t.id)}>
                    revoke
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Tokens({ tenant }: { tenant: string }) {
  const qc = useQueryClient();
  const key = ["tokens", tenant];
  const tokens = useQuery({
    queryKey: key,
    queryFn: () => api<Token[]>(`/tenants/${tenant}/tokens`),
  });
  const [created, setCreated] = useState<Token | null>(null);
  const revoke = useMutation({
    mutationFn: (id: string) => apiDelete(`/tenants/${tenant}/tokens/${id}`),
    onSuccess: () => qc.invalidateQueries({ queryKey: key }),
  });

  return (
    <section className="card" aria-labelledby="admin-tokens">
      <h2 className="card-label" id="admin-tokens">
        API tokens
      </h2>
      <p className="fm-note">
        For agents and scripts: <code>Authorization: Bearer &lt;token&gt;</code>. A token can view
        or operate. It can never review or administer; those need a signed-in person.
      </p>
      <p className="fm-note">
        An MCP client connects to{" "}
        <code>
          {window.location.origin}/mcp/{tenant}/&lt;project&gt;
        </code>{" "}
        with that header. It gets the tools its role allows, limited to what its rules let it
        see, and every call is in the audit log below.
      </p>
      {created?.token && (
        <div className="fm-secret" role="status">
          <p>
            Token <strong>{created.name}</strong> created. Copy it now: it is not stored and will
            not be shown again.
          </p>
          <code>{created.token}</code>
          <button className="primary" onClick={() => setCreated(null)}>
            I have copied it
          </button>
        </div>
      )}
      <QueryView
        q={tokens}
        what="API tokens"
        rows={2}
        isEmpty={(d) => d.length === 0}
        empty={<p className="fm-hint">No tokens yet. Create one below for an agent or a script.</p>}
      >
        {(list) => <TokenTable tokens={list} onRevoke={(id) => revoke.mutate(id)} />}
      </QueryView>
      <CreateToken
        tenant={tenant}
        onCreated={(token) => {
          setCreated(token);
          void qc.invalidateQueries({ queryKey: key });
        }}
      />
    </section>
  );
}

function CreateToken({ tenant, onCreated }: { tenant: string; onCreated: (t: Token) => void }) {
  const [name, setName] = useState("");
  const [role, setRole] = useState<"viewer" | "operator">("viewer");
  const [workspace, setWorkspace] = useState("");
  const [days, setDays] = useState("90");
  const create = useMutation({
    mutationFn: () =>
      apiPost<Token>(`/tenants/${tenant}/tokens`, {
        name,
        role,
        ...(workspace ? { workspace_id: workspace } : {}),
        ...(days ? { expires_days: Number(days) } : {}),
      }),
    onSuccess: (token) => {
      onCreated(token);
      setName("");
    },
  });
  return (
    <form
      className="fm-sub"
      onSubmit={(e) => {
        e.preventDefault();
        create.mutate();
      }}
    >
      <h3 className="fm-sub-title">New token</h3>
      <div className="fm-grid">
        <label className="fm-field">
          <span className="fm-label">Name</span>
          <input
            placeholder="token name"
            aria-label="token name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
        </label>
        <label className="fm-field">
          <span className="fm-label">Role</span>
          <select aria-label="token role" value={role} onChange={(e) => setRole(e.target.value as "viewer" | "operator")}>
            <option value="viewer">viewer</option>
            <option value="operator">operator</option>
          </select>
        </label>
        <WorkspaceSelect tenant={tenant} value={workspace} onChange={setWorkspace} />
        <label className="fm-field">
          <span className="fm-label">Expires after (days)</span>
          <input
            type="number"
            min={1}
            max={3650}
            placeholder="days"
            aria-label="expires after days"
            value={days}
            onChange={(e) => setDays(e.target.value)}
          />
        </label>
      </div>
      <div className="fm-actions">
        <button type="submit" disabled={create.isPending}>
          Create token
        </button>
      </div>
      {create.isError && <ErrorNote error={create.error} what="Creating the token" />}
    </form>
  );
}

// ── audit log ─────────────────────────────────────────────────────────────────

function Audit({ tenant }: { tenant: string }) {
  const audit = useQuery({
    queryKey: ["audit", tenant],
    queryFn: () => api<AuditEntry[]>(`/tenants/${tenant}/audit?limit=100`),
  });
  return (
    <section className="card" aria-labelledby="admin-audit">
      <h2 className="card-label" id="admin-audit">
        Audit log
      </h2>
      <p className="fm-note">
        Every request that changes something, and whether it was allowed. It records that a
        request was permitted, not that the command it started succeeded: that is in the run
        history.
      </p>
      <QueryView
        q={audit}
        what="the audit log"
        isEmpty={(d) => d.length === 0}
        empty={<p className="fm-hint">Nothing recorded yet.</p>}
      >
        {(entries) => (
          <div className="table-scroll fm-scroll-y">
            <table className="fm-table">
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
                {entries.map((e, i) => (
                  <tr key={`${e.at}-${i}`}>
                    <td>{when(e.at)}</td>
                    <td>{e.actor}</td>
                    <td>{e.action}</td>
                    <td>{e.workspace_id}</td>
                    <td>
                      <span className={e.outcome === "denied" ? "chip bad" : "chip ok"}>{e.outcome}</span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </QueryView>
    </section>
  );
}
