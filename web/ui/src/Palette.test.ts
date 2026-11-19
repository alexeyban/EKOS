import { describe, expect, it } from "vitest";
import { homeNav, workspaceNav, type Identity } from "./nav";
import { paletteRows, rank } from "./Palette";
import type { CommandEntry } from "./pages/Commands";

const me: Identity = { mode: "local", kind: "local", email: null, role: "write", owner: true };

function command(path: string, cls: CommandEntry["class"], allowed = true, metered = false): CommandEntry {
  return { path: path.split(" "), about: "", class: cls, metered, allowed, confirm: false, args: [] };
}

const base = {
  workspace: "erp",
  pages: workspaceNav("erp", me, { restricted: false }),
  projects: [
    { id: "erp", name: "idempiere-erp" },
    { id: "pa", name: "plausible-analytics" },
  ],
  commands: [
    command("compile", "write"),
    command("commit", "write", true, true),
    command("coverage", "read"),
    command("conflicts resolve", "review"),
    command("clean", "admin", false),
  ],
  objects: [{ id: "obj:c_country", name: "C_Country", kind: "Table" }],
};

describe("rank", () => {
  it("prefers a prefix, then a word start, then anywhere", () => {
    expect(rank("Conflicts", "co")).toBe(0);
    expect(rank("conflicts resolve", "re")).toBe(1);
    expect(rank("Dashboard", "sh")).toBe(2);
    expect(rank("Graph", "zz")).toBeNull();
  });
});

describe("palette rows", () => {
  it("answers 'co' with pages, objects and commands, in the design's groups", () => {
    const rows = paletteRows({ ...base, q: "co" });
    const groups = [...new Set(rows.map((r) => r.group))];
    expect(groups).toEqual(["Go to", "Objects", "Run a command"]);
    expect(rows[0].label).toBe("Conflicts");
    expect(rows.find((r) => r.group === "Objects")?.to).toBe("/w/erp/graph?focus=obj%3Ac_country");
  });

  it("offers only the commands the caller may run, and each opens its form", () => {
    const rows = paletteRows({ ...base, q: "c" }).filter((r) => r.group === "Run a command");
    expect(rows.map((r) => r.label)).not.toContain("clean");
    const commit = rows.find((r) => r.label === "commit");
    expect(commit?.to).toBe("/w/erp/commands?cmd=commit");
    expect(commit?.detail).toContain("may call the configured model");
    expect(rows.find((r) => r.label === "conflicts resolve")?.detail).toBe(
      "a decision recorded in your name",
    );
  });

  it("lists no commands until something is typed", () => {
    expect(paletteRows({ ...base, q: "" }).some((r) => r.group === "Run a command")).toBe(false);
  });

  it("offers the other projects, not the open one", () => {
    const rows = paletteRows({ ...base, q: "" }).filter((r) => r.group === "Projects");
    expect(rows.map((r) => r.label)).toEqual(["plausible-analytics"]);
  });

  it("outside a project offers pages and projects only", () => {
    const rows = paletteRows({ ...base, workspace: undefined, pages: homeNav(me), q: "s" });
    expect(rows.every((r) => r.group === "Go to" || r.group === "Projects")).toBe(true);
  });
});
