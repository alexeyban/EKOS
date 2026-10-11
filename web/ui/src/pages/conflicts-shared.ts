import type { Conflict, ConflictClaim, ConflictStatus } from "../api/types";

export const TYPE_LABEL: Record<string, string> = {
  duplicate_definition: "defined twice",
  merge_loss: "lost in a merge",
  label_mismatch: "two labels",
  doc_vs_data: "docs vs data",
};

export function conflictChip(s: ConflictStatus): string {
  if (s === "open") return "chip warn";
  return s === "resolved" ? "chip ok" : "chip";
}

/** A claim's value as one readable line. */
export function claimValue(value: unknown): string {
  if (value === null || value === undefined) return "—";
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map((v) => claimValue(v)).join(", ");
  return JSON.stringify(value);
}

export function claimWhere(c: ConflictClaim): string {
  return c.line ? `${c.path}:${c.line}` : c.path;
}

/** Open first — those are the ones waiting for a person — then by name. */
export function conflictOrder(items: Conflict[]): Conflict[] {
  const rank = (c: Conflict) => (c.status === "open" ? 0 : 1);
  return [...items].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}

/** The object ids with an open conflict — what the graph rings. */
export function disputedIds(items: Conflict[] | undefined): Set<string> {
  return new Set((items ?? []).filter((c) => c.status === "open").map((c) => c.subject.id));
}
