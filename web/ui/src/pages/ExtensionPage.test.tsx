import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import { extensionTabs, type Me } from "../WorkspaceShell";
import { ExtensionPage } from "./ExtensionPage";
import { at, rowsOf, show, tone, type ExtPage } from "./extension-view";

const PAGE: ExtPage = {
  extension: "lakehouse",
  id: "lakehouse",
  title: "Lakehouse",
  description: "Health per table and layer.",
  panels: [
    {
      id: "health",
      title: "Health",
      command: ["lakehouse", "status", "--json"],
      view: {
        kind: "table",
        rows: "/tables",
        columns: [
          { key: "/table", label: "Table", badge: false },
          { key: "/bronze/state", label: "Bronze", badge: true },
        ],
      },
      empty: "No tables yet.",
    },
    {
      id: "stack",
      title: "Stack",
      command: ["lakehouse", "tables", "--json"],
      view: { kind: "fields", fields: [{ key: "/settings/trino", label: "Trino", badge: false }] },
      empty: "No settings.",
    },
  ],
  actions: [
    { label: "Sync source changes", command: ["lakehouse", "sync"] },
    { label: "Check", command: ["lakehouse", "check"] },
  ],
};

const DATA: Record<string, unknown> = {
  "/workspaces/ws1/extensions/lakehouse/lakehouse/health": {
    tables: [
      { table: "public.orders", bronze: { state: "healthy" } },
      { table: "public.customers", bronze: { state: "failing" } },
    ],
  },
  "/workspaces/ws1/extensions/lakehouse/lakehouse/stack": { settings: { trino: "http://t:8080" } },
  "/extensions": { pages: [PAGE] },
  "/workspaces/ws1/cli": {
    commands: [
      { path: ["lakehouse", "sync"], class: "external", allowed: true, confirm: true },
      { path: ["lakehouse", "check"], class: "write", allowed: false, confirm: false },
    ],
  },
};

function renderPage(me: Me) {
  vi.spyOn(client, "api").mockImplementation(async (path: string) => {
    if (!(path in DATA)) throw new Error(`unexpected ${path}`);
    return DATA[path] as never;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/ws1/x/lakehouse/lakehouse"]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={me} />}>
            <Route path="x/:ext/:page" element={<ExtensionPage />} />
          </Route>
          <Route path="/runs/:runId" element={<p>run page</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const writer: Me = { mode: "token", email: null, role: "write" };

afterEach(() => vi.restoreAllMocks());

describe("extension view helpers", () => {
  it("walks JSON pointers through objects and arrays", () => {
    const v = { a: [{ b: "x" }], "c/d": 1, "e~f": 2 };
    expect(at(v, "")).toBe(v);
    expect(at(v, "/a/0/b")).toBe("x");
    expect(at(v, "/c~1d")).toBe(1);
    expect(at(v, "/e~0f")).toBe(2);
    expect(at(v, "/a/9/b")).toBeUndefined();
    expect(at(v, "no-slash")).toBeUndefined();
  });

  it("shows values and gives states their tone", () => {
    expect(show(null)).toBe("–");
    expect(show(["a", "b"])).toBe("a, b");
    expect(show({ k: 1 })).toBe('{"k":1}');
    expect(tone("healthy")).toBe("ok");
    expect(tone("schema_pending")).toBe("warn");
    expect(tone("FAILING")).toBe("bad");
    expect(tone("incremental")).toBe("");
  });

  it("finds a table's rows only where the pointer names an array", () => {
    const view = PAGE.panels[0].view;
    expect(rowsOf({ tables: [1, 2] }, view)).toEqual([1, 2]);
    expect(rowsOf({ tables: "x" }, view)).toEqual([]);
  });

  it("puts extension tabs after the built-in ones, keyed by extension and page", () => {
    expect(extensionTabs([PAGE])).toEqual([
      { to: "x/lakehouse/lakehouse", label: "Lakehouse", end: false },
    ]);
    expect(extensionTabs(undefined)).toEqual([]);
  });
});

describe("ExtensionPage", () => {
  it("renders each panel's data with status badges", async () => {
    renderPage(writer);
    expect(await screen.findByText("public.orders")).toBeTruthy();
    expect(screen.getByText("healthy").className).toContain("ok");
    expect(screen.getByText("failing").className).toContain("bad");
    expect(await screen.findByText("http://t:8080")).toBeTruthy();
  });

  it("disables an action the person's role cannot run", async () => {
    renderPage(writer);
    const check = await screen.findByRole("button", { name: "Check" });
    await waitFor(() => expect((check as HTMLButtonElement).disabled).toBe(true));
  });

  it("asks for the typed name before an external action, then opens its run", async () => {
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ run_id: "r-7" } as never);
    renderPage(writer);
    const sync = await screen.findByRole("button", { name: "Sync source changes" });
    await waitFor(() => expect((sync as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(sync);
    const box = screen.getByLabelText("type lakehouse sync to confirm");
    const run = screen.getByRole("button", { name: "Run" }) as HTMLButtonElement;
    expect(run.disabled).toBe(true);
    fireEvent.change(box, { target: { value: "lakehouse sync" } });
    fireEvent.click(run);
    await screen.findByText("run page");
    expect(post).toHaveBeenCalledWith("/workspaces/ws1/cli", {
      command: ["lakehouse", "sync"],
      args: {},
      confirm: "lakehouse sync",
    });
  });
});
