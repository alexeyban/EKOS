import { describe, expect, it } from "vitest";
import { impactHopMap, neighborhoodToGraph, type ImpactOut, type NeighborhoodOut } from "./graph-v2";

describe("neighborhoodToGraph", () => {
  it("converts objects and relationships into GNode/GLink shapes", () => {
    const data: NeighborhoodOut = {
      objects: [
        { id: "a", name: "Alpha", kind: "Table" },
        { id: "b", name: "Beta", kind: "Column" },
      ],
      relationships: [{ id: "r1", kind: "DependsOn", from: "a", to: "b" }],
    };

    const { nodes, links } = neighborhoodToGraph(data);

    expect(nodes).toEqual([
      { id: "a", label: "Alpha", kind: "Table", kindIdx: 1, degree: 0, isAggregate: false },
      { id: "b", label: "Beta", kind: "Column", kindIdx: 0, degree: 0, isAggregate: false },
    ]);
    expect(links).toEqual([{ source: "a", target: "b", relKind: "DependsOn", weight: 1 }]);
  });

  it("assigns kindIdx by alphabetically sorted distinct kinds", () => {
    const data: NeighborhoodOut = {
      objects: [
        { id: "1", name: "One", kind: "Zebra" },
        { id: "2", name: "Two", kind: "Apple" },
        { id: "3", name: "Three", kind: "Mango" },
      ],
      relationships: [],
    };

    const { nodes } = neighborhoodToGraph(data);
    const byKind = Object.fromEntries(nodes.map((n) => [n.kind, n.kindIdx]));
    expect(byKind).toEqual({ Apple: 0, Mango: 1, Zebra: 2 });
  });

  it("gives repeated kinds the same stable kindIdx", () => {
    const data: NeighborhoodOut = {
      objects: [
        { id: "1", name: "One", kind: "Table" },
        { id: "2", name: "Two", kind: "Table" },
      ],
      relationships: [],
    };

    const { nodes } = neighborhoodToGraph(data);
    expect(nodes[0].kindIdx).toBe(nodes[1].kindIdx);
  });

  it("handles an empty neighborhood", () => {
    const data: NeighborhoodOut = { objects: [], relationships: [] };
    expect(neighborhoodToGraph(data)).toEqual({ nodes: [], links: [] });
  });
});

describe("impactHopMap", () => {
  it("seeds the map with the root at hop 0", () => {
    const data: ImpactOut = {
      target: { id: "root" },
      direction: "dependents",
      max_hops: 2,
      count: 0,
      hops: [],
    };
    expect(impactHopMap(data)).toEqual(new Map([["root", 0]]));
  });

  it("records each hop's distance", () => {
    const data: ImpactOut = {
      target: { id: "root" },
      direction: "dependents",
      max_hops: 2,
      count: 2,
      hops: [
        { hop: 1, id: "a", name: "A", kind: "Table", via: "root" },
        { hop: 2, id: "b", name: "B", kind: "Table", via: "a" },
      ],
    };
    const m = impactHopMap(data);
    expect(m.get("root")).toBe(0);
    expect(m.get("a")).toBe(1);
    expect(m.get("b")).toBe(2);
  });

  it("keeps the shortest hop distance when a node is reached more than once", () => {
    const data: ImpactOut = {
      target: { id: "root" },
      direction: "dependents",
      max_hops: 3,
      count: 2,
      hops: [
        { hop: 2, id: "a", name: "A", kind: "Table", via: "x" },
        { hop: 1, id: "a", name: "A", kind: "Table", via: "root" },
      ],
    };
    expect(impactHopMap(data).get("a")).toBe(1);
  });
});
