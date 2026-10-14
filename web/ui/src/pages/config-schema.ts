// RFC 0186: reading the JSON schema `ekos config schema --json` prints (schemars) into form
// fields. Pure functions; `ConfigForm.tsx` renders them.

export type Schema = {
  $ref?: string;
  type?: string | string[];
  enum?: unknown[];
  oneOf?: Schema[];
  anyOf?: Schema[];
  const?: unknown;
  properties?: Record<string, Schema>;
  additionalProperties?: boolean | Schema;
  items?: Schema;
  description?: string;
  default?: unknown;
  $defs?: Record<string, Schema>;
};

/** How a setting is edited. `complex` settings (tables of tables, lists of tables) are left to
 * the text editor. */
export type Kind =
  | { kind: "text" }
  | { kind: "number"; integer: boolean }
  | { kind: "bool" }
  | { kind: "choice"; options: string[] }
  | { kind: "list" }
  | { kind: "section"; schema: Schema }
  | { kind: "complex" };

export function resolve(root: Schema, node: Schema): Schema {
  let n = node;
  for (let i = 0; i < 10 && n.$ref; i++) {
    const name = n.$ref.replace("#/$defs/", "");
    const target = root.$defs?.[name];
    if (!target) break;
    n = { ...target, description: n.description ?? target.description, default: n.default ?? target.default };
  }
  return n;
}

function types(n: Schema): string[] {
  if (Array.isArray(n.type)) return n.type.filter((t) => t !== "null");
  return n.type ? [n.type] : [];
}

/** The non-null branch of `Option<T>` (`anyOf: [T, {type: null}]`), or the node itself. */
function unwrapOptional(root: Schema, n: Schema): Schema {
  const alts = n.anyOf ?? n.oneOf;
  if (alts) {
    const real = alts.filter((a) => !(a.type === "null" || (Array.isArray(a.type) && a.type.every((t) => t === "null"))));
    if (real.length === 1) return resolve(root, real[0]);
  }
  return n;
}

function choices(n: Schema): string[] | null {
  if (n.enum && n.enum.every((v) => typeof v === "string")) return n.enum as string[];
  const alts = n.oneOf ?? n.anyOf;
  if (alts && alts.length > 0 && alts.every((a) => typeof a.const === "string" || (a.enum?.length === 1 && typeof a.enum[0] === "string"))) {
    return alts.map((a) => String(a.const ?? a.enum![0]));
  }
  return null;
}

export function kindOf(root: Schema, node: Schema): Kind {
  const n = unwrapOptional(root, resolve(root, node));
  const opts = choices(n);
  if (opts) return { kind: "choice", options: opts };
  const t = types(n);
  if (t.length === 1) {
    switch (t[0]) {
      case "string":
        return { kind: "text" };
      case "integer":
        return { kind: "number", integer: true };
      case "number":
        return { kind: "number", integer: false };
      case "boolean":
        return { kind: "bool" };
      case "array": {
        const item = n.items ? resolve(root, n.items) : {};
        return types(item).join() === "string" ? { kind: "list" } : { kind: "complex" };
      }
      case "object":
        if (n.properties && Object.keys(n.properties).length > 0 && !isMap(n)) {
          return { kind: "section", schema: n };
        }
        return { kind: "complex" };
    }
  }
  return { kind: "complex" };
}

function isMap(n: Schema): boolean {
  return typeof n.additionalProperties === "object";
}

/** The value a field's text stands for; `null` removes the key (an empty field). */
export function parseField(kind: Kind, text: string): unknown {
  switch (kind.kind) {
    case "bool":
      return text === "" ? null : text === "true";
    case "number": {
      if (text.trim() === "") return null;
      const n = Number(text);
      return Number.isFinite(n) && (!kind.integer || Number.isInteger(n)) ? n : text;
    }
    case "list":
      return text.trim() === ""
        ? null
        : text
            .split("\n")
            .map((s) => s.trim())
            .filter(Boolean);
    default:
      return text === "" ? null : text;
  }
}

/** The text a field shows for a value from the file. */
export function showField(kind: Kind, value: unknown): string {
  if (value === undefined || value === null) return "";
  if (kind.kind === "list" && Array.isArray(value)) return value.join("\n");
  return String(value);
}

/** The value at `path` in the file's values. */
export function valueAt(values: Record<string, unknown>, path: string[]): unknown {
  let v: unknown = values;
  for (const k of path) {
    if (v === null || typeof v !== "object") return undefined;
    v = (v as Record<string, unknown>)[k];
  }
  return v;
}
