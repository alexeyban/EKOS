import { describe, expect, it } from "vitest";
import { READ_ONLY_HIDDEN, seesAll } from "./WorkspaceShell";

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

describe("read-only showcase", () => {
  it("leaves out the pages that act or call a paid model", () => {
    expect([...READ_ONLY_HIDDEN].sort()).toEqual(["chat", "commands", "config", "run"]);
  });
});
