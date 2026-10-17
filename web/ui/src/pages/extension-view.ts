// RFC 0198: what an extension page declares, and the pure helpers that lay its data out.

export interface ExtColumn {
  key: string; // a JSON pointer into one row (or into the panel's object, for fields)
  label: string;
  badge: boolean;
}

export type ExtView =
  | { kind: "table"; rows: string; columns: ExtColumn[] }
  | { kind: "fields"; fields: ExtColumn[] }
  | { kind: "text"; pointer: string };

export interface ExtPanel {
  id: string;
  title: string;
  command: string[];
  view: ExtView;
  empty: string;
}

export interface ExtAction {
  label: string;
  command: string[];
}

export interface ExtPage {
  extension: string;
  id: string;
  title: string;
  description: string;
  panels: ExtPanel[];
  actions: ExtAction[];
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

const GOOD = new Set(["healthy", "ok", "active", "passed", "pass", "success", "green", "true"]);
const ATTENTION = new Set([
  "late",
  "unverified",
  "pending",
  "warning",
  "warn",
  "schema_pending",
  "proposed",
  "open",
]);
const BAD = new Set(["failing", "failed", "fail", "error", "paused", "diverged", "red", "false"]);

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
