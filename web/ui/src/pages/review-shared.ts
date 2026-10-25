// RFC 0213 — pure helpers for the step bar and the Review page.

import type { ReviewItem, SignOff, StepState } from "../api/types";

export const SUBJECT_LABEL: Record<string, string> = {
  scope: "Scope",
  pii: "PII",
  conflicts: "Conflicts",
  identity: "Identity",
  "write-behaviour": "Write behaviour",
  glossary: "Glossary",
};

// Subjects decided on this page; the others link to the page that decides them.
export const DECIDED_ELSEWHERE: Record<string, { to: string; label: string }> = {
  conflicts: { to: "conflicts", label: "Conflicts" },
  glossary: { to: "semantics", label: "Semantics" },
};

export function stepChip(state: StepState): string {
  switch (state) {
    case "done":
      return "chip ok";
    case "stale":
    case "blocked":
      return "chip bad";
    case "partial":
      return "chip warn";
    default:
      return "chip";
  }
}

export function signChip(s: SignOff): string {
  return s.state === "valid" ? "chip ok" : s.state === "none" ? "chip" : "chip bad";
}

/** One line for a sign-off, naming what changed when it is stale. */
export function signLine(s: SignOff): string {
  switch (s.state) {
    case "none":
      return "not signed off";
    case "valid":
      return `signed off by ${s.signed_by}, ${s.signed_at.slice(0, 16).replace("T", " ")}`;
    case "revoked":
      return `revoked by ${s.signed_by}: ${s.reason}`;
    case "stale": {
      const parts = [
        s.added.length && `${s.added.length} added`,
        s.removed.length && `${s.removed.length} removed`,
        s.changed.length && `${s.changed.length} changed`,
      ].filter(Boolean);
      return `stale — signed by ${s.signed_by}; since then ${parts.join(", ") || "the review changed"}`;
    }
  }
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/** A value for display; an object id is shortened to its first 8 characters. */
export function value(v: unknown): string {
  if (v === null || v === undefined) return "—";
  if (typeof v === "string" && UUID.test(v)) return `${v.slice(0, 8)}…`;
  if (Array.isArray(v)) return v.map(value).join(", ");
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

/** `key=value` pairs, signed facts first, for one compact cell. */
export function factsLine(item: ReviewItem): string {
  return Object.entries(item.facts)
    .filter(([, v]) => v !== null && v !== undefined)
    .map(([k, v]) => `${k}=${value(v)}`)
    .join(" · ");
}

export function infoLine(item: ReviewItem): string {
  return Object.entries(item.info)
    .map(([k, v]) => `${k}=${value(v)}`)
    .join(" · ");
}

export function decisionLine(item: ReviewItem): string {
  if (item.decision) {
    return Object.entries(item.decision)
      .filter(([k]) => !["reviewed_by", "reviewed_at", "reason"].includes(k))
      .map(([, v]) => value(v))
      .join(" ");
  }
  if (item.outdated) return "decision outdated";
  if (item.proposed) return `proposed: ${item.proposed}`;
  return "";
}

/** Case-insensitive filter over label and facts; blocking items first, then by label. */
export function visibleItems(items: ReviewItem[], filter: string): ReviewItem[] {
  const f = filter.trim().toLowerCase();
  return items
    .filter(
      (i) => !f || i.label.toLowerCase().includes(f) || factsLine(i).toLowerCase().includes(f),
    )
    .sort((a, b) => Number(b.blocking) - Number(a.blocking) || a.label.localeCompare(b.label));
}
