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
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";
import { type Me, mayOperate } from "../WorkspaceShell";

export function Sources() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const list = useQuery({
    queryKey: ["sources", id],
    queryFn: () =>
      api<{ sources: DirSource[]; databases: DatabaseSource[] }>(
        `/workspaces/${id}/sources`,
      ),
    enabled: id !== "",
  });
  const canOperate = mayOperate(me);

  return (
    <div className="fm-page">
      <PageHead
        title="Sources"
        sub={
          <>
            Step 1: what EKOS reads. Add the repositories, document folders and
            databases, test each database, then <strong>Init &amp; compile</strong>{" "}
            (top right) analyses them. Nothing is ever written into a source; a
            database is read through a read-only user.
          </>
        }
      />
      <QueryView q={list} what="sources" rows={5}>
        {(data) => (
          <>
            <Directories
              workspace={id}
              sources={data.sources}
              canOperate={canOperate}
            />
            <Databases
              workspace={id}
              databases={data.databases}
              canOperate={canOperate}
            />
          </>
        )}
      </QueryView>
    </div>
  );
}

function DirState({ s }: { s: DirSource }) {
  if (!s.exists) return <span className="chip bad">missing</span>;
  if (s.kind === "git" && !s.git)
    return <span className="chip warn">no .git: no history</span>;
  return <span className="chip ok">ok</span>;
}

function Directories({
  workspace,
  sources,
  canOperate,
}: {
  workspace: string;
  sources: DirSource[];
  canOperate: boolean;
}) {
  return (
    <section className="card" aria-labelledby="sources-dirs">
      <h2 className="card-label" id="sources-dirs">
        Directories
      </h2>
      {sources.length === 0 ? (
        <Empty title="No named directories" icon="folder">
          The project reads the directory it was created from.
          {canOperate && " Add a repository or a document folder to name it and read more."}
        </Empty>
      ) : (
        <div className="table-scroll">
          <table className="fm-table">
            <thead>
              <tr>
                <th>name</th>
                <th>kind</th>
                <th>path</th>
                <th>state</th>
              </tr>
            </thead>
            <tbody>
              {sources.map((s) => (
                <tr key={s.name}>
                  <td>{s.name}</td>
                  <td>{s.kind}</td>
                  <td>
                    <code>{s.path}</code>
                  </td>
                  <td>
                    <DirState s={s} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {canOperate && <AddDirectory workspace={workspace} />}
    </section>
  );
}

function Databases({
  workspace,
  databases,
  canOperate,
}: {
  workspace: string;
  databases: DatabaseSource[];
  canOperate: boolean;
}) {
  return (
    <section className="card" aria-labelledby="sources-dbs">
      <h2 className="card-label" id="sources-dbs">
        Databases
      </h2>
      {databases.length === 0 && (
        <Empty title="No database yet" icon="sources">
          {canOperate
            ? "Add a database below: EKOS reads its catalog through a read-only user, and the connection test compares it with what was compiled."
            : "An operator adds the project's databases here."}
        </Empty>
      )}
      {databases.map((d) => (
        <DatabaseRow
          key={d.name}
          workspace={workspace}
          db={d}
          canOperate={canOperate}
        />
      ))}
      {canOperate && <AddDatabase workspace={workspace} />}
    </section>
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
    <div className="fm-sub">
      <h3 className="fm-sub-title">Add a directory</h3>
      <div className="fm-grid">
        <label className="fm-field">
          <span className="fm-label">Name</span>
          <input
            aria-label="source name"
            placeholder="name (e.g. docs)"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <label className="fm-field">
          <span className="fm-label">Path</span>
          <input
            aria-label="source path"
            placeholder="/path/on/the/server"
            value={path}
            onChange={(e) => setPath(e.target.value)}
          />
          <span className="fm-hint">An absolute path on the console's machine.</span>
        </label>
        <label className="fm-field">
          <span className="fm-label">Kind</span>
          <select
            aria-label="source kind"
            value={kind}
            onChange={(e) => setKind(e.target.value)}
          >
            <option value="">detect</option>
            <option value="git">git</option>
            <option value="dir">dir</option>
          </select>
        </label>
      </div>
      <div className="fm-actions">
        <button
          className="primary"
          disabled={!name.trim() || !path.trim() || add.isPending}
          onClick={() => add.mutate()}
        >
          Add directory
        </button>
      </div>
      {add.isError && <ErrorNote error={add.error} what="Adding the directory" />}
      {add.isSuccess && <p className="fm-ok">{add.data.message}</p>}
    </div>
  );
}

// RFC 0235: what each kind's form asks for. Every kind is profiled (RFC 0252); the application
// dictionary (iDempiere's) is read where iDempiere runs: Oracle and, RFC 0256, PostgreSQL.
const hasDictionary = (kind: string) => kind === "oracle" || kind === "postgres";

const KINDS = {
  oracle: {
    label: "Oracle",
    connect: "Connect (host:port/service)",
    connectHint: "db.local:1521/ORCLPDB1",
    owners: "Schemas to read (comma-separated)",
    ownersHint: "ADEMPIERE",
  },
  postgres: {
    label: "PostgreSQL",
    connect: "Connect (host[:port]/database)",
    connectHint: "db.local:5432/shop",
    owners: "Schemas to read (comma-separated)",
    ownersHint: "public",
  },
  clickhouse: {
    label: "ClickHouse",
    connect: "Connect (http[s]://host[:port])",
    connectHint: "https://ch.local:8443",
    owners: "Databases to read (comma-separated)",
    ownersHint: "analytics",
  },
  // RFC 0252 §3.5. A self-signed certificate is accepted only when asked for in `connect`.
  sqlserver: {
    label: "SQL Server",
    connect: "Connect (host[:port]/database[?trust-server-certificate=true])",
    connectHint: "mssql.local:1433/shop",
    owners: "Schemas to read (comma-separated)",
    ownersHint: "dbo",
  },
  // RFC 0252 §3.5. The Informix Client SDK names the server; DRDA (drda://…) does not.
  informix: {
    label: "Informix",
    connect:
      "Connect (host[:port]/database@server or drda://host[:port]/database)",
    connectHint: "ifx.local:9088/stores@ol_ifx",
    owners: "Table owners to read (comma-separated)",
    ownersHint: "informix",
  },
  // RFC 0259 D5. A file on the console's machine, inside the allowed source roots, opened
  // read-only: no user, no password.
  duckdb: {
    label: "DuckDB file",
    connect: "Database file (path on the server)",
    connectHint: "/srv/marts/marts.duckdb",
    owners: "Schemas to read (comma-separated)",
    ownersHint: "main",
  },
} as const;
type Kind = keyof typeof KINDS;
const isFile = (kind: string) => kind === "duckdb";

function AddDatabase({ workspace }: { workspace: string }) {
  const qc = useQueryClient();
  const [f, setF] = useState({
    kind: "oracle" as Kind,
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
      apiPost<{ message: string; password_env: string }>(
        `/workspaces/${workspace}/databases`,
        {
          kind: f.kind,
          name: f.name,
          ...(isFile(f.kind)
            ? { path: f.connect }
            : { connect: f.connect, user: f.user, password: f.password }),
          owners: f.owners
            .split(/[,\s]+/)
            .map((o) => o.trim())
            .filter(Boolean),
          profile: f.profile,
          dictionary: hasDictionary(f.kind) ? f.dictionary || null : null,
        },
      ),
    onSuccess: () => {
      // The password leaves the page as soon as it is stored.
      setF((x) => ({ ...x, password: "" }));
      void qc.invalidateQueries({ queryKey: ["sources", workspace] });
    },
  });
  const ready =
    f.name &&
    f.connect &&
    f.owners &&
    (isFile(f.kind) || (f.user && f.password));
  const k = KINDS[f.kind];
  return (
    <details className="fm-disclosure">
      <summary>Add a database</summary>
      <div className="fm-grid">
        <label className="fm-field">
          <span className="fm-label">Kind</span>
          <select
            aria-label="database kind"
            value={f.kind}
            onChange={(e) =>
              setF((x) => ({ ...x, kind: e.target.value as Kind }))
            }
          >
            {(Object.keys(KINDS) as Kind[]).map((key) => (
              <option key={key} value={key}>
                {KINDS[key].label}
              </option>
            ))}
          </select>
        </label>
        <label className="fm-field">
          <span className="fm-label">Name</span>
          <input
            aria-label="database name"
            value={f.name}
            onChange={set("name")}
            placeholder="erp"
          />
        </label>
        <label className="fm-field fm-field-wide">
          <span className="fm-label">{k.connect}</span>
          <input
            aria-label="connect"
            value={f.connect}
            onChange={set("connect")}
            placeholder={k.connectHint}
          />
        </label>
        {!isFile(f.kind) && (
          <Credentials
            user={f.user}
            password={f.password}
            onUser={set("user")}
            onPassword={set("password")}
          />
        )}
        <label className="fm-field">
          <span className="fm-label">{k.owners}</span>
          <input
            aria-label="owners"
            value={f.owners}
            onChange={set("owners")}
            placeholder={k.ownersHint}
          />
        </label>
        <label className="fm-field">
          <span className="fm-label">Profile</span>
          <select
            aria-label="profile"
            value={f.profile}
            onChange={set("profile")}
          >
            <option value="p0">p0 — optimizer statistics only</option>
            <option value="p1">p1 — row counts</option>
            <option value="p2">
              p2 — one scan per table (null counts, bounds)
            </option>
          </select>
        </label>
        {hasDictionary(f.kind) && (
          <label className="fm-field">
            <span className="fm-label">Application dictionary</span>
            <select
              aria-label="dictionary"
              value={f.dictionary}
              onChange={set("dictionary")}
            >
              <option value="">none</option>
              <option value="idempiere">iDempiere / ADempiere</option>
            </select>
          </label>
        )}
      </div>
      <div className="fm-actions">
        <button
          className="primary"
          disabled={!ready || add.isPending}
          onClick={() => add.mutate()}
        >
          Add database
        </button>
        {!ready && (
          <span className="fm-status">
            {isFile(f.kind)
              ? "Name, file and schemas are needed."
              : "Name, connection, user, password and what to read are needed."}
          </span>
        )}
      </div>
      {add.isError && <ErrorNote error={add.error} what="Adding the database" />}
      {add.isSuccess && <p className="fm-ok">{add.data.message}</p>}
    </details>
  );
}

type OnText = (e: { target: { value: string } }) => void;

/** A server database's read-only user and its password (never kept in ekos.toml). */
function Credentials({
  user,
  password,
  onUser,
  onPassword,
}: {
  user: string;
  password: string;
  onUser: OnText;
  onPassword: OnText;
}) {
  return (
    <>
      <label className="fm-field">
        <span className="fm-label">Read-only user</span>
        <input aria-label="user" value={user} onChange={onUser} />
      </label>
      <label className="fm-field">
        <span className="fm-label">Password</span>
        <input
          aria-label="password"
          type="password"
          autoComplete="new-password"
          value={password}
          onChange={onPassword}
        />
        <span className="fm-hint">
          Kept in the console's keychain, never in ekos.toml.
        </span>
      </label>
    </>
  );
}

function PasswordChip({ db }: { db: DatabaseSource }) {
  if (db.path !== undefined) {
    return <span className="chip ok">read-only file, no password</span>;
  }
  return db.password_stored ? (
    <span className="chip ok">password stored</span>
  ) : (
    <span className="chip warn">password from ${db.password_env}</span>
  );
}

function ReadOnlyChip({ readOnly }: { readOnly: boolean | null | undefined }) {
  if (readOnly === true)
    return <span className="chip ok">writes refused (read-only)</span>;
  if (readOnly === false)
    return <span className="chip bad">this user can write</span>;
  return <span className="chip">write not checked</span>;
}

function EqualCell({ equal }: { equal: boolean | null }) {
  if (equal === true) return <span className="chip ok">equal</span>;
  if (equal === false) return <span className="chip bad">differs</span>;
  return <span className="muted">not compiled yet</span>;
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
    mutationFn: () =>
      apiPost<DatabaseTest>(
        `/workspaces/${workspace}/databases/${db.name}/test`,
      ),
  });
  const remove = useMutation({
    mutationFn: () =>
      apiDelete(`/workspaces/${workspace}/databases/${db.name}`),
    onSuccess: () =>
      void qc.invalidateQueries({ queryKey: ["sources", workspace] }),
  });
  return (
    <section className="fm-db" aria-label={`database ${db.name}`}>
      <p className="fm-db-head">
        <strong>{db.name}</strong> <span className="chip">{db.kind}</span>{" "}
        <code>{db.path ?? `${db.user}@${db.connect}`}</code>{" "}
        <PasswordChip db={db} />
      </p>
      <p className="fm-db-line muted">
        {db.kind === "clickhouse" ? "databases" : "schemas"}{" "}
        {db.owners.join(", ")}
        {` · profile ${db.profile}`}
        {db.dictionary ? ` · dictionary ${db.dictionary}` : ""}
      </p>
      {canOperate && (
        <div className="fm-actions">
          <button disabled={test.isPending} onClick={() => test.mutate()}>
            Test connection
          </button>
          <button
            className="danger-outline"
            disabled={remove.isPending}
            onClick={() => remove.mutate()}
          >
            Remove
          </button>
        </div>
      )}
      {test.isPending && (
        <p className="fm-hint">connecting and reading the catalog…</p>
      )}
      {test.isError && <ErrorNote error={test.error} what="Testing the connection" />}
      {remove.isError && <ErrorNote error={remove.error} what="Removing the database" />}
      {test.data && <TestResult t={test.data} />}
    </section>
  );
}

function TestResult({ t }: { t: DatabaseTest }) {
  return (
    <>
      <p className="fm-db-line">
        Connected as <code>{t.test.label}</code> ({t.test.family} views) in{" "}
        {t.ms} ms · <ReadOnlyChip readOnly={t.read_only} /> ·{" "}
        {t.invalid_objects} invalid object(s)
      </p>
      {t.test.owners.map((o) => (
        <p key={o.owner} className="fm-db-line muted">
          {o.owner}:{" "}
          {Object.values(o.census).reduce(
            (n, c) => n + c.valid + c.invalid,
            0,
          )}{" "}
          objects, {o.columns} columns
        </p>
      ))}
      <div className="table-scroll">
        <table className="fm-table" aria-label="census vs compiled">
          <thead>
            <tr>
              <th>
                <span className="fm-sr">what</span>
              </th>
              <th>census</th>
              <th>compiled</th>
              <th>
                <span className="fm-sr">result</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {t.census_vs_compiled.map((r) => (
              <tr key={r.what}>
                <td>{r.what}</td>
                <td>{r.census}</td>
                <td>{r.compiled ?? "—"}</td>
                <td>
                  <EqualCell equal={r.equal} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {t.test.any_privileges.length > 0 && (
        <p className="err">
          ANY privileges held: {t.test.any_privileges.join(", ")}
        </p>
      )}
    </>
  );
}
