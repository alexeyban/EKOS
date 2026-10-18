// RFC 0198: what an extension page declares, and the pure helpers that lay its data out.

export interface ExtColumn {
  key: string; // a JSON pointer into one row (or into the panel's object, for fields)
  label: string;
  badge: boolean;
}

export type ExtView =
  | { kind: "table"; rows: string; columns: ExtColumn[] }
  | { kind: "fields"; fields: ExtColumn[] }
  | { kind: "text"; pointer: string }
  // RFC 0207
  | { kind: "code"; left: string; right: string; left_label: string; right_label: string }
  | {
      kind: "graph";
      nodes: string;
      edges: string;
      id: string;
      label: string;
      group: string;
      badge: string;
      from: string;
      to: string;
    }
  | { kind: "markdown"; pointer: string };

export interface ExtPanel {
  id: string;
  title: string;
  command: string[];
  view: ExtView;
  empty: string;
  /** RFC 0207: page selections the panel's command takes as `--name=value`. */
  params?: string[];
}

/** RFC 0207: a selection a page keeps, offered by a source panel's rows. */
export interface ExtParam {
  name: string;
  label: string;
  source: string;
  rows: string;
  key: string;
}

export interface ExtAction {
  label: string;
  command: string[];
  /** RFC 0207: `[argument id, selection name]`. */
  binds?: [string, string][];
  /** RFC 0207: boolean flags always set. */
  flags?: string[];
  /** RFC 0207: a sentence to acknowledge before running. */
  confirm?: string | null;
}

export interface ExtPage {
  extension: string;
  id: string;
  title: string;
  description: string;
  panels: ExtPanel[];
  actions: ExtAction[];
  params?: ExtParam[];
}

/** The selections `panel` needs that `chosen` lacks. */
export function missingParams(panel: ExtPanel, chosen: Record<string, string>): string[] {
  return (panel.params ?? []).filter((p) => !chosen[p]);
}

/** Selections that depend on `name` (their source panel needs it, directly or through others). */
export function dependents(page: ExtPage, name: string): string[] {
  const out: string[] = [];
  const params = page.params ?? [];
  let changed = true;
  while (changed) {
    changed = false;
    for (const p of params) {
      if (out.includes(p.name) || p.name === name) continue;
      const source = page.panels.find((x) => x.id === p.source);
      const needs = source?.params ?? [];
      if (needs.includes(name) || needs.some((n) => out.includes(n))) {
        out.push(p.name);
        changed = true;
      }
    }
  }
  return out;
}

/** The query string of a panel's selections, in declaration order. */
export function selectionQuery(panel: ExtPanel, chosen: Record<string, string>): string {
  const q = new URLSearchParams();
  for (const p of panel.params ?? []) q.set(p, chosen[p] ?? "");
  const s = q.toString();
  return s ? `?${s}` : "";
}

/** The `/cli` args of an action: bound selections and fixed flags; `null` if one is missing. */
export function actionArgs(
  action: ExtAction,
  chosen: Record<string, string>,
): Record<string, string | boolean> | null {
  const args: Record<string, string | boolean> = {};
  for (const [arg, param] of action.binds ?? []) {
    if (!chosen[param]) return null;
    args[arg] = chosen[param];
  }
  for (const f of action.flags ?? []) args[f] = true;
  return args;
}

export interface GraphNode {
  id: string;
  label: string;
  group: string;
  badge: unknown;
  x: number;
  y: number;
}

/** A left-to-right layout: layer = longest path from a source; one lane per group. */
export function layoutGraph(
  nodes: { id: string; label: string; group: string; badge: unknown }[],
  edges: { from: string; to: string }[],
  dx = 190,
  dy = 46,
  laneGap = 24,
): { nodes: GraphNode[]; edges: { from: string; to: string }[]; width: number; height: number } {
  const ids = new Set(nodes.map((n) => n.id));
  const es = edges.filter((e) => ids.has(e.from) && ids.has(e.to) && e.from !== e.to);
  const layer = new Map(nodes.map((n) => [n.id, 0]));
  // Longest path, bounded so a cycle cannot loop forever.
  for (let i = 0; i < nodes.length; i++) {
    let moved = false;
    for (const e of es) {
      const next = (layer.get(e.from) ?? 0) + 1;
      if (next > (layer.get(e.to) ?? 0) && next < nodes.length) {
        layer.set(e.to, next);
        moved = true;
      }
    }
    if (!moved) break;
  }
  const groups = [...new Set(nodes.map((n) => n.group))];
  let y0 = 0;
  const placed: GraphNode[] = [];
  for (const g of groups) {
    const members = nodes.filter((n) => n.group === g);
    const perLayer = new Map<number, number>();
    let rows = 1;
    for (const n of members) {
      const l = layer.get(n.id) ?? 0;
      const row = perLayer.get(l) ?? 0;
      perLayer.set(l, row + 1);
      rows = Math.max(rows, row + 1);
      placed.push({ ...n, x: l * dx, y: y0 + row * dy });
    }
    y0 += rows * dy + laneGap;
  }
  const maxLayer = Math.max(0, ...placed.map((n) => n.x / dx));
  return { nodes: placed, edges: es, width: (maxLayer + 1) * dx, height: Math.max(dy, y0 - laneGap) };
}

/** RFC 6901: `""` is the whole value, `/a/0/b` walks objects and arrays. */
export function at(value: unknown, pointer: string): unknown {
  if (pointer === "") return value;
  if (!pointer.startsWith("/")) return undefined;
  let cur: unknown = value;
  for (const raw of pointer.slice(1).split("/")) {
    const key = raw.replace(/~1/g, "/").replace(/~0/g, "~");
    if (Array.isArray(cur)) {
      const i = Number(key);
      cur = Number.isInteger(i) ? cur[i] : undefined;
    } else if (cur !== null && typeof cur === "object") {
      cur = (cur as Record<string, unknown>)[key];
    } else {
      return undefined;
    }
  }
  return cur;
}

/** A cell's text: strings as they are, arrays joined, objects as JSON, nothing as "–". */
export function show(value: unknown): string {
  if (value === undefined || value === null || value === "") return "–";
  if (Array.isArray(value)) return value.length === 0 ? "–" : value.map(show).join(", ");
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

const GOOD = new Set([
  "healthy",
  "ok",
  "active",
  "passed",
  "pass",
  "success",
  "green",
  "true",
  "exact",
  "merged",
]);
const ATTENTION = new Set([
  "late",
  "unverified",
  "pending",
  "warning",
  "warn",
  "schema_pending",
  "proposed",
  "open",
  "rewritten",
  "llm-drafted",
]);
const BAD = new Set([
  "failing",
  "failed",
  "fail",
  "error",
  "paused",
  "diverged",
  "red",
  "false",
  "gap",
]);

/** The badge tone for a status value (RFC 0198 §1); "" is neutral. */
export function tone(value: unknown): "ok" | "warn" | "bad" | "" {
  const v = show(value).toLowerCase();
  if (GOOD.has(v)) return "ok";
  if (ATTENTION.has(v)) return "warn";
  if (BAD.has(v)) return "bad";
  return "";
}

/** The rows a table view shows: an array at `rows`, or nothing. */
export function rowsOf(data: unknown, view: ExtView): unknown[] {
  if (view.kind !== "table") return [];
  const rows = at(data, view.rows);
  return Array.isArray(rows) ? rows : [];
}
