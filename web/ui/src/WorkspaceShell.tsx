import { useQuery } from "@tanstack/react-query";
import { NavLink, Outlet, useOutletContext, useParams } from "react-router-dom";
import { api } from "./api/client";

export interface Me {
  mode: "oidc" | "token";
  email: string | null;
  role: "read" | "write";
}

interface Workspace {
  id: string;
  name: string;
}

const TABS = [
  { to: "", label: "Dashboard", end: true },
  { to: "graph", label: "Graph" },
  { to: "run", label: "Run" },
  { to: "runs", label: "History" },
  { to: "evals", label: "Evals" },
  { to: "config", label: "ekos.toml" },
];

export function WorkspaceShell() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const ws = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => api<Workspace[]>("/workspaces"),
  });
  const name = ws.data?.find((w) => w.id === id)?.name ?? id;

  return (
    <>
      <nav className="ws-nav">
        <NavLink to="/" className="ws-nav-home" end>
          ‹ workspaces
        </NavLink>
        <span className="ws-nav-name">{name}</span>
        <span className="ws-nav-tabs">
          {TABS.map((t) => (
            <NavLink
              key={t.to}
              to={t.to ? `/w/${id}/${t.to}` : `/w/${id}`}
              end={t.end}
              className={({ isActive }) => (isActive ? "ws-tab active" : "ws-tab")}
            >
              {t.label}
            </NavLink>
          ))}
        </span>
      </nav>
      <Outlet context={me} />
    </>
  );
}
