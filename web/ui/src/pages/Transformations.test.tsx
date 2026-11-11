import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { Me } from "../WorkspaceShell";
import {
  type ChainGraph,
  type ChainNode,
  chainLayout,
  expandWith,
  segments,
} from "./chain-shared";
import { Transformations } from "./Transformations";

const N1 = "11111111-1111-4111-8111-111111111111";
const TEXT = "CREATE VIEW v AS SELECT o.id FROM oe o WHERE o.docstatus IN ('CO','CL')";

const step = (over: Partial<ChainNode>): ChainNode => ({
  id: "n0",
  index: 0,
  type: "Source",
  label: "oe",
  summary: "reads `oe` as `o`",
  group: "main",
  statement: null,
  columns_in: [],
  columns_out: [],
  columns_used: [],
  source_text: "oe",
  span: null,
  term: null,
  rule: null,
  fidelity: null,
  status: null,
  evidence: [],
  annotations: [],
  members: [],
  ...over,
});

const filterSpan = { node: N1, start: 45, end: TEXT.length, start_line: 1, end_line: 1 };
const sourceSpan = { node: "n0", start: 34, end: 36, start_line: 1, end_line: 1 };

const graph = (over: Partial<ChainGraph> = {}): ChainGraph => ({
  format: "ekos.transformation-graph/1",
  transformation: { key: "a.sql#0", kind: "view", name: "v", object: "view-1", path: "a.sql", recovered: true },
  source: { path: "a.sql", start_line: 1, text: TEXT },
  nodes: [
    step({ span: sourceSpan }),
    step({
      id: N1,
      index: 1,
      type: "Filter",
      label: "where o.docstatus IN…",
      summary: "keeps rows where `o.docstatus` is one of CO, CL",
      source_text: "o.docstatus IN ('CO', 'CL')",
      columns_used: ["o.docstatus"],
      span: filterSpan,
      annotations: [
        {
          id: "a1",
          target: N1,
          kind: "comment",
          text: "cancelled orders too?",
          rule: null,
          term: null,
          author: "ann",
          scope: "personal",
          approved_by: null,
          state: "active",
          updated_at: "2026-11-11T10:00:00Z",
        },
      ],
    }),
    step({ id: "n2", index: 2, type: "Unmapped", label: "not understood", status: "unmapped", summary: "not understood by recovery: CTE" }),
  ],
  edges: [
    { from: "n0", to: N1, kind: "data" },
    { from: N1, to: "n2", kind: "data" },
  ],
  spans: [sourceSpan, filterSpan],
  groups: [{ id: "main", label: "steps", size: 3, collapsed: false }],
  collapsed: false,
  counts: { nodes: 3, mapped: 2, unmapped: 1, shown: 3 },
  rows: { available: false, reason: "row counts are measured for a mart only" },
  ...over,
});

function renderPage(me: Me = { mode: "oidc", email: "ann@x", role: "write" }, path = "/w/ws1/transformations") {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={me} />}>
            <Route path="transformations" element={<Transformations />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function mockApi(g: ChainGraph = graph()) {
  return vi.spyOn(client, "api").mockImplementation((path: string) =>
    Promise.resolve(
      path.includes("/transformations/graph")
        ? g
        : {
            total: 1,
            hidden: 4,
            transformations: [{ key: "a.sql#0", name: "v", kind: "view", object: "n0", steps: 3, unmapped: 1 }],
          },
    ),
  );
}

afterEach(() => vi.restoreAllMocks());

describe("chain helpers", () => {
  it("cuts the source at span boundaries; a segment selects its innermost span", () => {
    const outer = { node: "j", start: 0, end: 10, start_line: 1, end_line: 1 };
    const inner = { node: "s", start: 2, end: 4, start_line: 1, end_line: 1 };
    const segs = segments(12, [outer, inner], "j");
    expect(segs.map((s) => [s.start, s.end, s.node, s.selected])).toEqual([
      [0, 2, "j", true],
      [2, 4, "s", true],
      [4, 10, "j", true],
      [10, 12, null, false],
    ]);
    expect(segments(5, [{ ...inner, end: 99 }], null)).toEqual([{ start: 0, end: 5, node: null, selected: false }]);
  });

  it("asks for one more statement in full, 1-based", () => {
    expect(expandWith("", 2)).toBe("3");
    expect(expandWith("3", 0)).toBe("1,3");
    expect(expandWith("all", 4)).toBe("all");
    expect(expandWith("2", null)).toBe("2");
  });

  it("lays the chain out left to right; collapsed statements share one lane", () => {
    const l = chainLayout(graph());
    const x = new Map(l.nodes.map((n) => [n.id, n.x]));
    expect(x.get("n0")! < x.get(N1)! && x.get(N1)! < x.get("n2")!).toBe(true);
    const collapsed = graph({
      nodes: [
        step({ id: "statement:0", type: "Statement", group: "statement:0", statement: 0, label: "statements 1–4" }),
        step({ id: "statement:4", type: "Statement", group: "statement:4", statement: 4, label: "statement 5" }),
      ],
      edges: [{ from: "statement:0", to: "statement:4", kind: "order" }],
    });
    const c = chainLayout(collapsed);
    expect(new Set(c.nodes.map((n) => n.group))).toEqual(new Set(["statements"]));
    expect(c.nodes[0].y).toBe(c.nodes[1].y);
  });
});

describe("Transformations page", () => {
  it("lists transformations and opens one as a chain with its source", async () => {
    const api = mockApi();
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "v" }));
    expect(await screen.findByRole("img", { name: "transformation chain" })).toBeInTheDocument();
    expect(api.mock.calls.map((c) => c[0])).toContain("/workspaces/ws1/transformations/graph?object=n0");
    expect(screen.getByText(/4 statement\(s\) nothing was understood of not listed/)).toBeInTheDocument();
    expect(screen.getByText(/1 not understood by recovery/)).toBeInTheDocument();
  });

  it("selecting a step shows what it does and highlights its source; the source selects back", async () => {
    mockApi();
    renderPage(undefined, "/w/ws1/transformations?object=n0");
    fireEvent.click(await screen.findByRole("button", { name: /step where o.docstatus/ }));
    const panel = screen.getByRole("complementary", { name: "step" });
    expect(within(panel).getByText("keeps rows where `o.docstatus` is one of CO, CL")).toBeInTheDocument();
    expect(within(panel).getByText("cancelled orders too?")).toBeInTheDocument();
    expect(within(panel).getByText(/row counts are measured for a mart only/)).toBeInTheDocument();
    const source = screen.getByLabelText("source text");
    const marked = source.querySelector("button.chain-span.selected");
    expect(marked?.textContent).toBe("o.docstatus IN ('CO','CL')");
    fireEvent.click(within(source).getByText("oe"));
    expect(within(screen.getByRole("complementary", { name: "step" })).getByText("reads `oe` as `o`")).toBeInTheDocument();
  });

  it("adds an annotation on the step through the annotation route, targeting the node", async () => {
    mockApi();
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ author: "ann@x" });
    renderPage(undefined, "/w/ws1/transformations?object=n0");
    fireEvent.click(await screen.findByRole("button", { name: /step where o.docstatus/ }));
    const form = screen.getByRole("form", { name: "add annotation" });
    expect(within(form).queryByLabelText("target")).toBeNull();
    fireEvent.change(within(form).getByLabelText("text"), { target: { value: "exclude cancelled" } });
    fireEvent.click(within(form).getByRole("button", { name: "Add" }));
    await screen.findByText(/Recorded as ann@x/);
    expect(post).toHaveBeenCalledWith(
      "/workspaces/ws1/annotations",
      expect.objectContaining({ target: N1, kind: "comment", text: "exclude cancelled" }),
    );
  });

  it("a reader sees the notes but no form, and a not-understood step is badged", async () => {
    mockApi();
    renderPage({ mode: "oidc", email: "r@x", role: "read" }, "/w/ws1/transformations?object=n0");
    fireEvent.click(await screen.findByRole("button", { name: /step not understood/ }));
    const panel = screen.getByRole("complementary", { name: "step" });
    expect(within(panel).getByText("unmapped")).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "add annotation" })).toBeNull();
  });

  it("a collapsed statement asks for its steps", async () => {
    const api = mockApi(
      graph({
        collapsed: true,
        nodes: [step({ id: "statement:2", type: "Statement", group: "statement:2", statement: 2, label: "statement 3 · 4 steps", summary: "4 step(s): Source → Filter", members: ["a", "b"] })],
        edges: [],
        spans: [],
      }),
    );
    renderPage(undefined, "/w/ws1/transformations?object=n0");
    fireEvent.click(await screen.findByRole("button", { name: /step statement 3/ }));
    fireEvent.click(screen.getByRole("button", { name: "show its steps" }));
    await vi.waitFor(() =>
      expect(api.mock.calls.map((c) => c[0])).toContain(
        "/workspaces/ws1/transformations/graph?object=n0&expand=3",
      ),
    );
  });
});
