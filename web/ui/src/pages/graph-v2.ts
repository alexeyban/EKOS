// RFC 0136 — Graph v2: neighbourhood isolation + impact mode data shapes and conversions.
//
// Both `ekos_neighborhood` and `ekos_impact` return *raw* KIR shapes (real `KirObject`/
// `KirRelationship` JSON, `kind` a plain string — `Custom(_)` variants serialize untagged, so
// there's never a `{Custom: "..."}` wrapper to unwrap), unlike `/graph`'s own compact
// `{n, k, d, ...}` format with its separate `kind_index` lookup table. This module is the one
// place that difference is bridged.

import type { GLink, GNode } from "./graph-shared";

export interface NeighborhoodOut {
  objects: { id: string; name: string; kind: string; properties?: Record<string, unknown> }[];
  relationships: { id: string; kind: string; from: string; to: string }[];
}

export interface ImpactHop {
  hop: number;
  id: string;
  name: string;
  kind: string;
  via: string;
}

export interface ImpactOut {
  target: { id: string };
  direction: "dependents" | "dependencies";
  max_hops: number;
  count: number;
  hops: ImpactHop[];
}

/// Converts a raw neighbourhood sub-graph into the `GNode`/`GLink` shape `GraphCanvas` renders.
/// Assigns a stable `kindIdx` per distinct kind name *within this result* (sorted, so the same
/// kind set always colors the same way across repeated isolate calls) — there is no shared
/// `kind_index` table here the way `/graph` has one, since this is a real sub-graph, not a
/// pre-indexed export.
export function neighborhoodToGraph(data: NeighborhoodOut): { nodes: GNode[]; links: GLink[] } {
  const kinds = [...new Set(data.objects.map((o) => o.kind))].sort();
  const kindIdx = new Map(kinds.map((k, i) => [k, i]));
  const nodes: GNode[] = data.objects.map((o) => ({
    id: o.id,
    label: o.name,
    kind: o.kind,
    kindIdx: kindIdx.get(o.kind) ?? 0,
    degree: 0,
    isAggregate: false,
  }));
  const links: GLink[] = data.relationships.map((r) => ({
    source: r.from,
    target: r.to,
    relKind: r.kind,
    weight: 1,
  }));
  return { nodes, links };
}

/// `id -> hop distance` from an impact-mode result. The root itself is hop 0 (not present in
/// `hops`, which only lists what was *reached*), added explicitly so the source node gets its own
/// distinguishing color too rather than falling back to "not impacted."
export function impactHopMap(data: ImpactOut): Map<string, number> {
  const m = new Map<string, number>();
  m.set(data.target.id, 0);
  for (const h of data.hops) {
    const existing = m.get(h.id);
    if (existing === undefined || h.hop < existing) m.set(h.id, h.hop);
  }
  return m;
}
