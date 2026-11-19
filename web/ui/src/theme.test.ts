import { describe, expect, it } from "vitest";
import { withAlpha } from "./theme";

describe("withAlpha", () => {
  it("turns a token's hex or rgb value into rgba at the given opacity", () => {
    expect(withAlpha("#5c5578", 0.3)).toBe("rgba(92, 85, 120, 0.3)");
    expect(withAlpha("#fff", 0.5)).toBe("rgba(255, 255, 255, 0.5)");
    expect(withAlpha(" rgba(255, 255, 255, 0.1)", 0.2)).toBe("rgba(255, 255, 255, 0.2)");
    expect(withAlpha("rgb(1 2 3)", 1)).toBe("rgba(1, 2, 3, 1)");
  });

  it("leaves a colour it cannot read as it is", () => {
    expect(withAlpha("violet", 0.4)).toBe("violet");
  });
});
