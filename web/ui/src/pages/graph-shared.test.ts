import { describe, expect, it } from "vitest";
import { bucketEnd, colorFor, impactColorFor } from "./graph-shared";

describe("bucketEnd", () => {
  it("appends the end-of-day instant to a date bucket label", () => {
    expect(bucketEnd("2026-08-26")).toBe("2026-08-26T23:59:59.999Z");
  });
});

describe("colorFor", () => {
  it("returns a stable color for a given kind index", () => {
    expect(colorFor(0)).toBe(colorFor(0));
  });

  it("returns distinct colors for distinct low indices", () => {
    expect(colorFor(0)).not.toBe(colorFor(1));
  });

  it("wraps around the palette for an out-of-range index", () => {
    // The palette has 14 entries — index 14 must wrap to the same color as index 0.
    expect(colorFor(14)).toBe(colorFor(0));
  });

  it("returns a valid hex color", () => {
    expect(colorFor(3)).toMatch(/^#[0-9a-f]{6}$/);
  });
});

describe("impactColorFor", () => {
  it("returns the first scale color at hop 0", () => {
    expect(impactColorFor(0)).toBe(impactColorFor(0));
  });

  it("returns increasingly distant colors for increasing hops", () => {
    expect(impactColorFor(0)).not.toBe(impactColorFor(1));
    expect(impactColorFor(1)).not.toBe(impactColorFor(2));
  });

  it("clamps to the last scale color beyond the scale's length", () => {
    // The scale has 5 entries (hops 0-4) — anything beyond must clamp to hop 4's color.
    expect(impactColorFor(4)).toBe(impactColorFor(100));
  });
});
