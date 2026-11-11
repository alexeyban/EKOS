// RFC 0257 — a transformation as a chain of steps: the document `ekos transformation graph
// --json` prints (`ekos.transformation-graph/1`), and the pure helpers the page draws it with.
// A mart's graph (`lakehouse marts graph`) is meant to print the same shape, with `term`, `rule`
// and `fidelity` filled in.

import type { Annotation } from "../api/types";
import { layoutGraph } from "./extension-view";

export interface ChainSpan {
  node: string;
  /** UTF-16 offsets into `source.text`, so `String.prototype.slice` cuts it exactly. */
  start: number;
  end: number;
  start_line: number;
  end_line: number;
}

export interface ChainEvidence {
  path: string;
  line: number | null;
  fragment: string;
  confidence: number;
}

export interface ChainNode {
  id: string;
  index: number | null;
  type: string;
  label: string;
  summary: string;
  group: string;
  statement: number | null;
  columns_in: string[];
  columns_out: string[];
  columns_used: string[];
  source_text: string;
  span: ChainSpan | null;
  term: string | null;
  rule: string | null;
  fidelity: string | null;
  status: string | null;
  evidence: ChainEvidence[];
  annotations: Annotation[];
  members: string[];
}

export interface ChainGraph {
  format: string;
  transformation: {
    key: string;
    kind: string;
    name: string;
    object: string | null;
    path: string;
    recovered: boolean;
  };
  source: { path: string; start_line: number; text: string } | null;
  nodes: ChainNode[];
  edges: { from: string; to: string; kind: "data" | "order" }[];
  spans: ChainSpan[];
  groups: { id: string; label: string; size: number; collapsed: boolean }[];
  collapsed: boolean;
  counts: { nodes: number; mapped: number; unmapped: number; shown: number };
  rows: { available: boolean; reason: string };
}

export interface TransformationRow {
  key: string;
  name: string;
  kind: string;
  object: string;
  steps: number;
  unmapped: number;
}

export interface TransformationList {
  total: number;
  hidden: number;
  transformations: TransformationRow[];
}

/** The badge of a step: a mart's fidelity, else `unmapped` for a step recovery did not understand. */
export function badgeOf(n: ChainNode): string {
  return n.fidelity ?? n.status ?? "";
}

/** The lane a step is drawn in: collapsed statements share one, so their order reads left to
 * right; an expanded statement, or a graph without statements, has its own. */
export function laneOf(n: ChainNode, groups: ChainGraph["groups"]): string {
  if (n.type === "Statement") return "statements";
  return groups.find((g) => g.id === n.group)?.label ?? n.group;
}

/** The layered left-to-right layout of the chain (RFC 0207's `layoutGraph`, no new dependency). */
export function chainLayout(g: ChainGraph) {
  return layoutGraph(
    g.nodes.map((n) => ({ id: n.id, label: n.label, group: laneOf(n, g.groups), badge: badgeOf(n) })),
    g.edges,
  );
}

export interface Segment {
  start: number;
  end: number;
  /** The node of the shortest span covering the segment, which a click selects. */
  node: string | null;
  /** The selected node's span covers it. */
  selected: boolean;
}

/** The source text cut at every span boundary, so overlapping spans still each select their own
 * step: a segment selects the innermost span covering it. */
export function segments(length: number, spans: ChainSpan[], selected: string | null): Segment[] {
  const valid = spans.filter((s) => s.start >= 0 && s.end <= length && s.start < s.end);
  const cuts = new Set<number>([0, length]);
  for (const s of valid) {
    cuts.add(s.start);
    cuts.add(s.end);
  }
  const points = [...cuts].sort((a, b) => a - b);
  const out: Segment[] = [];
  for (let i = 0; i + 1 < points.length; i++) {
    const [start, end] = [points[i], points[i + 1]];
    const covering = valid.filter((s) => s.start <= start && s.end >= end);
    const inner = covering.sort((a, b) => a.end - a.start - (b.end - b.start))[0];
    out.push({
      start,
      end,
      node: inner?.node ?? null,
      selected: covering.some((s) => s.node === selected),
    });
  }
  return out;
}

/** The 1-based statement numbers to request so that `statement` is drawn in full too. */
export function expandWith(current: string, statement: number | null): string {
  if (statement === null) return current;
  if (current === "all") return "all";
  const set = new Set(current.split(",").filter(Boolean).map(Number));
  set.add(statement + 1);
  return [...set].sort((a, b) => a - b).join(",");
}

/** A step's step number as shown on the page: its index in the graph, or its statement range. */
export function stepTitle(n: ChainNode): string {
  if (n.type === "Statement") return n.label;
  return n.index === null ? n.type : `step ${n.index} · ${n.type}`;
}
