// RFC 0233 — pure helpers for the Integrity tab, kept apart so they are tested without a DOM.

import type { XrefDiagnostic, XrefSeverity } from "../api/types";

export const KIND_LABEL: Record<string, string> = {
  UnresolvedReference: "unresolved",
  AmbiguousReference: "ambiguous",
  ReferenceCycle: "cycle",
  OrphanArtifact: "unused",
  UnreachableSymbol: "unreachable",
  DynamicReferenceUnknown: "dynamic",
};

const RANK: Record<XrefSeverity, number> = { error: 0, warning: 1, info: 2 };

export const severityChip = (s: XrefSeverity) =>
  s === "error" ? "chip bad" : s === "warning" ? "chip warn" : "chip";

/** `file:line`, else `file`, else the referring object. */
export function where(d: XrefDiagnostic): string {
  const { file, line, object_kind, object_name } = d.source;
  if (file && line) return `${file}:${line}`;
  if (file) return file;
  return `${object_kind ?? ""} ${object_name}`.trim();
}

/** A rate as a percentage, or a dash when nothing was checked. */
export function pct(r: number | null | undefined): string {
  return r === null || r === undefined ? "—" : `${(r * 100).toFixed(1)}%`;
}

export interface Filters {
  kind: string;
  severity: string;
  minConfidence: number;
  crossOnly: boolean;
  text: string;
  showGaps: boolean;
}

export const DEFAULT_FILTERS: Filters = {
  kind: "",
  severity: "",
  minConfidence: 0,
  crossOnly: false,
  text: "",
  // Coverage gaps say where EKOS did not look, not what is wrong: hidden until asked for.
  showGaps: false,
};

export function applyFilters(all: XrefDiagnostic[], f: Filters): XrefDiagnostic[] {
  const needle = f.text.trim().toLowerCase();
  return all
    .filter((d) => !f.kind || d.kind === f.kind)
    .filter((d) => !f.severity || d.severity === f.severity)
    .filter((d) => d.confidence >= f.minConfidence)
    .filter((d) => !f.crossOnly || d.cross_system === true)
    .filter((d) => f.showGaps || !d.coverage_gap)
    .filter(
      (d) =>
        !needle ||
        d.target_spec.written.toLowerCase().includes(needle) ||
        d.source.object_name.toLowerCase().includes(needle) ||
        (d.source.file ?? "").toLowerCase().includes(needle),
    )
    .sort(
      (a, b) =>
        RANK[a.severity] - RANK[b.severity] ||
        b.confidence - a.confidence ||
        where(a).localeCompare(where(b)),
    );
}
