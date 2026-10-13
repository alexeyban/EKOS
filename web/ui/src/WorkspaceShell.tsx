import { useQuery } from "@tanstack/react-query";
import { NavLink, Outlet, useOutletContext, useParams } from "react-router-dom";
import { api } from "./api/client";

export interface Me {
  mode: "oidc" | "token";
  email: string | null;
  // The two-value summary: "write" when the caller may change anything anywhere.
  role: "read" | "write";
  // Set by this shell from the caller's roles on the open workspace (RFC 0180).
  canOperate?: boolean;
  canReview?: boolean;
}

interface Workspace {
  id: string;
  name: string;
  roles?: string[];
}

// What the caller may do in the open workspace. Outside a workspace shell (or against an API
// that sends no roles) the summary decides. These only hide controls; the API enforces.
export const mayOperate = (me: Me) => me.canOperate ?? me.role === "write";
export const mayReview = (me: Me) => me.canReview ?? me.role === "write";

const TABS = [
  { to: "", label: "Dashboard", end: true },
  { to: "graph", label: "Graph" },
  { to: "run", label: "Run" },
  { to: "runs", label: "History" },
  { to: "evals", label: "Evals" },
  { to: "semantics", label: "Semantics" },
  { to: "conflicts", label: "Conflicts" },
  { to: "config", label: "ekos.toml" },
];

export function WorkspaceShell() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const ws = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => api<Workspace[]>("/workspaces"),
  });
  const current = ws.data?.find((w) => w.id === id);
  const name = current?.name ?? id;
  const roles = current?.roles;
  const scoped: Me = roles
    ? {
        ...me,
        canOperate: roles.includes("operator") || roles.includes("admin"),
        canReview: roles.includes("reviewer") || roles.includes("admin"),
      }
    : me;

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
      <Outlet context={scoped} />
    </>
  );
}
