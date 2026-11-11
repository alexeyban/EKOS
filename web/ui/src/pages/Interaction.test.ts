import { describe, expect, it } from "vitest";
import { type Cell, opsText, sortCells } from "./Interaction";

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
