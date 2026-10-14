import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link, Outlet } from "react-router-dom";
import { ApiError, api, localLogin, logout, tokenLogin } from "./api/client";

interface Me {
  mode: "oidc" | "token" | "local";
  kind?: string;
  email: string | null;
  role: "read" | "write";
  // RFC 0180. Optional so an older API still type-checks.
  owner?: boolean;
  tenants?: { id: string; name: string; roles: string[] }[];
}

// The Admin page is for a platform owner or an admin of at least one tenant.
const administers = (me: Me) =>
  me.owner === true || (me.tenants ?? []).some((t) => t.roles.includes("admin"));

export function Layout() {
  const me = useQuery({
    queryKey: ["me"],
    queryFn: () => api<Me>("/auth/me"),
    retry: false,
  });

  // A 401 is the definitive "not signed in" signal — the session cookie is gone or invalid, which
  // is exactly the state `sign out` produces. Any other error (a 500, a network blip) is treated
  // as transient: React Query keeps the last successful `me.data`, and we keep trusting it rather
  // than bouncing a still-authenticated operator to the sign-in screen. Without the `unauthorized`
  // check, `sign out` looked broken: logout clears the cookie server-side and the `me` refetch
  // 401s, but the stale `me.data` lingered and the console stayed on the authenticated view.
  const unauthorized = me.error instanceof ApiError && me.error.status === 401;
  const identity = unauthorized ? undefined : me.data;

  return (
    <>
      <header>
        <Link to="/" className="brand">
          <span>EKOS</span> Console
        </Link>
        {identity?.role === "write" && (
          <Link to="/schedules" className="hdr-link">
            Schedules
          </Link>
        )}
        {identity && administers(identity) && (
          <Link to="/admin" className="hdr-link">
            Admin
          </Link>
        )}
        {identity?.owner === true && (
          <Link to="/settings" className="hdr-link">
            Settings
          </Link>
        )}
        <span style={{ flex: 1 }} />
        {identity && <Identity me={identity} />}
      </header>
      <main>{identity ? <Outlet context={identity} /> : <SignIn error={me.error} />}</main>
    </>
  );
}

function Identity({ me }: { me: Me }) {
  const qc = useQueryClient();
  return (
    <span className="muted" style={{ fontSize: "0.82rem" }}>
      {me.email ?? (me.kind === "local" ? "this machine" : "token")} · <span className={`chip ${me.role === "write" ? "ok" : ""}`}>{me.role}</span>{" "}
      <button
        className="linkish"
        onClick={async () => {
          await logout();
          // Refetch every active query under the now-cleared session. The `me` query 401s, which
          // `Layout`'s `unauthorized` check turns into the sign-in screen — without this refetch
          // the stale `me.data` would keep the console looking signed in.
          await qc.invalidateQueries();
        }}
      >
        sign out
      </button>
    </span>
  );
}

function SignIn({ error }: { error: unknown }) {
  const qc = useQueryClient();
  const mode =
    error instanceof ApiError && error.body && typeof error.body === "object"
      ? (error.body as { mode?: string }).mode
      : undefined;
  const [token, setToken] = useState("");
  const login = useMutation({
    mutationFn: async (): Promise<unknown> =>
      mode === "local" ? localLogin(token.trim()) : tokenLogin(token.trim()),
    onSuccess: () => qc.invalidateQueries(),
  });

  return (
    <section className="card" style={{ maxWidth: 420, margin: "3rem auto" }}>
      <strong>Sign in</strong>
      {mode === "local" ? (
        <>
          <p className="muted">
            This console is for this machine only. Run <code>ekos console open</code> in a terminal
            here: it opens a signed-in page, or prints a one-time code to paste below. A code works
            once, for two minutes.
          </p>
          <form
            className="token-row"
            onSubmit={(e) => {
              e.preventDefault();
              login.mutate();
            }}
          >
            <input
              value={token}
              placeholder="one-time code"
              onChange={(e) => setToken(e.target.value)}
              aria-label="one-time code"
            />
            <button type="submit" disabled={login.isPending}>
              Sign in
            </button>
          </form>
          {login.isError && <p className="err">{String(login.error)}</p>}
        </>
      ) : mode === "oidc" ? (
        <>
          <p className="muted">Authenticate with your identity provider.</p>
          <button onClick={() => (window.location.href = "/api/auth/login")}>
            Sign in with SSO
          </button>
        </>
      ) : (
        <>
          <p className="muted">
            Enter the token value your operator configured — the value set for{" "}
            <code>CONSOLE_TOKEN</code> (read access) or <code>CONSOLE_WRITE_TOKEN</code> (read +
            write), not those names themselves.
          </p>
          <form
            className="token-row"
            onSubmit={(e) => {
              e.preventDefault();
              login.mutate();
            }}
          >
            <input
              type="password"
              value={token}
              placeholder="console token"
              onChange={(e) => setToken(e.target.value)}
              aria-label="console token"
            />
            <button type="submit" disabled={login.isPending}>
              Sign in
            </button>
          </form>
          {login.isError && <p className="err">{String(login.error)}</p>}
        </>
      )}
    </section>
  );
}
