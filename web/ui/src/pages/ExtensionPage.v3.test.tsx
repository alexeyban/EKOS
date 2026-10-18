// RFC 0207: selections that panels and actions use, and the code / graph / markdown views.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { Me } from "../WorkspaceShell";
import { ExtensionPage } from "./ExtensionPage";
import { actionArgs, dependents, layoutGraph, missingParams, selectionQuery, type ExtPage } from "./extension-view";
import { parseMarkdown } from "./markdown";

const PAGE: ExtPage = {
  extension: "lakehouse",
  id: "pentaho",
  title: "Pentaho → dbt",
  description: "",
  params: [
    { name: "package", label: "Package", source: "packages", rows: "/packages", key: "/package" },
    { name: "step", label: "Step", source: "steps", rows: "/steps", key: "/step" },
  ],
  panels: [
    {
      id: "packages",
      title: "Packages",
      command: ["lakehouse", "pentaho", "scan", "--json"],
      view: { kind: "table", rows: "/packages", columns: [{ key: "/package", label: "Package", badge: false }] },
      empty: "none",
    },
    {
      id: "steps",
      title: "Steps",
      command: ["lakehouse", "pentaho", "report", "--json"],
      params: ["package"],
      view: {
        kind: "table",
        rows: "/steps",
        columns: [
          { key: "/step", label: "Step", badge: false },
          { key: "/fidelity", label: "Fidelity", badge: true },
        ],
      },
      empty: "none",
    },
    {
      id: "step",
      title: "Step",
      command: ["lakehouse", "pentaho", "report", "--json"],
      params: ["package", "step"],
      view: { kind: "code", left: "/step/original", right: "/step/sql", left_label: "Kettle", right_label: "SQL" },
      empty: "none",
    },
    {
      id: "graph",
      title: "Graph",
      command: ["lakehouse", "pentaho", "report", "--json"],
      params: ["package"],
      view: {
        kind: "graph",
        nodes: "/graph/nodes",
        edges: "/graph/edges",
        id: "/id",
        label: "/label",
        group: "/group",
        badge: "/fidelity",
        from: "/from",
        to: "/to",
      },
      empty: "no graph",
    },
    {
      id: "proposal",
      title: "Proposal",
      command: ["lakehouse", "pentaho", "report", "--json"],
      params: ["package"],
      view: { kind: "markdown", pointer: "/proposal" },
      empty: "none",
    },
  ],
  actions: [
    {
      label: "Translate",
      command: ["lakehouse", "pentaho", "translate"],
      binds: [["package", "package"]],
      flags: ["allow-metered"],
      confirm: "This calls a paid model.",
    },
  ],
};

const base = "/workspaces/ws1/extensions/lakehouse/pentaho";
const REPORT = {
  steps: [
    { step: "orders", fidelity: "exact" },
    { step: "js", fidelity: "gap" },
  ],
  step: { original: "var x = 1;", sql: "SELECT 1" },
  graph: {
    nodes: [
      { id: "t/orders", label: "orders", group: "t", fidelity: "exact" },
      { id: "t/js", label: "js", group: "t", fidelity: "gap" },
    ],
    edges: [{ from: "t/orders", to: "t/js" }],
  },
  proposal: "# Pentaho package\n\n| Model | Fidelity |\n|---|---|\n| `gold__x` | exact |\n\n<script>alert(1)</script>",
};

function renderPage() {
  const calls: string[] = [];
  vi.spyOn(client, "api").mockImplementation(async (path: string) => {
    calls.push(path);
    if (path === "/extensions") return { pages: [PAGE] } as never;
    if (path === "/workspaces/ws1/cli")
      return {
        commands: [{ path: ["lakehouse", "pentaho", "translate"], class: "external", allowed: true, confirm: true }],
      } as never;
    if (path === `${base}/packages`) return { packages: [{ package: "etl" }, { package: "crm" }] } as never;
    if (path.startsWith(`${base}/`)) return REPORT as never;
    throw new Error(`unexpected ${path}`);
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/ws1/x/lakehouse/pentaho"]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={{ mode: "token", email: null, role: "write" } as Me} />}>
            <Route path="x/:ext/:page" element={<ExtensionPage />} />
          </Route>
          <Route path="/runs/:runId" element={<p>run page</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return calls;
}

afterEach(() => vi.restoreAllMocks());

describe("RFC 0207 helpers", () => {
  it("knows what a panel or action still needs, and what a selection invalidates", () => {
    expect(missingParams(PAGE.panels[2], { package: "etl" })).toEqual(["step"]);
    expect(selectionQuery(PAGE.panels[2], { package: "etl", step: "a b" })).toBe("?package=etl&step=a+b");
    expect(dependents(PAGE, "package")).toEqual(["step"]);
    expect(actionArgs(PAGE.actions[0], {})).toBeNull();
    expect(actionArgs(PAGE.actions[0], { package: "etl" })).toEqual({ package: "etl", "allow-metered": true });
  });

  it("lays a graph out in layers and lanes, and survives a cycle", () => {
    const g = layoutGraph(
      [
        { id: "a", label: "a", group: "t1", badge: "exact" },
        { id: "b", label: "b", group: "t1", badge: "gap" },
        { id: "c", label: "c", group: "t2", badge: "exact" },
      ],
      [
        { from: "a", to: "b" },
        { from: "b", to: "a" },
        { from: "a", to: "zzz" },
      ],
    );
    expect(g.nodes.map((n) => n.id)).toEqual(["a", "b", "c"]);
    expect(g.edges.length).toBe(2);
    const c = g.nodes.find((n) => n.id === "c")!;
    expect(c.y).toBeGreaterThan(g.nodes[0].y);
  });

  it("parses Markdown into blocks without HTML", () => {
    const b = parseMarkdown(REPORT.proposal);
    expect(b.map((x) => x.kind)).toEqual(["heading", "table", "paragraph"]);
    expect(b[2]).toEqual({ kind: "paragraph", text: "<script>alert(1)</script>" });
  });
});

describe("ExtensionPage with selections", () => {
  it("waits for a selection, then drills down by row click", async () => {
    const calls = renderPage();
    expect(await screen.findByRole("cell", { name: "crm" })).toBeTruthy();
    expect(screen.getAllByText(/Choose a package/).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("cell", { name: "etl" }));
    expect(await screen.findByRole("cell", { name: "js" })).toBeTruthy();
    expect(calls).toContain(`${base}/steps?package=etl`);
    expect(screen.getByText("Choose a step to see this.")).toBeTruthy();
    fireEvent.click(screen.getByRole("cell", { name: "js" }));
    expect(await screen.findByText("var x = 1;")).toBeTruthy();
    expect(screen.getByText("SELECT 1")).toBeTruthy();
    expect(calls).toContain(`${base}/step?package=etl&step=js`);
    // The graph and the proposal, with the script shown as text.
    expect(screen.getByRole("img", { name: "graph" })).toBeTruthy();
    expect(screen.getByText("<script>alert(1)</script>")).toBeTruthy();
    expect(document.querySelector("script")).toBeNull();
  });

  it("runs an action on the selection after consent and the typed name", async () => {
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ run_id: "r-9" } as never);
    renderPage();
    const btn = (await screen.findByRole("button", { name: "Translate" })) as HTMLButtonElement;
    await waitFor(() => expect(btn.disabled).toBe(true));
    fireEvent.change(screen.getByLabelText("Package"), { target: { value: "etl" } });
    const ready = (await screen.findByRole("button", { name: "Translate: etl" })) as HTMLButtonElement;
    await waitFor(() => expect(ready.disabled).toBe(false));
    fireEvent.click(ready);
    const run = screen.getByRole("button", { name: "Run" }) as HTMLButtonElement;
    fireEvent.change(screen.getByLabelText("type lakehouse pentaho translate to confirm"), {
      target: { value: "lakehouse pentaho translate" },
    });
    expect(run.disabled).toBe(true);
    fireEvent.click(screen.getByLabelText(/This calls a paid model/));
    expect(run.disabled).toBe(false);
    fireEvent.click(run);
    await screen.findByText("run page");
    expect(post).toHaveBeenCalledWith("/workspaces/ws1/cli", {
      command: ["lakehouse", "pentaho", "translate"],
      args: { package: "etl", "allow-metered": true },
      confirm: "lakehouse pentaho translate",
    });
  });
});
