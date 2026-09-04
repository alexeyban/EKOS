// Shared graph types + palette — deliberately dependency-free so `Graph.tsx` can import it
// statically without pulling in `react-force-graph-2d` (which stays behind a lazy boundary).

export interface GNode {
  id: string;
  label: string;
  kind: string;
  kindIdx: number;
  degree: number;
  count?: number; // aggregate super-nodes only
  isAggregate: boolean;
  firstSeen?: string; // RFC 0134 — `fs`, RFC 3339; when the node first entered the ledger
}

export interface GLink {
  source: string;
  target: string;
  relKind: string;
  weight: number;
  firstSeen?: string; // RFC 0134 — `fs`
}

// RFC 0134 — an `ekos ledger timeline` bucket label ("2026-08-26") → the end of that day as an
// RFC 3339 instant, so a lexical `firstSeen <= asOf` compare (and the `as_of` query param) both
// see everything minted on or before that bucket.
export const bucketEnd = (label: string): string => `${label}T23:59:59.999Z`;

const PALETTE = [
  "#7c3aed",
  "#059669",
  "#2563eb",
  "#d97706",
  "#dc2626",
  "#0891b2",
  "#db2777",
  "#65a30d",
  "#9333ea",
  "#0d9488",
  "#c026d3",
  "#ca8a04",
  "#4f46e5",
  "#16a34a",
];

export const colorFor = (kindIdx: number) => PALETTE[kindIdx % PALETTE.length];
