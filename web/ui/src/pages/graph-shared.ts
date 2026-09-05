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
  // RFC 0136 §4 — server-computed ForceAtlas2 position, pinned so the renderer skips its own
  // simulation entirely (same `fx`/`fy` fields `GraphCanvas`'s `onEngineStop` already uses to
  // freeze a client-simulated layout).
  fx?: number;
  fy?: number;
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

// RFC 0136 §4 — the same threshold the console backend's `app/layout.py` documents; kept as one
// source of truth on this side so the frontend's "call the server for a layout" decision and the
// RFC's own stated number never drift apart silently.
export const SERVER_LAYOUT_THRESHOLD = 2000;

// RFC 0136 §3 — impact mode's hop-distance color scale, source outward. Deliberately distinct
// from `PALETTE` (kind coloring) so the two visual languages never collide when both are active
// at once (impact mode overlays on top of whatever kind-colored graph is already on screen).
const IMPACT_SCALE = ["#fbbf24", "#f97316", "#ef4444", "#b91c1c", "#7f1d1d"];
export const impactColorFor = (hop: number) => IMPACT_SCALE[Math.min(hop, IMPACT_SCALE.length - 1)];
