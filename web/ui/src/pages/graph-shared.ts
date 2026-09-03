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
}

export interface GLink {
  source: string;
  target: string;
  relKind: string;
  weight: number;
}

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
