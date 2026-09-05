import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import ForceGraph2D, { type ForceGraphMethods } from "react-force-graph-2d";
import { colorFor, impactColorFor, type GLink, type GNode } from "./graph-shared";
import { exportGltf, exportPng } from "./graph-export";

// RFC 0136 §5 — imperative export handle, so the toolbar (in `Graph.tsx`) can trigger a PNG/glTF
// download without `GraphCanvas` needing to know anything about buttons or file names.
export interface GraphCanvasHandle {
  exportPng: () => void;
  exportGltf: () => void;
}

export const GraphCanvas = forwardRef<
  GraphCanvasHandle,
  {
    nodes: GNode[];
    links: GLink[];
    focusId: string | null;
    selectedId: string | null;
    dimKind: string | null; // when set, nodes of other kinds are dimmed
    asOf: string | null; // RFC 0134 — hide anything minted after this instant
    impactHops?: Map<string, number> | null; // RFC 0136 §3 — id -> hop distance, when active
    onNodeClick: (n: GNode) => void;
  }
>(function GraphCanvas(
  { nodes, links, focusId, selectedId, dimKind, asOf, impactHops, onNodeClick },
  ref,
) {
  const fgRef = useRef<ForceGraphMethods<GNode, GLink> | undefined>(undefined);
  const wrapRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });

  const data = useMemo(() => ({ nodes, links }), [nodes, links]);
  const maxDeg = useMemo(() => Math.max(1, ...nodes.map((n) => n.degree)), [nodes]);
  const maxCount = useMemo(() => Math.max(1, ...nodes.map((n) => n.count ?? 0)), [nodes]);

  // keep the canvas the size of its container (which may be fullscreen)
  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    setSize({ w: el.clientWidth, h: el.clientHeight });
    return () => ro.disconnect();
  }, []);

  // RFC 0134 — `nodeVisibility` / `linkVisibility` depend on `asOf`; nudge the renderer to
  // re-evaluate them (the layout is frozen, so this only repaints).
  useEffect(() => {
    (fgRef.current as unknown as { refresh?: () => void } | undefined)?.refresh?.();
  }, [asOf]);

  useEffect(() => {
    if (!focusId || !fgRef.current) return;
    const n = nodes.find((x) => x.id === focusId) as
      | (GNode & { x?: number; y?: number })
      | undefined;
    if (n?.x != null && n.y != null) {
      fgRef.current.centerAt(n.x, n.y, 600);
      fgRef.current.zoom(4, 600);
    }
  }, [focusId, nodes]);

  const fg = () => fgRef.current;
  const zoomBy = (f: number) => fg()?.zoom((fg()?.zoom() ?? 1) * f, 200);
  const pan = (dx: number, dy: number) => {
    const g = fg();
    if (!g) return;
    const k = g.zoom() || 1;
    const c = g.centerAt();
    g.centerAt(c.x + dx / k, c.y + dy / k, 200);
  };

  // RFC 0136 §3 — an edge is "on the trace" when both its endpoints were reached by impact mode.
  // Real evidence (the edge survived `/graph`'s own filters), not a reconstructed path — see the
  // RFC's own §3 note on why a precise hop-to-hop path isn't attempted here.
  const isImpactEdge = (l: GLink) => {
    if (!impactHops) return false;
    const s = typeof l.source === "string" ? l.source : (l.source as GNode).id;
    const t = typeof l.target === "string" ? l.target : (l.target as GNode).id;
    return impactHops.has(s) && impactHops.has(t);
  };

  useImperativeHandle(
    ref,
    () => ({
      exportPng: () => {
        const canvas = wrapRef.current?.querySelector("canvas");
        if (canvas) exportPng(canvas);
      },
      exportGltf: () => exportGltf(nodes, links),
    }),
    [nodes, links],
  );

  return (
    <div ref={wrapRef} className="gc-wrap">
      <ForceGraph2D
        ref={fgRef}
        width={size.w}
        height={size.h}
        graphData={data}
        backgroundColor="#0b0a12"
        nodeRelSize={4}
        nodeVal={(n) =>
          n.isAggregate ? 4 + 20 * ((n.count ?? 1) / maxCount) : 1 + 8 * (n.degree / maxDeg)
        }
        nodeColor={(n) => {
          if (n.id === selectedId) return "#ffffff";
          if (impactHops) {
            const hop = impactHops.get(n.id);
            return hop === undefined ? "#4b4b5e33" : impactColorFor(hop);
          }
          const c = colorFor(n.kindIdx);
          return dimKind && n.kind !== dimKind ? c + "33" : c;
        }}
        nodeLabel={(n) =>
          n.isAggregate
            ? `${n.label} — ${n.count} objects`
            : `${n.label}<br/><span style="opacity:.6">${n.kind}</span>`
        }
        nodeCanvasObjectMode={(n) => (n.isAggregate ? "after" : undefined)}
        nodeCanvasObject={(n, ctx, scale) => {
          if (!n.isAggregate) return;
          ctx.font = `${12 / scale}px sans-serif`;
          ctx.fillStyle = "#d6d3e6";
          ctx.textAlign = "center";
          ctx.fillText(n.label, n.x ?? 0, (n.y ?? 0) + 14 / scale);
        }}
        linkColor={(l) => (isImpactEdge(l) ? "#f97316cc" : "rgba(160,160,190,0.22)")}
        linkWidth={(l) => (isImpactEdge(l) ? 2.5 : 0.4 + Math.min(4, Math.log2(1 + l.weight)))}
        nodeVisibility={(n) => !asOf || !n.firstSeen || n.firstSeen <= asOf}
        linkVisibility={(l) => !asOf || !l.firstSeen || l.firstSeen <= asOf}
        onNodeClick={(n) => onNodeClick(n)}
        cooldownTicks={120}
        onEngineStop={() => {
          // RFC 0134 — freeze the layout once it settles, so scrubbing the time slider only
          // toggles visibility and never re-simulates; surviving nodes never move.
          for (const n of nodes as (GNode & { x?: number; y?: number; fx?: number; fy?: number })[]) {
            if (n.x != null && n.fx == null) {
              n.fx = n.x;
              n.fy = n.y;
            }
          }
          fgRef.current?.zoomToFit(400, 40);
        }}
      />

      <div className="gc-nav">
        <button onClick={() => zoomBy(1.4)} title="zoom in">
          +
        </button>
        <button onClick={() => zoomBy(1 / 1.4)} title="zoom out">
          −
        </button>
        <div className="gc-pad">
          <button onClick={() => pan(0, 120)} title="up">
            ↑
          </button>
          <button onClick={() => pan(-120, 0)} title="left">
            ←
          </button>
          <button onClick={() => fg()?.zoomToFit(400, 40)} title="fit">
            ⊙
          </button>
          <button onClick={() => pan(120, 0)} title="right">
            →
          </button>
          <button onClick={() => pan(0, -120)} title="down">
            ↓
          </button>
        </div>
      </div>
    </div>
  );
});
