import { useQuery } from "@tanstack/react-query";
import { NavLink, Outlet, useLocation, useOutletContext, useParams } from "react-router-dom";
import { api } from "./api/client";
import { InitAndCompile } from "./InitAndCompile";

export interface Me {
  mode: "oidc" | "token";
  email: string | null;
  // The two-value summary: "write" when the caller may change anything anywhere.
  role: "read" | "write";
  // Set by this shell from the caller's roles on the open workspace (RFC 0180).
  canOperate?: boolean;
  canReview?: boolean;
  // The caller sees only part of the open workspace (RFC 0181).
  restricted?: boolean;
}

interface Workspace {
  id: string;
  name: string;
  roles?: string[];
  restricted?: boolean;
}

// What someone who sees only part of a workspace can open (RFC 0181): the pages that read
// through their scope. The API refuses the others for them.
const RESTRICTED_TABS = new Set(["graph"]);

// What the caller may do in the open workspace. Outside a workspace shell (or against an API
// that sends no roles) the summary decides. These only hide controls; the API enforces.
export const mayOperate = (me: Me) => me.canOperate ?? me.role === "write";
export const mayReview = (me: Me) => me.canReview ?? me.role === "write";
// False for someone who sees part of the workspace: a page then skips the requests the API
// refuses them (statistics, conflicts) instead of making them and drawing nothing.
export const seesAll = (me: Me | undefined) => me?.restricted !== true;

// RFC 0198: extension pages come after the built-in tabs; a restricted reader sees none (their
// commands read the whole workspace).
export const extensionTabs = (pages?: { extension: string; id: string; title: string }[]) =>
  (pages ?? []).map((p) => ({ to: `x/${p.extension}/${p.id}`, label: p.title, end: false }));

const TABS: { to: string; label: string; end?: boolean }[] = [
  { to: "", label: "Dashboard", end: true },
  { to: "graph", label: "Graph" },
  { to: "chat", label: "Chat" },
  { to: "run", label: "Run" },
  { to: "commands", label: "Commands" },
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
  const restricted = current?.restricted === true;
  // RFC 0198: one tab per page the console's binary declares (none for the public `ekos`).
  const extPages = useQuery({
    queryKey: ["extension-pages"],
    queryFn: () => api<{ pages: { extension: string; id: string; title: string }[] }>("/extensions"),
    staleTime: Infinity,
  });
  const { pathname } = useLocation();
  const onScopedPage = pathname.replace(/\/$/, "").endsWith("/graph");
  const scoped: Me = roles
    ? {
        ...me,
        canOperate: roles.includes("operator") || roles.includes("admin"),
        canReview: roles.includes("reviewer") || roles.includes("admin"),
        restricted,
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
          {[...TABS, ...(restricted ? [] : extensionTabs(extPages.data?.pages))]
            .filter((t) => !restricted || RESTRICTED_TABS.has(t.to))
            .map((t) => (
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
        {!restricted && mayOperate(scoped) && <InitAndCompile workspace={id} name={name} />}
      </nav>
      {/* Until the list says whether this caller is restricted, a page would ask for what
          a restricted reader is refused. */}
      {ws.isPending ? null : restricted && !onScopedPage ? (
        <section className="card">
          <strong>Restricted access</strong>
          <p className="muted">
            You can see part of this project. The graph and its search show what you have access
            to; the dashboard, run history, configuration, semantics and conflicts are for people
            who see all of it.
          </p>
          <NavLink to={`/w/${id}/graph`}>Open the graph</NavLink>
        </section>
      ) : (
        <Outlet context={scoped} />
      )}
    </>
  );
}
