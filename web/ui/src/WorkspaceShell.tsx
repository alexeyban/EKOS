import { useQuery } from "@tanstack/react-query";
import { NavLink, Outlet, useLocation, useOutletContext, useParams } from "react-router-dom";
import { api } from "./api/client";
import { InitAndCompile } from "./InitAndCompile";
import { StepBar } from "./StepBar";

export interface Me {
  mode: "oidc" | "token" | "local";
  email: string | null;
  // The two-value summary: "write" when the caller may change anything anywhere.
  role: "read" | "write";
  // Set by this shell from the caller's roles on the open workspace (RFC 0180).
  canOperate?: boolean;
  canReview?: boolean;
  canAdmin?: boolean;
  // The caller sees only part of the open workspace (RFC 0181).
  restricted?: boolean;
  // RFC 0251 amendment 1: a read-only showcase — nothing that acts is shown (the API refuses it).
  read_only?: boolean;
}

interface Workspace {
  id: string;
  name: string;
  roles?: string[];
  restricted?: boolean;
}

// The page rules live in the one navigation model (RFC 0261 §2.7); re-exported for the pages
// and tests that import them from here.
export { READ_ONLY_HIDDEN, RESTRICTED_TABS } from "./nav";

// What the caller may do in the open workspace. Outside a workspace shell (or against an API
// that sends no roles) the summary decides. These only hide controls; the API enforces.
export const mayOperate = (me: Me) => me.canOperate ?? me.role === "write";
export const mayReview = (me: Me) => me.canReview ?? me.role === "write";
export const mayAdmin = (me: Me) => me.canAdmin ?? me.role === "write";
// False for someone who sees part of the workspace: a page then skips the requests the API
// refuses them (statistics, conflicts) instead of making them and drawing nothing.
export const seesAll = (me: Me | undefined) => me?.restricted !== true;

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
  const { pathname } = useLocation();
  const onScopedPage = pathname.replace(/\/$/, "").endsWith("/graph");
  const readOnly = me.read_only === true;
  const scoped: Me = readOnly
    ? { ...me, canOperate: false, canReview: false, canAdmin: false, restricted }
    : roles
      ? {
          ...me,
          canOperate: roles.includes("operator") || roles.includes("admin"),
          canReview: roles.includes("reviewer") || roles.includes("admin"),
          canAdmin: roles.includes("admin"),
          restricted,
        }
      : me;

  return (
    <>
      {/* Until the list says whether this caller is restricted, a page would ask for what
          a restricted reader is refused. */}
      {ws.isPending ? null : restricted && !onScopedPage ? (
        <section className="card">
          <strong>Restricted access</strong>
          <p className="muted">
            You can see part of this project. The graph and its search show what you have access
            to; the dashboard, run history, configuration, semantics, conflicts and the review
            are for people who see all of it.
          </p>
          <NavLink to={`/w/${id}/graph`}>Open the graph</NavLink>
        </section>
      ) : (
        <>
          {!restricted && (
            <div className="ws-bar">
              <StepBar workspace={id} />
              {mayOperate(scoped) && <InitAndCompile workspace={id} name={name} />}
            </div>
          )}
          <Outlet context={scoped} />
        </>
      )}
    </>
  );
}
