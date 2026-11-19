import { describe, expect, it } from "vitest";
import { allItems, current, homeNav, workspaceNav, type Identity } from "./nav";

const writer: Identity = { mode: "local", kind: "local", email: null, role: "write", owner: true };
const reader: Identity = { mode: "oidc", email: "r@x", role: "read" };
const showcase: Identity = {
  mode: "oidc",
  email: null,
  name: "github:root",
  role: "read",
  owner: true,
  read_only: true,
  tenants: [{ id: "demo", name: "Demo", roles: ["admin"] }],
};
const labels = (groups: ReturnType<typeof homeNav>) => allItems(groups).map((i) => i.label);
const ext = [{ extension: "lakehouse", id: "main", title: "Lakehouse" }];

describe("workspace navigation", () => {
  it("keeps every former tab, in the design's groups", () => {
    const groups = workspaceNav("erp", writer, { restricted: false }, ext);
    expect(groups.map((g) => g.label)).toEqual([
      "Overview",
      "Explore",
      "Decide",
      "Check",
      "Operate",
      "Configure",
      "Extensions",
    ]);
    expect(labels(groups)).toEqual([
      "Dashboard", "Sources", "Graph", "Chat", "Transformations", "Interaction", "Review",
      "Semantics", "Conflicts", "Questions", "Inbox", "Notes", "Integrity", "Evals", "Run",
      "Commands", "History", "Schedules", "ekos.toml", "Lakehouse",
    ]);
    expect(allItems(groups).find((i) => i.label === "Dashboard")?.to).toBe("/w/erp");
    expect(allItems(groups).find((i) => i.label === "Lakehouse")?.to).toBe("/w/erp/x/lakehouse/main");
  });

  it("shows a restricted reader the graph only, and no extension pages (RFC 0181)", () => {
    expect(labels(workspaceNav("erp", writer, { restricted: true }, ext))).toEqual(["Graph"]);
  });

  it("leaves out what acts or calls a paid model in a read-only showcase (RFC 0251)", () => {
    const shown = labels(workspaceNav("erp", showcase, { restricted: false }, ext));
    for (const hidden of ["Run", "Commands", "Chat", "ekos.toml", "Schedules", "Lakehouse"]) {
      expect(shown).not.toContain(hidden);
    }
    expect(shown).toContain("Graph");
  });

  it("gives Schedules to writers only", () => {
    expect(labels(workspaceNav("erp", reader, { restricted: false }))).not.toContain("Schedules");
  });
});

describe("home navigation", () => {
  it("gives an owner the whole install", () => {
    expect(labels(homeNav(writer))).toEqual(["Projects", "Schedules", "Settings", "Channels", "Admin"]);
  });

  it("gives a plain reader projects and channels", () => {
    expect(labels(homeNav(reader))).toEqual(["Projects", "Channels"]);
  });

  it("hides Admin, Settings and Schedules in a read-only showcase", () => {
    expect(labels(homeNav(showcase))).toEqual(["Projects"]);
  });

  it("gives a tenant admin the Admin page", () => {
    const admin: Identity = { ...reader, tenants: [{ id: "t", name: "T", roles: ["admin"] }] };
    expect(labels(homeNav(admin))).toContain("Admin");
  });
});

describe("current page", () => {
  const groups = workspaceNav("erp", writer, { restricted: false });
  it("matches the longest item, so a sub-page is not the dashboard", () => {
    expect(current(groups, "/w/erp")?.label).toBe("Dashboard");
    expect(current(groups, "/w/erp/")?.label).toBe("Dashboard");
    expect(current(groups, "/w/erp/runs")?.label).toBe("History");
    expect(current(groups, "/w/erp/evals/run-1.json")?.label).toBe("Evals");
    expect(current(groups, "/w/erp/graph")?.group).toBe("Explore");
  });
  it("finds nothing for a path outside the model", () => {
    expect(current(groups, "/w/other/graph")).toBeUndefined();
  });
});
