import { useEffect, useMemo, useRef } from "react";
import ForceGraph2D, { type ForceGraphMethods } from "react-force-graph-2d";
import { colorFor, type GLink, type GNode } from "./graph-shared";

export function GraphCanvas({
  nodes,
  links,
  focusId,
  selectedId,
  onNodeClick,
}: {
  nodes: GNode[];
  links: GLink[];
  focusId: string | null;
  selectedId: string | null;
  onNodeClick: (n: GNode) => void;
}) {
  const fgRef = useRef<ForceGraphMethods<GNode, GLink> | undefined>(undefined);
  const data = useMemo(() => ({ nodes, links }), [nodes, links]);
  const maxDeg = useMemo(() => Math.max(1, ...nodes.map((n) => n.degree)), [nodes]);
  const maxCount = useMemo(() => Math.max(1, ...nodes.map((n) => n.count ?? 0)), [nodes]);

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

  return (
    <ForceGraph2D
      ref={fgRef}
      graphData={data}
      backgroundColor="#0b0a12"
      nodeRelSize={4}
      nodeVal={(n) =>
        n.isAggregate ? 4 + 20 * ((n.count ?? 1) / maxCount) : 1 + 8 * (n.degree / maxDeg)
      }
      nodeColor={(n) => (n.id === selectedId ? "#ffffff" : colorFor(n.kindIdx))}
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
      linkColor={() => "rgba(160,160,190,0.25)"}
      linkWidth={(l) => 0.4 + Math.min(4, Math.log2(1 + l.weight))}
      onNodeClick={(n) => onNodeClick(n)}
      cooldownTicks={120}
      onEngineStop={() => fgRef.current?.zoomToFit(400, 40)}
    />
  );
}
