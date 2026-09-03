import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link, Outlet } from "react-router-dom";
import { getToken, setToken } from "./api/client";

export function Layout() {
  return (
    <>
      <header>
        <Link to="/" className="brand">
          <span>EKOS</span> Console
        </Link>
        <span className="muted phase">RFC 0127 / 0129 — Phase 1</span>
      </header>
      <main>
        <TokenCard />
        <Outlet />
      </main>
    </>
  );
}

function TokenCard() {
  const qc = useQueryClient();
  const [value, setValue] = useState(getToken());
  const [open, setOpen] = useState(!getToken());

  if (!open) {
    return (
      <p className="muted token-collapsed">
        console token set —{" "}
        <button className="linkish" onClick={() => setOpen(true)}>
          change
        </button>
      </p>
    );
  }

  return (
    <section className="card">
      <strong>Console token</strong>
      <p className="muted">
        Sent as <code>Authorization: Bearer …</code> on every call except <code>/api/health</code>.
        Stored in <code>localStorage</code>.
      </p>
      <form
        className="token-row"
        onSubmit={(e) => {
          e.preventDefault();
          setToken(value.trim());
          qc.invalidateQueries();
          if (value.trim()) setOpen(false);
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
    </section>
  );
}
