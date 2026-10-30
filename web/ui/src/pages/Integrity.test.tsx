import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { XrefDiagnostic, XrefReport } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { Integrity } from "./Integrity";
import { DEFAULT_FILTERS, applyFilters, pct, where } from "./integrity-shared";

const finding = (over: Partial<XrefDiagnostic> = {}): XrefDiagnostic => ({
  kind: "UnresolvedReference",
  severity: "error",
  confidence: 0.8,
  source: {
    object_id: "o-1",
    object_kind: "Procedure",
    object_name: "lsmb__closest_business_day",
    file: "sql/modules/Business_Dates.sql",
    line: 101,
    extractor: "routine-calls",
    access: "call",
  },
  target_spec: { written: "lsmb__next_businss_day" },
  candidates: [{ id: "o-2", kind: "Procedure", name: "lsmb__next_business_day" }],
  reason: "no compiled routine or built-in function has this name",
  fingerprint: "f1",
  ...over,
});

const report = (diagnostics: XrefDiagnostic[]): XrefReport => ({
  version: 2,
  computed_at: "2026-10-30T16:07:23Z",
  metrics: {
    resolution_rate: 0.98,
    cross_system_resolution_rate: null,
    cycles: 1,
    orphans: { Table: 28 },
    reachability: {
      sql: {
        supported: true,
        symbols: 517,
        entry_points: 20,
        reachable: 22,
        unreachable: 0,
        dynamic: 495,
        reachability_coverage: 0.0426,
        dynamic_share: 0.957,
      },
      perl: {
        supported: false,
        unsupported_because: "no call graph is recovered",
        symbols: 1333,
        entry_points: 0,
        reachable: 0,
        unreachable: 0,
        dynamic: 0,
      },
    },
  },
  diagnostics,
});

function renderPage() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const me: Me = { mode: "token", email: null, role: "read" };
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/ws1/integrity"]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={me} />}>
            <Route path="integrity" element={<Integrity />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

describe("integrity helpers", () => {
  it("formats locations and rates", () => {
    expect(where(finding())).toBe("sql/modules/Business_Dates.sql:101");
    expect(where(finding({ source: { object_name: "X", object_kind: "PerlPackage" } }))).toBe(
      "PerlPackage X",
    );
    expect(pct(0.9876)).toBe("98.8%");
    expect(pct(null)).toBe("—");
  });

  it("hides coverage gaps until asked and puts errors first", () => {
    const items = [
      finding({ fingerprint: "i", severity: "info", confidence: 0.3 }),
      finding({ fingerprint: "g", severity: "info", coverage_gap: true }),
      finding({ fingerprint: "e" }),
    ];
    expect(applyFilters(items, DEFAULT_FILTERS).map((d) => d.fingerprint)).toEqual(["e", "i"]);
    expect(
      applyFilters(items, { ...DEFAULT_FILTERS, showGaps: true }).map((d) => d.fingerprint),
    ).toEqual(["e", "g", "i"]);
    expect(applyFilters(items, { ...DEFAULT_FILTERS, minConfidence: 0.5 })).toHaveLength(1);
    expect(applyFilters(items, { ...DEFAULT_FILTERS, text: "business_dates" })).toHaveLength(2);
  });
});

describe("Integrity view", () => {
  it("shows the metrics with dynamic share beside reachability, and why perl is unsupported", async () => {
    const api = vi.spyOn(client, "api").mockResolvedValue(report([finding()]));
    renderPage();
    expect(await screen.findByText("98.0%")).toBeInTheDocument();
    expect(api.mock.calls[0][0]).toBe("/workspaces/ws1/integrity");
    expect(screen.getByText(/4.3% reachable, 95.7% dynamic/)).toBeInTheDocument();
    expect(screen.getByText(/reachability unsupported — no call graph is recovered/)).toBeInTheDocument();
    expect(screen.getByText("28 Table")).toBeInTheDocument();
  });

  it("lists a finding with its location and closest name, and opens it", async () => {
    vi.spyOn(client, "api").mockImplementation((path: string) =>
      Promise.resolve(path.endsWith("/integrity") ? report([finding()]) : {}),
    );
    renderPage();
    const pick = await screen.findByRole("button", { name: "lsmb__next_businss_day" });
    expect(screen.getByText(/closest: lsmb__next_business_day/)).toBeInTheDocument();
    fireEvent.click(pick);
    expect(pick).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("link", { name: "open in the graph" })).toHaveAttribute(
      "href",
      "/w/ws1/graph?focus=o-1",
    );
  });

  it("says when every reference resolves", async () => {
    vi.spyOn(client, "api").mockResolvedValue(report([]));
    renderPage();
    expect(await screen.findByText(/every checked reference resolves/)).toBeInTheDocument();
  });

  it("points at ekos commit when there is no index yet", async () => {
    vi.spyOn(client, "api").mockRejectedValue(new Error("502: no integrity index"));
    renderPage();
    expect(await screen.findByText(/written by/)).toBeInTheDocument();
  });
});
