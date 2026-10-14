import { describe, expect, it } from "vitest";
import { grouped, parseInput, type SettingField, uninstallTarget } from "./Settings";

const field = (name: string, type: string, group = "G"): SettingField => ({
  name,
  group,
  help: "",
  secret: false,
  editable: true,
  source: "default",
  set: false,
  value: null,
  type,
});

describe("settings page", () => {
  it("keeps the server's group order", () => {
    const g = grouped([field("a", "str", "Server"), field("b", "str", "MCP"), field("c", "int", "Server")]);
    expect(g.map(([name, fs]) => [name, fs.map((f) => f.name)])).toEqual([
      ["Server", ["a", "c"]],
      ["MCP", ["b"]],
    ]);
  });

  it("turns input text into the value the server expects", () => {
    expect(parseInput(field("port", "int"), "9000")).toBe(9000);
    expect(parseInput(field("port", "int"), "soon")).toBe("soon");
    expect(parseInput(field("port", "int"), "")).toBeNull();
    expect(parseInput(field("production", "bool"), "true")).toBe(true);
    expect(parseInput(field("production", "bool"), "false")).toBe(false);
    expect(parseInput(field("owners", "str"), "a@b.c")).toBe("a@b.c");
  });

  it("follows the uninstall page only to this machine", () => {
    expect(uninstallTarget("http://127.0.0.1:41234/?t=abc")).toBe("http://127.0.0.1:41234/?t=abc");
    expect(uninstallTarget("http://evil.example/?t=abc")).toBeNull();
    expect(uninstallTarget("http://127.0.0.1.evil.example:80/")).toBeNull();
    expect(uninstallTarget(undefined)).toBeNull();
  });
});
