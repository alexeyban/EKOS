import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link, Outlet } from "react-router-dom";
import { ApiError, api, logout, tokenLogin } from "./api/client";

interface Me {
  mode: "oidc" | "token";
  email: string | null;
  role: "read" | "write";
}

export function Layout() {
  const me = useQuery({
    queryKey: ["me"],
    queryFn: () => api<Me>("/auth/me"),
    retry: false,
  });

  return (
    <>
      <header>
        <Link to="/" className="brand">
          <span>EKOS</span> Console
        </Link>
        <span className="muted phase">RFC 0127 — web console</span>
        {me.data?.role === "write" && (
          <Link to="/schedules" className="linkish" style={{ fontSize: "0.82rem" }}>
            schedules
          </Link>
        )}
        <span style={{ flex: 1 }} />
        {me.data && <Identity me={me.data} />}
      </header>
      <main>{me.data ? <Outlet context={me.data} /> : <SignIn error={me.error} />}</main>
    </>
  );
}

function Identity({ me }: { me: Me }) {
  const qc = useQueryClient();
  return (
    <span className="muted" style={{ fontSize: "0.82rem" }}>
      {me.email ?? "token"} · <span className={`chip ${me.role === "write" ? "ok" : ""}`}>{me.role}</span>{" "}
      <button
        className="linkish"
        onClick={async () => {
          await logout();
          qc.invalidateQueries();
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
    mutationFn: () => tokenLogin(token.trim()),
    onSuccess: () => qc.invalidateQueries(),
  });

  return (
    <section className="card" style={{ maxWidth: 420, margin: "3rem auto" }}>
      <strong>Sign in</strong>
      {mode === "oidc" ? (
        <>
          <p className="muted">Authenticate with your identity provider.</p>
          <button onClick={() => (window.location.href = "/api/auth/login")}>
            Sign in with SSO
          </button>
        </>
      ) : (
        <>
          <p className="muted">
            Enter the console token (<code>CONSOLE_TOKEN</code> for read, or{" "}
            <code>CONSOLE_WRITE_TOKEN</code> for read + write).
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
