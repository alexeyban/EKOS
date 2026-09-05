// RFC 0136 §5 — client-side PNG/glTF export. Both read straight off the data already driving the
// canvas (whatever the user is currently looking at — zoom, pan, filters, isolate/impact state
// included) rather than re-deriving anything server-side; see the RFC's own "why not server-side
// PNG rendering" note.

import { colorFor, type GLink, type GNode } from "./graph-shared";

function triggerDownload(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

export function exportPng(canvas: HTMLCanvasElement): void {
  canvas.toBlob((blob) => {
    if (blob) triggerDownload(blob, `ekos-graph-${Date.now()}.png`);
  }, "image/png");
}

/// A minimal, valid glTF 2.0 document: nodes as a `POINTS` primitive (`POSITION` + `COLOR_0`),
/// edges as a `LINES` primitive (`POSITION` only) — a flat point/line snapshot of the current 2D
/// layout (z=0), not a re-derived 3D scene (RFC 0136 §6 Non-goals). Positions come from whatever
/// `x`/`y` the force simulation (or a server-computed layout) has already settled the nodes at;
/// nodes with no position yet (layout still running) are skipped rather than exported at (0,0),
/// which would misrepresent the graph's real shape.
export function exportGltf(nodes: GNode[], links: GLink[]): void {
  type PositionedNode = GNode & { x: number; y: number };
  const positioned = nodes.filter((n): n is PositionedNode => {
    const p = n as GNode & { x?: number; y?: number };
    return typeof p.x === "number" && typeof p.y === "number";
  });
  if (positioned.length === 0) return;

  const indexOf = new Map(positioned.map((n, i) => [n.id, i]));

  // POINTS: interleaved is unnecessary at this scale — separate POSITION/COLOR_0 buffers.
  const pointPositions = new Float32Array(positioned.length * 3);
  const pointColors = new Float32Array(positioned.length * 4);
  positioned.forEach((n, i) => {
    pointPositions[i * 3] = n.x;
    pointPositions[i * 3 + 1] = -n.y; // glTF is right-handed Y-up; canvas Y grows downward
    pointPositions[i * 3 + 2] = 0;
    const hex = colorFor(n.kindIdx);
    const r = Number.parseInt(hex.slice(1, 3), 16) / 255;
    const g = Number.parseInt(hex.slice(3, 5), 16) / 255;
    const b = Number.parseInt(hex.slice(5, 7), 16) / 255;
    pointColors[i * 4] = r;
    pointColors[i * 4 + 1] = g;
    pointColors[i * 4 + 2] = b;
    pointColors[i * 4 + 3] = 1;
  });

  // LINES: one pair of vertices per edge, endpoints missing from `positioned` dropped.
  const linePairs: number[] = [];
  for (const l of links) {
    const s = typeof l.source === "string" ? l.source : (l.source as GNode).id;
    const t = typeof l.target === "string" ? l.target : (l.target as GNode).id;
    const si = indexOf.get(s);
    const ti = indexOf.get(t);
    if (si !== undefined && ti !== undefined) linePairs.push(si, ti);
  }
  const linePositions = new Float32Array(linePairs.length * 3);
  linePairs.forEach((nodeIdx, i) => {
    linePositions[i * 3] = pointPositions[nodeIdx * 3];
    linePositions[i * 3 + 1] = pointPositions[nodeIdx * 3 + 1];
    linePositions[i * 3 + 2] = pointPositions[nodeIdx * 3 + 2];
  });

  const align4 = (n: number) => (n % 4 === 0 ? n : n + (4 - (n % 4)));
  const pointPosBytes = pointPositions.buffer.byteLength;
  const pointColorBytes = pointColors.buffer.byteLength;
  const linePosBytes = linePositions.buffer.byteLength;

  const pointPosOffset = 0;
  const pointColorOffset = align4(pointPosOffset + pointPosBytes);
  const linePosOffset = align4(pointColorOffset + pointColorBytes);
  const totalBytes = linePosOffset + linePosBytes;

  const buffer = new Uint8Array(totalBytes);
  buffer.set(new Uint8Array(pointPositions.buffer), pointPosOffset);
  buffer.set(new Uint8Array(pointColors.buffer), pointColorOffset);
  buffer.set(new Uint8Array(linePositions.buffer), linePosOffset);

  const minMax = (arr: Float32Array, comps: number) => {
    const min = Array(comps).fill(Infinity);
    const max = Array(comps).fill(-Infinity);
    for (let i = 0; i < arr.length; i += comps) {
      for (let c = 0; c < comps; c++) {
        min[c] = Math.min(min[c], arr[i + c]);
        max[c] = Math.max(max[c], arr[i + c]);
      }
    }
    return { min, max };
  };
  const posMinMax = minMax(pointPositions, 3);

  let binary = "";
  for (const byte of buffer) binary += String.fromCharCode(byte);
  const base64 = btoa(binary);

  // glTF requires every mesh to have at least one primitive — when there are no edges to draw,
  // the "relationships" mesh/node/accessor/bufferView are omitted entirely rather than emitted
  // empty (an empty `primitives: []` is not valid glTF).
  const hasLines = linePairs.length > 0;
  const gltfNodes = hasLines
    ? [{ mesh: 0, name: "objects" }, { mesh: 1, name: "relationships" }]
    : [{ mesh: 0, name: "objects" }];
  const meshes = hasLines
    ? [
        { primitives: [{ attributes: { POSITION: 0, COLOR_0: 1 }, mode: 0 }] }, // 0 = POINTS
        { primitives: [{ attributes: { POSITION: 2 }, mode: 1 }] }, // 1 = LINES
      ]
    : [{ primitives: [{ attributes: { POSITION: 0, COLOR_0: 1 }, mode: 0 }] }];
  const accessors = [
    {
      bufferView: 0,
      componentType: 5126, // FLOAT
      count: positioned.length,
      type: "VEC3",
      min: posMinMax.min,
      max: posMinMax.max,
    },
    { bufferView: 1, componentType: 5126, count: positioned.length, type: "VEC4" },
    ...(hasLines
      ? [{ bufferView: 2, componentType: 5126, count: linePairs.length, type: "VEC3" }]
      : []),
  ];
  const bufferViews = [
    { buffer: 0, byteOffset: pointPosOffset, byteLength: pointPosBytes, target: 34962 },
    { buffer: 0, byteOffset: pointColorOffset, byteLength: pointColorBytes, target: 34962 },
    ...(hasLines
      ? [{ buffer: 0, byteOffset: linePosOffset, byteLength: linePosBytes, target: 34962 }]
      : []),
  ];

  const gltf = {
    asset: { version: "2.0", generator: "EKOS Web Console (RFC 0136)" },
    scenes: [{ nodes: gltfNodes.map((_, i) => i) }],
    scene: 0,
    nodes: gltfNodes,
    meshes,
    accessors,
    bufferViews,
    buffers: [
      {
        // Equal to `linePosOffset` when there are no edges (`linePositions` is then a
        // zero-length buffer), so this is correct either way without a conditional.
        byteLength: totalBytes,
        uri: `data:application/octet-stream;base64,${base64}`,
      },
    ],
  };

  const blob = new Blob([JSON.stringify(gltf)], { type: "model/gltf+json" });
  triggerDownload(blob, `ekos-graph-${Date.now()}.gltf`);
}
