// RFC 0223 — step 1 of the workflow: a project's sources. Directories (git repositories,
// document folders) and live databases, a connection test with the database's own census next to
// what was compiled, and the project's Analyse button (the top bar's Init & compile).
//
// A database password is sent once and kept in the console's keychain; the page never shows it.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams } from "react-router-dom";
import { api, apiDelete, apiPost } from "../api/client";
import type { DatabaseSource, DatabaseTest, DirSource } from "../api/types";
import { type Me, mayOperate } from "../WorkspaceShell";

export function Sources() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const list = useQuery({
    queryKey: ["sources", id],
    queryFn: () =>
      api<{ sources: DirSource[]; databases: DatabaseSource[] }>(`/workspaces/${id}/sources`),
    enabled: id !== "",
  });
  const canOperate = mayOperate(me);

  return (
    <div className="sem-page">
      <p className="muted sem-note">
        Step 1: what EKOS reads. Add the repositories, document folders and databases, test each
        database, then <strong>Init &amp; compile</strong> (top right) analyses them. Nothing is
        ever written into a source; a database is read through a read-only user.
      </p>
      {list.isLoading && <p className="muted">loading…</p>}
      {list.isError && <p className="err">{String(list.error)}</p>}
      {list.data && (
        <>
          <section className="card">
            <strong>Directories</strong>
            {list.data.sources.length === 0 ? (
              <p className="muted">
                No named sources: the project reads the directory it was created from. Adding one
                names that directory too.
              </p>
            ) : (
              <table className="review-table">
                <thead>
                  <tr>
                    <th>name</th>
                    <th>kind</th>
                    <th>path</th>
                    <th>state</th>
                  </tr>
                </thead>
                <tbody>
                  {list.data.sources.map((s) => (
                    <tr key={s.name}>
                      <td>{s.name}</td>
                      <td>{s.kind}</td>
                      <td>
                        <code>{s.path}</code>
                      </td>
                      <td>
                        {!s.exists ? (
                          <span className="chip bad">missing</span>
                        ) : s.kind === "git" && !s.git ? (
                          <span className="chip warn">no .git: no history</span>
                        ) : (
                          <span className="chip ok">ok</span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            {canOperate && <AddDirectory workspace={id} />}
          </section>
          <section className="card">
            <strong>Databases</strong>
            {list.data.databases.length === 0 && <p className="muted">No database yet.</p>}
            {list.data.databases.map((d) => (
              <DatabaseRow key={d.name} workspace={id} db={d} canOperate={canOperate} />
            ))}
            {canOperate && <AddDatabase workspace={id} />}
          </section>
        </>
      )}
    </div>
  );
}

function AddDirectory({ workspace }: { workspace: string }) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [path, setPath] = useState("");
  const [kind, setKind] = useState("");
  const add = useMutation({
    mutationFn: () =>
      apiPost<{ message: string }>(`/workspaces/${workspace}/sources`, {
        name,
        path,
        kind: kind || null,
      }),
    onSuccess: () => {
      setName("");
      setPath("");
      void qc.invalidateQueries({ queryKey: ["sources", workspace] });
    },
  });
  return (
    <div className="sem-decide">
      <div className="btnrow">
        <input
          aria-label="source name"
          placeholder="name (e.g. docs)"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <input
          aria-label="source path"
          placeholder="/path/on/the/server"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          style={{ minWidth: "24rem" }}
        />
        <select aria-label="source kind" value={kind} onChange={(e) => setKind(e.target.value)}>
          <option value="">detect</option>
          <option value="git">git</option>
          <option value="dir">dir</option>
        </select>
        <button
          disabled={!name.trim() || !path.trim() || add.isPending}
          onClick={() => add.mutate()}
        >
          Add directory
        </button>
      </div>
      {add.isError && <p className="err">{String(add.error)}</p>}
      {add.isSuccess && <p className="ok-line">{add.data.message}</p>}
    </div>
  );
}

function AddDatabase({ workspace }: { workspace: string }) {
  const qc = useQueryClient();
  const [f, setF] = useState({
    name: "",
    connect: "",
    user: "",
    password: "",
    owners: "",
    profile: "p0",
    dictionary: "",
  });
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) =>
    setF((x) => ({ ...x, [k]: e.target.value }));
  const add = useMutation({
    mutationFn: () =>
      apiPost<{ message: string; password_env: string }>(`/workspaces/${workspace}/databases`, {
        name: f.name,
        connect: f.connect,
        user: f.user,
        password: f.password,
        owners: f.owners
          .split(/[,\s]+/)
          .map((o) => o.trim())
          .filter(Boolean),
        profile: f.profile,
        dictionary: f.dictionary || null,
      }),
    onSuccess: () => {
      // The password leaves the page as soon as it is stored.
      setF((x) => ({ ...x, password: "" }));
      void qc.invalidateQueries({ queryKey: ["sources", workspace] });
    },
  });
  const ready = f.name && f.connect && f.user && f.password && f.owners;
  return (
    <details className="sem-decide">
      <summary>Add an Oracle database</summary>
      <div className="sources-form">
        <label className="sem-field">
          <span>Name</span>
          <input
            aria-label="database name"
            value={f.name}
            onChange={set("name")}
            placeholder="erp"
          />
        </label>
        <label className="sem-field">
          <span>Connect (host:port/service)</span>
          <input
            aria-label="connect"
            value={f.connect}
            onChange={set("connect")}
            placeholder="db.local:1521/ORCLPDB1"
          />
        </label>
        <label className="sem-field">
          <span>Read-only user</span>
          <input aria-label="user" value={f.user} onChange={set("user")} />
        </label>
        <label className="sem-field">
          <span>Password (kept in the console's keychain, never in ekos.toml)</span>
          <input
            aria-label="password"
            type="password"
            autoComplete="new-password"
            value={f.password}
            onChange={set("password")}
          />
        </label>
        <label className="sem-field">
          <span>Schemas to read (comma-separated)</span>
          <input
            aria-label="owners"
            value={f.owners}
            onChange={set("owners")}
            placeholder="ADEMPIERE"
          />
        </label>
        <label className="sem-field">
          <span>Profile</span>
          <select aria-label="profile" value={f.profile} onChange={set("profile")}>
            <option value="p0">p0 — optimizer statistics only</option>
            <option value="p1">p1 — row counts</option>
            <option value="p2">p2 — one scan per table (null counts, bounds)</option>
          </select>
        </label>
        <label className="sem-field">
          <span>Application dictionary</span>
          <select aria-label="dictionary" value={f.dictionary} onChange={set("dictionary")}>
            <option value="">none</option>
            <option value="idempiere">iDempiere / ADempiere</option>
          </select>
        </label>
      </div>
      <div className="btnrow">
        <button className="save" disabled={!ready || add.isPending} onClick={() => add.mutate()}>
          Add database
        </button>
      </div>
      {add.isError && <p className="err">{String(add.error)}</p>}
      {add.isSuccess && <p className="ok-line">{add.data.message}</p>}
    </details>
  );
}

function DatabaseRow({
  workspace,
  db,
  canOperate,
}: {
  workspace: string;
  db: DatabaseSource;
  canOperate: boolean;
}) {
  const qc = useQueryClient();
  const test = useMutation({
    mutationFn: () => apiPost<DatabaseTest>(`/workspaces/${workspace}/databases/${db.name}/test`),
  });
  const remove = useMutation({
    mutationFn: () => apiDelete(`/workspaces/${workspace}/databases/${db.name}`),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["sources", workspace] }),
  });
  const t = test.data;
  return (
    <div className="sem-decide" aria-label={`database ${db.name}`}>
      <p>
        <strong>{db.name}</strong>{" "}
        <code>
          {db.user}@{db.connect}
        </code>{" "}
        · schemas {db.owners.join(", ")} · profile {db.profile}
        {db.dictionary ? ` · dictionary ${db.dictionary}` : ""} ·{" "}
        {db.password_stored ? (
          <span className="chip ok">password stored</span>
        ) : (
          <span className="chip warn">password from ${db.password_env}</span>
        )}
      </p>
      {canOperate && (
        <div className="btnrow">
          <button disabled={test.isPending} onClick={() => test.mutate()}>
            Test connection
          </button>
          <button
            className="danger-btn"
            disabled={remove.isPending}
            onClick={() => remove.mutate()}
          >
            Remove
          </button>
        </div>
      )}
      {test.isPending && <p className="muted">connecting and reading the catalog…</p>}
      {test.isError && <p className="err">{String(test.error)}</p>}
      {remove.isError && <p className="err">{String(remove.error)}</p>}
      {t && (
        <>
          <p>
            Connected as <code>{t.test.label}</code> ({t.test.family} views) in {t.ms} ms ·{" "}
            {t.read_only === true ? (
              <span className="chip ok">writes refused (read-only)</span>
            ) : t.read_only === false ? (
              <span className="chip bad">this user can write</span>
            ) : (
              <span className="chip">write not checked</span>
            )}{" "}
            · {t.invalid_objects} invalid object(s)
          </p>
          {t.test.owners.map((o) => (
            <p key={o.owner} className="muted">
              {o.owner}: {Object.values(o.census).reduce((n, c) => n + c.valid + c.invalid, 0)}{" "}
              objects, {o.columns} columns
            </p>
          ))}
          <table className="review-table" aria-label="census vs compiled">
            <thead>
              <tr>
                <th />
                <th>census</th>
                <th>compiled</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {t.census_vs_compiled.map((r) => (
                <tr key={r.what}>
                  <td>{r.what}</td>
                  <td>{r.census}</td>
                  <td>{r.compiled ?? "—"}</td>
                  <td>
                    {r.equal === true && <span className="chip ok">equal</span>}
                    {r.equal === false && <span className="chip bad">differs</span>}
                    {r.equal === null && <span className="muted">not compiled yet</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {t.test.any_privileges.length > 0 && (
            <p className="err">ANY privileges held: {t.test.any_privileges.join(", ")}</p>
          )}
        </>
      )}
    </div>
  );
}
