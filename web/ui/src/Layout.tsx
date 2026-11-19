// RFC 0261 §2.7: the console's app shell — a sidebar of grouped pages, a top bar (project,
// where you are, search, theme, who you are) and a status bar. The same shell serves the
// browser and the EKOS.DEV desktop app.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Link, NavLink, Outlet, useLocation, useMatch } from "react-router-dom";
import { ApiError, api, localLogin, logout, tokenLogin } from "./api/client";
import type { Health, Stats, Workspace, WorkflowStep } from "./api/types";
import { Icon } from "./Icon";
import {
  administers,
  allItems,
  current,
  homeNav,
  workspaceNav,
  type ExtensionPage,
  type Identity,
  type NavGroup,
  type NavItem,
} from "./nav";
import { CommandPalette, usePaletteHotkey } from "./Palette";
import { THEME_LABEL, applyTheme, nextTheme, readTheme, type ThemeChoice } from "./theme";

export { administers };
type Me = Identity;

const SIDEBAR_KEY = "ekos-sidebar";

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

  if (!identity) {
    return (
      <div className="signin-page">
        <div className="brand brand-lg">
          <span>EKOS</span>.DEV
        </div>
        {me.isPending ? null : <SignIn error={me.error} />}
      </div>
    );
  }
  return <Shell me={identity} />;
}

function Shell({ me }: { me: Me }) {
  const ws = useWorkspaceHere();
  const groups = useNav(me, ws);
  const { pathname } = useLocation();
  const [navOpen, setNavOpen] = useState(false);
  const [collapsed, setCollapsed] = useState(() => readFlag(SIDEBAR_KEY));
  const palette = usePaletteHotkey();
  // A narrow screen's drawer closes when the page changes.
  useEffect(() => setNavOpen(false), [pathname]);

  return (
    <div className={`app${collapsed ? " sidebar-collapsed" : ""}${navOpen ? " nav-open" : ""}`}>
      <Sidebar
        groups={groups}
        workspace={ws}
        collapsed={collapsed}
        onCollapse={() => {
          setCollapsed(!collapsed);
          writeFlag(SIDEBAR_KEY, !collapsed);
        }}
      />
      <button
        type="button"
        className="nav-scrim"
        aria-label="Close the menu"
        tabIndex={navOpen ? 0 : -1}
        onClick={() => setNavOpen(false)}
      />
      <div className="app-main">
        <TopBar
          me={me}
          workspace={ws}
          groups={groups}
          onMenu={() => setNavOpen(!navOpen)}
          onSearch={() => palette.setOpen(true)}
        />
        <main className="app-content">
          <Outlet context={me} />
        </main>
      </div>
      <StatusBar workspace={ws} />
      {palette.open && (
        <CommandPalette me={me} workspace={ws} groups={groups} onClose={() => palette.setOpen(false)} />
      )}
    </div>
  );
}

/**
 * The project the page belongs to: `/w/<id>/…`, or a run's project on `/runs/<id>` (read through
 * the run page's own query, so this costs no extra request).
 */
function useWorkspaceHere(): string | undefined {
  const inWorkspace = useMatch("/w/:id/*")?.params.id;
  const runId = useMatch("/runs/:runId")?.params.runId;
  const run = useQuery({
    queryKey: ["run", runId],
    queryFn: () => api<{ workspace_id: string }>(`/runs/${runId}`),
    enabled: runId !== undefined,
    staleTime: Infinity,
  });
  return inWorkspace ?? (runId ? run.data?.workspace_id : undefined);
}

/** The navigation for where the person is: a project's pages, or the home's. */
export function useNav(me: Me, ws: string | undefined): NavGroup[] {
  const workspaces = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => api<Workspace[]>("/workspaces"),
    enabled: ws !== undefined,
  });
  const ext = useQuery({
    queryKey: ["extension-pages"],
    queryFn: () => api<{ pages: ExtensionPage[] }>("/extensions"),
    staleTime: Infinity,
    enabled: ws !== undefined,
  });
  if (ws === undefined) return homeNav(me);
  const restricted = workspaces.data?.find((w) => w.id === ws)?.restricted === true;
  return workspaceNav(ws, me, { restricted }, ext.data?.pages ?? []);
}

// ── sidebar ──────────────────────────────────────────────────────────────────

function Sidebar({
  groups,
  workspace,
  collapsed,
  onCollapse,
}: {
  groups: NavGroup[];
  workspace: string | undefined;
  collapsed: boolean;
  onCollapse: () => void;
}) {
  const badges = useBadges(workspace, groups);
  return (
    <aside className="sidebar" aria-label="pages">
      <div className="sidebar-head">
        <Link to="/" className="brand" aria-label="EKOS.DEV, all projects">
          <span>EKOS</span>
          <span className="brand-rest">.DEV</span>
        </Link>
        <button
          type="button"
          className="icon-btn sidebar-collapse"
          aria-label={collapsed ? "Expand the sidebar" : "Collapse the sidebar"}
          aria-pressed={collapsed}
          onClick={onCollapse}
        >
          <Icon name="panel" />
        </button>
      </div>
      <nav className="sidebar-nav">
        {groups.map((g) => (
          <div key={g.label} className="nav-group">
            <div className="nav-label">{g.label}</div>
            <ul>
              {g.items.map((item) => (
                <li key={item.to}>
                  <NavLink
                    to={item.to}
                    end={item.end}
                    title={collapsed ? item.label : undefined}
                    className={({ isActive }) => `nav-item${isActive ? " active" : ""}`}
                  >
                    <Icon name={item.icon} />
                    <span className="nav-text">{item.label}</span>
                    {item.badge && badges[item.badge] ? (
                      <span className="nav-badge" aria-label={`${badges[item.badge]} waiting`}>
                        {badges[item.badge]}
                      </span>
                    ) : null}
                  </NavLink>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </nav>
    </aside>
  );
}

type Badges = Partial<Record<NonNullable<NavItem["badge"]>, number>>;

/**
 * Counts of things waiting for a person, read through the same queries the pages use (so the
 * pages and the badges share one request). Only for the badges the navigation shows.
 */
function useBadges(ws: string | undefined, groups: NavGroup[]): Badges {
  const shown = new Set(allItems(groups).map((i) => i.badge).filter(Boolean));
  const on = (b: NonNullable<NavItem["badge"]>) => ws !== undefined && shown.has(b);
  const opts = { staleTime: 300_000, retry: false } as const;
  const conflicts = useQuery({
    queryKey: ["conflicts", ws, "open"],
    queryFn: () => api<unknown[]>(`/workspaces/${ws}/conflicts?status=open`),
    enabled: on("conflicts"),
    ...opts,
  });
  const questions = useQuery({
    queryKey: ["questions", ws, "status=open"],
    queryFn: () => api<unknown[]>(`/workspaces/${ws}/questions?status=open`),
    enabled: on("questions"),
    ...opts,
  });
  const workflow = useQuery({
    queryKey: ["workflow", ws],
    queryFn: () => api<{ steps?: WorkflowStep[] }>(`/workspaces/${ws}/workflow`),
    enabled: on("review"),
    staleTime: 60_000,
    retry: false,
  });
  const review = workflow.data?.steps?.find((s) => s.id === "review");
  return {
    conflicts: conflicts.data?.length,
    questions: questions.data?.length,
    review: review?.subjects?.filter((s) => s.items > 0 && s.signoff.state !== "valid").length,
  };
}

// ── top bar ──────────────────────────────────────────────────────────────────

function TopBar({
  me,
  workspace,
  groups,
  onMenu,
  onSearch,
}: {
  me: Me;
  workspace: string | undefined;
  groups: NavGroup[];
  onMenu: () => void;
  onSearch: () => void;
}) {
  const { pathname } = useLocation();
  return (
    <header className="topbar">
      <button type="button" className="icon-btn nav-toggle" aria-label="Menu" onClick={onMenu}>
        <Icon name="projects" />
      </button>
      {workspace !== undefined && <ProjectSwitcher current={workspace} />}
      <Breadcrumb groups={groups} pathname={pathname} />
      <span className="topbar-gap" />
      <button type="button" className="search-btn" onClick={onSearch}>
        <Icon name="search" />
        <span className="search-text">Search or run a command</span>
        <kbd>{isMac() ? "⌘ K" : "Ctrl K"}</kbd>
      </button>
      <ThemeButton />
      <IdentityMenu me={me} />
    </header>
  );
}

function Breadcrumb({ groups, pathname }: { groups: NavGroup[]; pathname: string }) {
  const runDetail = /^\/runs\/[^/]+/.test(pathname);
  const here = current(groups, pathname);
  const trail = runDetail
    ? ["Operate", "History", "Run"]
    : here
      ? [here.group === "Home" ? "" : here.group, here.label].filter(Boolean)
      : [];
  return (
    <nav className="crumbbar" aria-label="breadcrumb">
      <ol>
        {trail.map((part, i) => (
          <li key={`${i}-${part}`} aria-current={i === trail.length - 1 ? "page" : undefined}>
            {i > 0 && <Icon name="chevronRight" size={12} className="crumb-sep" />}
            {part}
          </li>
        ))}
      </ol>
    </nav>
  );
}

function ProjectSwitcher({ current: id }: { current: string }) {
  const workspaces = useQuery({ queryKey: ["workspaces"], queryFn: () => api<Workspace[]>("/workspaces") });
  const here = workspaces.data?.find((w) => w.id === id);
  const state = here?.server?.state;
  const menu = useRef<HTMLDetailsElement>(null);
  const { pathname } = useLocation();
  useEffect(() => menu.current?.removeAttribute("open"), [pathname]);
  return (
    <details className="menu project-switcher" ref={menu}>
      <summary className="menu-btn" aria-label={`Project: ${here?.name ?? id}`}>
        <span className={`dot ${state === "failed" ? "fail" : state === "starting" ? "warn" : "ok"}`} />
        <span className="project-name">{here?.name ?? id}</span>
        <Icon name="chevronDown" size={13} />
      </summary>
      <div className="menu-pop" role="menu">
        {workspaces.data?.map((w) => (
          <Link key={w.id} role="menuitem" to={`/w/${w.id}`} className={w.id === id ? "active" : undefined}>
            {w.name}
            {w.restricted && <span className="chip warn">restricted</span>}
          </Link>
        ))}
        <hr />
        <Link role="menuitem" to="/">
          All projects
        </Link>
      </div>
    </details>
  );
}

function ThemeButton() {
  const [choice, setChoice] = useState<ThemeChoice>(readTheme);
  const icon = choice === "light" ? "sun" : choice === "dark" ? "moon" : "system";
  return (
    <button
      type="button"
      className="icon-btn"
      aria-label={THEME_LABEL[choice]}
      title={THEME_LABEL[choice]}
      onClick={() => {
        const next = nextTheme(choice);
        applyTheme(next);
        setChoice(next);
      }}
    >
      <Icon name={icon} />
    </button>
  );
}

function who(me: Me): string {
  return me.email ?? me.name ?? (me.kind === "local" ? "this machine" : "token");
}

function roleChip(me: Me): { text: string; cls: string } {
  if (me.read_only) return { text: "read-only", cls: "chip" };
  if (me.owner) return { text: "owner", cls: "chip ok" };
  return { text: me.role, cls: me.role === "write" ? "chip ok" : "chip" };
}

function IdentityMenu({ me }: { me: Me }) {
  const qc = useQueryClient();
  const chip = roleChip(me);
  return (
    <details className="menu identity">
      <summary className="menu-btn">
        <span className="identity-name">{who(me)}</span> <span className={chip.cls}>{chip.text}</span>
      </summary>
      <div className="menu-pop menu-right" role="menu">
        <div className="menu-note muted">
          Signed in as {who(me)}
          {me.mode === "local" ? " (local mode)" : ""}
        </div>
        <button
          type="button"
          role="menuitem"
          className="linkish"
          onClick={async () => {
            const out = await logout();
            if (out?.redirect) {
              window.location.href = out.redirect;
              return;
            }
            // Refetch every active query under the now-cleared session. The `me` query 401s, which
            // `Layout`'s `unauthorized` check turns into the sign-in screen — without this refetch
            // the stale `me.data` would keep the console looking signed in.
            await qc.invalidateQueries();
          }}
        >
          sign out
        </button>
      </div>
    </details>
  );
}

// ── status bar ───────────────────────────────────────────────────────────────

function StatusBar({ workspace }: { workspace: string | undefined }) {
  const health = useQuery({
    queryKey: ["health"],
    queryFn: () => api<Health>("/health"),
    refetchInterval: 30_000,
    retry: false,
  });
  const runs = useQuery({
    queryKey: ["runs", "active"],
    queryFn: () => api<{ id: string }[]>("/runs?status=running"),
    refetchInterval: 5_000,
    retry: false,
  });
  const workspaces = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => api<Workspace[]>("/workspaces"),
    enabled: workspace !== undefined,
  });
  const here = workspaces.data?.find((w) => w.id === workspace);
  const stats = useQuery({
    queryKey: ["ws", workspace, "stats"],
    queryFn: () => api<Stats>(`/workspaces/${workspace}/stats`),
    enabled: workspace !== undefined && here !== undefined && !here.restricted,
    staleTime: 300_000,
    retry: false,
  });
  const active = runs.data?.length ?? 0;
  const mcp = here?.server?.state;
  return (
    <footer className="statusbar" aria-label="status">
      <span>
        <span className={`dot ${health.isError ? "fail" : "ok"}`} />
        {health.isError ? "Console unreachable" : "Console running"}
      </span>
      {stats.data && <span>Ledger: {stats.data.objects.toLocaleString()} objects</span>}
      <span>
        {active > 0 && <span className="dot warn" />}
        {active === 0 ? "No runs" : active === 1 ? "1 run in progress" : `${active} runs in progress`}
      </span>
      {mcp && (
        <span>
          <span className={`dot ${mcp === "ready" ? "ok" : mcp === "failed" ? "fail" : "warn"}`} />
          MCP {mcp}
        </span>
      )}
      <span className="statusbar-gap" />
      {health.data && <span>v{health.data.version}</span>}
    </footer>
  );
}

// ── sign-in ──────────────────────────────────────────────────────────────────

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
    <section className="card signin-card">
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
            <button type="submit" className="primary" disabled={login.isPending}>
              Sign in
            </button>
          </form>
          {login.isError && <p className="err">{String(login.error)}</p>}
        </>
      ) : mode === "oidc" ? (
        <>
          <p className="muted">Authenticate with your identity provider.</p>
          <button className="primary" onClick={() => (window.location.href = "/api/auth/login")}>
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
            <button type="submit" className="primary" disabled={login.isPending}>
              Sign in
            </button>
          </form>
          {login.isError && <p className="err">{String(login.error)}</p>}
        </>
      )}
    </section>
  );
}

// ── small helpers ────────────────────────────────────────────────────────────

function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
}

function readFlag(key: string): boolean {
  try {
    return window.localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

function writeFlag(key: string, on: boolean): void {
  try {
    if (on) window.localStorage.setItem(key, "1");
    else window.localStorage.removeItem(key);
  } catch {
    // Storage blocked: the state lasts for this page only.
  }
}
