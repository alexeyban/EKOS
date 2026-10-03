import { describe, expect, it } from "vitest";
import { diffLines, sideBySide } from "./line-diff";

/** Applying an edit script to `a` must give `b`, and its "same" lines must be a common
 * subsequence — the two properties that make it a diff. */
function apply(a: string[], b: string[]) {
  const ops = diffLines(a, b);
  const left = ops.filter((o) => o.kind !== "add").map((o) => o.text);
  const right = ops.filter((o) => o.kind !== "del").map((o) => o.text);
  expect(left).toEqual(a);
  expect(right).toEqual(b);
  return ops;
}

describe("diffLines", () => {
  it("handles the edges", () => {
    expect(apply([], [])).toEqual([]);
    expect(apply(["a"], []).map((o) => o.kind)).toEqual(["del"]);
    expect(apply([], ["a"]).map((o) => o.kind)).toEqual(["add"]);
    expect(apply(["a", "b"], ["a", "b"]).every((o) => o.kind === "same")).toBe(true);
  });

  it("finds a minimal script for a one-line edit in a long text", () => {
    const a = Array.from({ length: 2000 }, (_, i) => `line ${i}`);
    const b = a.slice();
    b[1200] = "edited";
    const ops = apply(a, b);
    expect(ops.filter((o) => o.kind !== "same")).toHaveLength(2);
  });

  it("is correct on random inputs", () => {
    let seed = 7;
    const rnd = () => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return seed / 0x7fffffff;
    };
    for (let t = 0; t < 200; t++) {
      const a = Array.from({ length: Math.floor(rnd() * 12) }, () => "abcd"[Math.floor(rnd() * 4)]);
      const b = Array.from({ length: Math.floor(rnd() * 12) }, () => "abcd"[Math.floor(rnd() * 4)]);
      apply(a, b);
    }
  });
});

describe("sideBySide", () => {
  it("pairs a replaced line, numbers both sides and collapses distant context", () => {
    const before = Array.from({ length: 20 }, (_, i) => `l${i + 1}`).join("\n");
    const after = before.replace("l10", "L10");
    const { rows, changes } = sideBySide(before, after, 2);
    expect(changes).toBe(1);
    const change = rows.find((r) => r.kind === "change")!;
    expect(change.left).toEqual({ n: 10, text: "l10" });
    expect(change.right).toEqual({ n: 10, text: "L10" });
    expect(rows[0]).toEqual({ kind: "gap", hidden: 7 });
    expect(rows[rows.length - 1]).toEqual({ kind: "gap", hidden: 8 });
  });

  it("reports no changes for identical text", () => {
    expect(sideBySide("a\nb", "a\nb").changes).toBe(0);
  });
});
