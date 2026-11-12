import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { ProfileShow, SemanticsDetail } from "../api/types";
import { usageTarget } from "./SemanticsItemPanel";
import { UsageOverTime } from "./UsageOverTime";
import { bars, describe as say, peak } from "./usage-chart";

describe("bars", () => {
  it("fills the months between the first and the last and keeps a withheld count withheld", () => {
    const b = bars([
      { month: "2026-01", rows: 10 },
      { month: "2026-03", rows: null },
    ]);
    expect(b.map((x) => x.month)).toEqual(["2026-01", "2026-02", "2026-03"]);
    expect(b[1]).toEqual({ month: "2026-02", rows: 0, empty: true });
    expect(b[2]).toEqual({ month: "2026-03", rows: null, empty: false });
    expect(peak(b)).toBe(10);
    expect(say(b, 5)).toBe("2026-01 to 2026-03, most rows in 2026-01 (10), 1 month(s) with fewer than 5 rows");
  });

  it("keeps the latest months only, across a year boundary", () => {
    const b = bars(
      [
        { month: "2024-11", rows: 1 },
        { month: "2025-02", rows: 2 },
      ],
      2,
    );
    expect(b.map((x) => x.month)).toEqual(["2025-01", "2025-02"]);
  });

  it("has nothing to draw without dated rows", () => {
    expect(bars([])).toEqual([]);
    expect(say([], 5)).toBe("no dated rows");
  });
});

const item = (kind: string, props: Record<string, unknown>): SemanticsDetail => ({
  id: "x",
  kind: kind as SemanticsDetail["kind"],
  name: "n",
  properties: props,
  evidence: [],
  links: [],
});

describe("usageTarget", () => {
  it("charts a code value, the values no code names, and nothing else", () => {
    expect(usageTarget(item("EnumMeaning", { table: "orders", column: "status", value: "'A'" }))).toEqual({
      table: "orders",
      column: "status",
      value: "'A'",
    });
    expect(
      usageTarget(item("SemanticGap", { table: "orders", column: "status", gap_type: "data_only_values" })),
    ).toEqual({ table: "orders", column: "status", value: null });
    expect(usageTarget(item("BusinessConcept", { table: "orders" }))).toBeNull();
  });
});

const PROFILE: ProfileShow = {
  measured: true,
  columns: [
    {
      database: "erp",
      owner: "public",
      table: "orders",
      column: "status",
      tier: "p1",
      time_column: "created_at",
      coverage: {
        non_null: 50,
        literals: { "'A'": { rows: 45 } },
        unexplained: { rows: 5 },
        monthly: {
          min_cell: 5,
          literals: {
            "'A'": [
              { month: "2026-01", rows: 40 },
              { month: "2026-02", rows: null },
            ],
          },
          other: [{ month: "2026-02", rows: 5 }],
        },
      },
      code_history: {
        "'A'": {
          introduced: "2026-02-03T00:00:00+00:00",
          sha: "a".repeat(40),
          path: "sql/orders.sql",
          line: 12,
          data_first_seen: "2026-01",
          order: "data_before_code",
        },
      },
      values_no_code_names: { new: true, code_last_changed: { date: "2025-12-01T00:00:00Z", sha: "b" } },
    },
  ],
};

function show(value: string | null) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <UsageOverTime workspace="w" table="orders" column="status" value={value} />
    </QueryClientProvider>,
  );
}

describe("UsageOverTime", () => {
  afterEach(() => vi.restoreAllMocks());

  it("draws a value's months with the month the code first named it", async () => {
    const spy = vi.spyOn(client, "api").mockResolvedValue(PROFILE);
    show("'A'");
    const chart = await screen.findByRole("img");
    expect(chart.getAttribute("aria-label")).toContain("1 month(s) with fewer than 5 rows");
    expect(screen.getByText(/before the code knew it/)).toBeTruthy();
    expect(spy).toHaveBeenCalledWith("/workspaces/w/profile?table=orders");
    expect(chart.querySelectorAll("rect")).toHaveLength(2);
    expect(chart.querySelector("line")).not.toBeNull();
  });

  it("says when every value no code names is newer than the code", async () => {
    vi.spyOn(client, "api").mockResolvedValue(PROFILE);
    show(null);
    expect(await screen.findByText(/first appeared after the code last changed/)).toBeTruthy();
  });

  it("shows nothing before any profile run", async () => {
    const spy = vi.spyOn(client, "api").mockResolvedValue({ measured: false, reason: "no profile run yet" });
    const { container } = show("'A'");
    await vi.waitFor(() => expect(spy).toHaveBeenCalled());
    expect(container.querySelector(".usage-over-time")).toBeNull();
  });
});
