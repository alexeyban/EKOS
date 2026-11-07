import type { Annotation, AnnotationState } from "../api/types";

export const KIND_HELP: Record<string, string> = {
  comment: "For people. Visible at once; never applied as logic.",
  instruction:
    "A rule EKOS can apply, or a note a model reads as data. Shared ones apply to everyone once someone else approves.",
  synonym: "A business word that means this entity. Shared ones apply to everyone once approved.",
};

export function stateChip(s: AnnotationState): string {
  if (s === "active") return "chip ok";
  return s === "stale" ? "chip warn" : "chip";
}

/** What the annotation does for others right now, in words. */
export function reach(a: Annotation): string {
  if (a.state === "withdrawn") return "withdrawn";
  if (a.state === "stale") return "stale: the entity changed since — confirm it again";
  if (a.kind === "comment") return "visible to everyone";
  if (a.scope === "personal") return "applies to its author's requests only";
  return a.approved_by ? `approved by ${a.approved_by}: applies to everyone` : "waiting for approval";
}

/** Waiting for a person first (stale, then unapproved shared), withdrawn last, then newest. */
export function annotationOrder(items: Annotation[]): Annotation[] {
  const rank = (a: Annotation) => {
    if (a.state === "stale") return 0;
    if (a.state === "withdrawn") return 3;
    return a.scope === "shared" && a.kind !== "comment" && !a.approved_by ? 1 : 2;
  };
  return [...items].sort(
    (a, b) => rank(a) - rank(b) || b.updated_at.localeCompare(a.updated_at) || a.id.localeCompare(b.id),
  );
}

/** A target as a short label: `project`, or the first block of an id. */
export function targetLabel(target: string): string {
  return target === "project" ? "project" : target.slice(0, 8);
}

/** The optional rule box: empty is no rule; otherwise one JSON object with a `rule` name. */
export function parseRule(text: string): { rule: Record<string, unknown> | null; error?: string } {
  const t = text.trim();
  if (!t) return { rule: null };
  try {
    const v: unknown = JSON.parse(t);
    if (v === null || typeof v !== "object" || Array.isArray(v) || typeof (v as { rule?: unknown }).rule !== "string") {
      return { rule: null, error: 'a rule is a JSON object like {"rule":"fiscal_year_start","month":4}' };
    }
    return { rule: v as Record<string, unknown> };
  } catch {
    return { rule: null, error: "the rule is not valid JSON" };
  }
}

const TARGET = /^(project|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/;
export const validTarget = (t: string): boolean => TARGET.test(t.trim());
