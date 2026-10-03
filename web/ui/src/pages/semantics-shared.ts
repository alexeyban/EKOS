// RFC 0170 — helpers shared by the Semantics review page and the LinkML viewer/editor. Pure, so
// they are unit-tested without rendering.

import type { Annotations, ReviewStatus, SemanticsItem, SemanticsKind } from "../api/types";

export const KIND_LABEL: Record<SemanticsKind, string> = {
  BusinessConcept: "concept",
  EnumMeaning: "code",
  ConstraintCandidate: "constraint",
  SemanticGap: "gap",
  ConceptConflict: "conflict",
  RationaleLink: "rationale",
};

/** The `--kind` filter value the API takes for each kind. */
export const KIND_FILTER: Record<SemanticsKind, string> = {
  BusinessConcept: "concept",
  EnumMeaning: "enum",
  ConstraintCandidate: "constraint",
  SemanticGap: "gap",
  ConceptConflict: "conflict",
  RationaleLink: "rationale",
};

/** Kinds a human can decide on — a rationale link is a git fact, not a hypothesis. */
export const REVIEWABLE: SemanticsKind[] = [
  "BusinessConcept",
  "EnumMeaning",
  "ConstraintCandidate",
  "SemanticGap",
  "ConceptConflict",
];

export function prop(item: SemanticsItem, key: string): string {
  const v = item.properties[key];
  if (v === null || v === undefined) return "";
  if (typeof v === "string") return v;
  if (Array.isArray(v) && v.every((x) => typeof x !== "object" || x === null)) return v.join(", ");
  return JSON.stringify(v);
}

/** A past decision as a sentence: "confirmed by token:Ann on 2026-10-03 — note". */
export function previousReview(item: SemanticsItem): string {
  const p = item.properties["previous_review"] as Record<string, unknown> | null | undefined;
  if (!p || typeof p !== "object") return "";
  const by = p["reviewed_by"] ? ` by ${String(p["reviewed_by"])}` : "";
  const at = p["reviewed_at"] ? ` on ${String(p["reviewed_at"]).slice(0, 10)}` : "";
  const note = p["review_note"] ? ` — ${String(p["review_note"])}` : "";
  return `${String(p["status"] ?? "reviewed")}${by}${at}${note}`;
}

export function status(item: SemanticsItem): ReviewStatus {
  return (prop(item, "status") || "hypothesis") as ReviewStatus;
}

/** Chip colour for a status: confirmed is good, rejected bad, needs_review a warning. */
export function statusChip(s: string): string {
  if (s === "confirmed") return "chip ok";
  if (s === "rejected") return "chip bad";
  if (s === "needs_review") return "chip warn";
  return "chip";
}

/** What a human named it, else what EKOS recovered. */
export function displayName(item: SemanticsItem): string {
  return prop(item, "expert_name") || item.name;
}

/** The one line that says what the item asserts. */
export function summary(item: SemanticsItem): string {
  switch (item.kind) {
    case "BusinessConcept":
      return prop(item, "definition");
    case "EnumMeaning": {
      const label = prop(item, "expert_label") || prop(item, "label");
      return label ? `means “${label}”` : "meaning unknown";
    }
    case "ConstraintCandidate":
      return `${prop(item, "constraint_type")} · ${prop(item, "expression")}`;
    case "SemanticGap":
    case "ConceptConflict":
      return prop(item, "question");
    case "RationaleLink":
      return `${prop(item, "date").slice(0, 10)} · ${prop(item, "summary")}`;
  }
}

/** A LinkML annotation's value — both `tag: value` and `tag: {value: …}` forms. */
export function annotation(a: Annotations | undefined, tag: string): string | undefined {
  const v = a?.[tag];
  if (v === null || v === undefined) return undefined;
  if (typeof v === "string") return v;
  return v.value;
}

/** Items sorted for review: the queue first (needs_review, then hypotheses by confidence), then
 * decided ones. */
export function reviewOrder(items: SemanticsItem[]): SemanticsItem[] {
  const rank: Record<string, number> = { needs_review: 0, hypothesis: 1, confirmed: 2, rejected: 3 };
  const conf = (i: SemanticsItem) => Number(i.properties["confidence"] ?? 0);
  return items.slice().sort((a, b) => {
    const r = (rank[status(a)] ?? 9) - (rank[status(b)] ?? 9);
    if (r !== 0) return r;
    const c = conf(b) - conf(a);
    if (c !== 0) return c;
    return displayName(a).localeCompare(displayName(b));
  });
}

const REVIEWER_KEY = "ekos.semantics.reviewer";

/** Token mode's typed reviewer name, remembered per browser. Storage can be unavailable. */
export function loadReviewer(): string {
  try {
    return localStorage.getItem(REVIEWER_KEY) ?? "";
  } catch {
    return "";
  }
}

export function saveReviewer(name: string): void {
  try {
    localStorage.setItem(REVIEWER_KEY, name);
  } catch {
    /* a private window: the name is simply not remembered */
  }
}
