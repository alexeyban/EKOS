import { afterEach, describe, expect, it, vi } from "vitest";
import { mockDownload } from "../test/mockDownload";
import { exportGltf, exportPng } from "./graph-export";
import type { GLink, GNode } from "./graph-shared";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function node(id: string, x: number, y: number, kindIdx = 0): GNode & { x: number; y: number } {
  return { id, label: id, kind: "Table", kindIdx, degree: 0, isAggregate: false, x, y };
}

describe("exportPng", () => {
  it("downloads whatever blob canvas.toBlob produces, named ekos-graph-<timestamp>.png", () => {
    const { lastFilename, lastBlob, click } = mockDownload();
    const blob = new Blob(["fake-png-bytes"], { type: "image/png" });
    const canvas = {
      toBlob: (cb: (b: Blob | null) => void) => cb(blob),
    } as unknown as HTMLCanvasElement;

    exportPng(canvas);

    expect(click).toHaveBeenCalledOnce();
    expect(lastFilename()).toMatch(/^ekos-graph-\d+\.png$/);
    expect(lastBlob()).toBe(blob);
  });

  it("does nothing when canvas.toBlob yields no blob", () => {
    const { click } = mockDownload();
    const canvas = { toBlob: (cb: (b: Blob | null) => void) => cb(null) } as unknown as HTMLCanvasElement;

    exportPng(canvas);

    expect(click).not.toHaveBeenCalled();
  });
});

describe("exportGltf", () => {
  it("does nothing when no node has a resolved position", () => {
    const { click } = mockDownload();
    const nodes: GNode[] = [{ id: "a", label: "A", kind: "Table", kindIdx: 0, degree: 0, isAggregate: false }];

    exportGltf(nodes, []);

    expect(click).not.toHaveBeenCalled();
  });

  it("exports positioned nodes and their edges as a valid glTF 2.0 document", async () => {
    const { lastFilename, lastBlob, click } = mockDownload();
    const nodes = [node("a", 1, 2), node("b", 3, 4, 1)];
    const links: GLink[] = [{ source: "a", target: "b", relKind: "DependsOn", weight: 1 }];

    exportGltf(nodes, links);

    expect(click).toHaveBeenCalledOnce();
    expect(lastFilename()).toMatch(/^ekos-graph-\d+\.gltf$/);

    const blob = lastBlob();
    expect(blob).toBeDefined();
    const gltf = JSON.parse(await blob!.text());
    expect(gltf.asset.version).toBe("2.0");
    // Both a "objects" points mesh and a "relationships" lines mesh, since there's one edge.
    expect(gltf.nodes.map((n: { name: string }) => n.name)).toEqual(["objects", "relationships"]);
    expect(gltf.accessors[0].count).toBe(2); // 2 positioned nodes
    expect(gltf.accessors[2].count).toBe(2); // 1 edge = 2 line-vertex indices
  });

  it("omits the relationships mesh entirely when there are no edges to draw", async () => {
    const { lastBlob } = mockDownload();
    exportGltf([node("a", 0, 0)], []);

    const gltf = JSON.parse(await lastBlob()!.text());
    expect(gltf.nodes.map((n: { name: string }) => n.name)).toEqual(["objects"]);
    expect(gltf.accessors).toHaveLength(2);
  });

  it("drops nodes that have not settled into a position yet", async () => {
    const { lastBlob } = mockDownload();
    const nodes: GNode[] = [
      { id: "a", label: "A", kind: "Table", kindIdx: 0, degree: 0, isAggregate: false }, // no x/y
      node("b", 5, 6),
    ];

    exportGltf(nodes, []);

    const gltf = JSON.parse(await lastBlob()!.text());
    expect(gltf.accessors[0].count).toBe(1);
  });

  it("drops edges whose endpoint was filtered out for lacking a position", async () => {
    const { lastBlob } = mockDownload();
    const nodes: GNode[] = [
      node("a", 0, 0),
      { id: "b", label: "B", kind: "Table", kindIdx: 0, degree: 0, isAggregate: false }, // no x/y
    ];
    const links: GLink[] = [{ source: "a", target: "b", relKind: "DependsOn", weight: 1 }];

    exportGltf(nodes, links);

    const gltf = JSON.parse(await lastBlob()!.text());
    // Only "a" is positioned, so the edge to unpositioned "b" is dropped — no lines mesh at all.
    expect(gltf.nodes.map((n: { name: string }) => n.name)).toEqual(["objects"]);
  });
});
