import { describe, expect, it } from "vitest";
import { seesAll } from "./WorkspaceShell";

describe("seesAll", () => {
  it("is true outside a workspace shell and for an unrestricted caller", () => {
    expect(seesAll(undefined)).toBe(true);
    expect(seesAll({ mode: "oidc", role: "read" } as never)).toBe(true);
    expect(seesAll({ mode: "oidc", role: "read", restricted: false } as never)).toBe(true);
  });
  it("is false for someone who sees part of the workspace", () => {
    expect(seesAll({ mode: "oidc", role: "read", restricted: true } as never)).toBe(false);
  });
});
