// RFC 0261 §2.7: the one navigation model. The sidebar, the breadcrumb and the command palette
// all read it, so a page hidden from someone is hidden everywhere. The rules are the ones the
// header and the tab row applied before (RFC 0180, 0181, 0198, 0251), moved here unchanged.
// They only hide: the API still enforces every one of them.

import type { IconName } from "./Icon";

/** The signed-in caller as `/auth/me` describes them. */
export interface Identity {
  mode: "oidc" | "token" | "local";
  kind?: string;
  email: string | null;
  /** RFC 0251: the platform sign-in name (`github:<login>`) when there is no e-mail. */
  name?: string | null;
  role: "read" | "write";
  owner?: boolean;
  tenants?: { id: string; name: string; roles: string[] }[];
  /** RFC 0251 amendment 1: a read-only showcase — nothing that acts is shown. */
  read_only?: boolean;
}

export interface NavItem {
  /** Stable id (also the page's path segment inside a workspace; "" is the dashboard). */
  id: string;
  label: string;
  /** Absolute path. */
  to: string;
  icon: IconName;
  /** Match the path exactly (the dashboard), not as a prefix. */
  end?: boolean;
  /** The badge's source: a count of things waiting for a person. */
  badge?: "review" | "conflicts" | "questions";
}

export interface NavGroup {
  label: string;
  items: NavItem[];
}

// What someone who sees only part of a workspace can open (RFC 0181): the pages that read
// through their scope. The API refuses the others for them.
export const RESTRICTED_TABS = new Set(["graph"]);
// What a read-only showcase leaves out: the pages that act or call a paid model (RFC 0251).
export const READ_ONLY_HIDDEN = new Set(["run", "commands", "chat", "config"]);

/** The Admin page is for a platform owner or an admin of at least one tenant. */
export const administers = (me: Identity) =>
  me.owner === true || (me.tenants ?? []).some((t) => t.roles.includes("admin"));

type PageDef = Omit<NavItem, "to"> & { group: string };

/** A workspace's pages, in the design's groups. Nothing that was a tab before is dropped. */
const WORKSPACE_PAGES: PageDef[] = [
  { group: "Overview", id: "", label: "Dashboard", icon: "dashboard", end: true },
  { group: "Overview", id: "sources", label: "Sources", icon: "sources" },
  { group: "Explore", id: "graph", label: "Graph", icon: "graph" },
  { group: "Explore", id: "chat", label: "Chat", icon: "chat" },
  { group: "Explore", id: "transformations", label: "Transformations", icon: "transformations" },
  { group: "Explore", id: "interaction", label: "Interaction", icon: "interaction" },
  { group: "Decide", id: "review", label: "Review", icon: "review", badge: "review" },
  { group: "Decide", id: "semantics", label: "Semantics", icon: "semantics" },
  { group: "Decide", id: "conflicts", label: "Conflicts", icon: "conflicts", badge: "conflicts" },
  { group: "Decide", id: "questions", label: "Questions", icon: "questions", badge: "questions" },
  { group: "Decide", id: "inbox", label: "Inbox", icon: "inbox" },
  { group: "Decide", id: "annotations", label: "Notes", icon: "notes" },
  { group: "Check", id: "integrity", label: "Integrity", icon: "integrity" },
  { group: "Check", id: "evals", label: "Evals", icon: "evals" },
  { group: "Operate", id: "run", label: "Run", icon: "run" },
  { group: "Operate", id: "commands", label: "Commands", icon: "commands" },
  { group: "Operate", id: "runs", label: "History", icon: "history" },
  { group: "Configure", id: "config", label: "ekos.toml", icon: "file" },
];

export interface ExtensionPage {
  extension: string;
  id: string;
  title: string;
}

export interface WorkspaceAccess {
  /** The caller sees only part of the workspace (RFC 0181). */
  restricted: boolean;
}

function groupsOf(items: (NavItem & { group: string })[]): NavGroup[] {
  const out: NavGroup[] = [];
  for (const { group, ...item } of items) {
    const last = out[out.length - 1];
    if (last?.label === group) last.items.push(item);
    else out.push({ label: group, items: [item] });
  }
  return out;
}

/** The sidebar inside workspace `id`. */
export function workspaceNav(
  id: string,
  me: Identity,
  access: WorkspaceAccess,
  extensions: ExtensionPage[] = [],
): NavGroup[] {
  const readOnly = me.read_only === true;
  const base = `/w/${id}`;
  const pages = WORKSPACE_PAGES.filter((p) => !access.restricted || RESTRICTED_TABS.has(p.id))
    .filter((p) => !readOnly || !READ_ONLY_HIDDEN.has(p.id))
    .map((p) => ({ ...p, to: p.id ? `${base}/${p.id}` : base }));
  // Schedules sits with Operate in the design; it is an install-wide page, for writers.
  const schedules =
    me.role === "write" && !access.restricted
      ? [{ group: "Operate", id: "schedules", label: "Schedules", icon: "schedules" as const, to: "/schedules" }]
      : [];
  const operateEnd = pages.findIndex((p) => p.group === "Configure");
  const withSchedules =
    operateEnd < 0 ? [...pages, ...schedules] : [...pages.slice(0, operateEnd), ...schedules, ...pages.slice(operateEnd)];
  // RFC 0198: extension pages; a restricted reader sees none (their commands read the whole
  // workspace), nor does a read-only showcase.
  const ext =
    access.restricted || readOnly
      ? []
      : extensions.map((p) => ({
          group: "Extensions",
          id: `x/${p.extension}/${p.id}`,
          label: p.title,
          icon: "extension" as const,
          to: `${base}/x/${p.extension}/${p.id}`,
        }));
  return groupsOf([...withSchedules, ...ext]);
}

/** The sidebar outside a project: the home, this install, and (in local mode) the app. */
export function homeNav(me: Identity): NavGroup[] {
  const ro = me.read_only === true;
  const items: (NavItem & { group: string })[] = [
    { group: "Home", id: "projects", label: "Projects", icon: "projects", to: "/", end: true },
  ];
  if (me.role === "write") {
    items.push({ group: "Home", id: "schedules", label: "Schedules", icon: "schedules", to: "/schedules" });
  }
  if (me.owner === true && !ro) {
    items.push({ group: "This install", id: "settings", label: "Settings", icon: "settings", to: "/settings" });
  }
  if (!ro) {
    items.push({ group: "This install", id: "channels", label: "Channels", icon: "channels", to: "/channels" });
  }
  if (!ro && administers(me)) {
    items.push({ group: "This install", id: "admin", label: "Admin", icon: "admin", to: "/admin" });
  }
  return groupsOf(items);
}

/** Every item of `groups`, flat. */
export const allItems = (groups: NavGroup[]) => groups.flatMap((g) => g.items.map((i) => ({ ...i, group: g.label })));

/**
 * The group and item `pathname` is on: the longest matching item wins, so `/w/x/runs` is
 * History, not the dashboard.
 */
export function current(groups: NavGroup[], pathname: string) {
  const path = pathname.replace(/\/$/, "") || "/";
  let best: (NavItem & { group: string }) | undefined;
  for (const item of allItems(groups)) {
    const hit = item.end ? path === item.to : path === item.to || path.startsWith(`${item.to}/`);
    if (hit && (!best || item.to.length > best.to.length)) best = item;
  }
  return best;
}
