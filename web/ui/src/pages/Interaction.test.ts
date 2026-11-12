import { describe, expect, it } from "vitest";
import { type Cell, opsText, sortCells, systemGraph } from "./Interaction";

const cell = (over: Partial<Cell>): Cell => ({
  actor: "routine:x",
  kind: "routine",
  name: "x",
  declared: [],
  writes: false,
  ...over,
});

describe("interaction map", () => {
  it("says declared and observed operations side by side", () => {
    const c = cell({
      declared: ["update"],
      observed: { update: { calls: 3, rows: 3, window_end: "t" }, delete: { calls: 1, rows: 1, window_end: "t" } },
    });
    expect(opsText(c)).toBe("update (declared, observed 3), delete (observed 1)");
  });

  it("lists writers first", () => {
    const r = sortCells([
      cell({ actor: "routine:a", name: "a" }),
      cell({ actor: "db_role:b", kind: "db_role", name: "b", writes: true }),
    ]);
    expect(r.map((c) => c.name)).toEqual(["b", "a"]);
  });
});

describe("system-level graph", () => {
  const m = {
    observed_facts: 1,
    observed_note: "",
    tables: [
      {
        table: "customers",
        cells: [
          cell({ actor: "component:erp", kind: "component", name: "erp", declared: ["update"], writes: true }),
          cell({ actor: "db_role:etl", kind: "db_role", name: "etl", writes: true, observed: { update: { calls: 3, rows: 3, window_end: "t" } } }),
          cell({ actor: "routine:fix", kind: "routine", name: "fix", declared: ["update"], writes: true }),
          cell({ actor: "db_role:bi", kind: "db_role", name: "bi", observed: { read: { calls: 7, rows: 7, window_end: "t" } } }),
        ],
      },
      { table: "audit", cells: [cell({ actor: "routine:log", kind: "routine", name: "log", declared: ["insert"], writes: true })] },
    ],
  };

  it("draws writers into a table and readers out of it, leaving routines out", () => {
    const g = systemGraph(m, new Set(["customers"]));
    expect(g.nodes.map((n) => n.id).sort()).toEqual(["component:erp", "db_role:bi", "db_role:etl", "table:customers"]);
    expect(g.edges).toContainEqual({ from: "db_role:etl", to: "table:customers" });
    expect(g.edges).toContainEqual({ from: "table:customers", to: "db_role:bi" });
    expect(g.nodes.find((n) => n.id === "table:customers")?.badge).toBe("warning");
  });

  it("caps the tables and says how many were left out", () => {
    const g = systemGraph(m, new Set(), 0);
    expect(g.nodes).toEqual([]);
    expect(g.omitted).toBe(1);
  });
});
