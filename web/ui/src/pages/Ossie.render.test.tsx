import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { ImportPlan, OssieModel } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { Ossie, ekosData, meaningCount } from "./Ossie";

const ext = (v: object) => [{ vendor_name: "EKOS", data: JSON.stringify(v) }];

const MODEL: OssieModel = {
  version: "0.2.0.dev0",
  name: "shop",
  datasets: [
    {
      name: "orders",
      source: "public.orders",
      primary_key: ["id"],
      description: "Customer orders",
      ai_context: { synonyms: ["purchases"] },
      fields: [
        { name: "id", expression: { sql: "id" }, datatype: "Integer" },
        {
          name: "status",
          expression: { sql: "status" },
          datatype: "String",
          ai_context: { instructions: "Codes: 'O' = open." },
          custom_extensions: ext({ enum_meanings: [{ ekos_id: "e1", value: "'O'", label: "open", status: "confirmed" }] }),
        },
      ],
      metrics: [{ name: "order_count", expression: { sql: "COUNT(id)" }, description: "Orders" }],
      custom_extensions: ext({ ekos_id: "t1", kind: "Table", status: "observed" }),
    },
    { name: "customers", source: "customers", fields: [] },
  ],
  relationships: [
    { name: "o_c", from: "orders", to: "customers", from_columns: ["customer_id"], to_columns: ["id"] },
  ],
};

const writer: Me = { mode: "oidc", email: "a@b.c", role: "write" };
const reader: Me = { mode: "oidc", email: "r@b.c", role: "read" };

function mockApi(json: object = { empty: false, model: MODEL }) {
  return vi.spyOn(client, "api").mockImplementation((path: string) => {
    if (path.startsWith("/workspaces/shop/semantics/ossie/yaml"))
      return Promise.resolve({ empty: false, yaml: "version: 0.2.0.dev0\n" }) as never;
    if (path.startsWith("/workspaces/shop/semantics/ossie")) return Promise.resolve(json) as never;
    return Promise.reject(new Error(`unexpected ${path}`));
  });
}

function renderOssie(me: Me = writer) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/shop/ossie"]}>
        <Routes>
          <Route path="/w/:id/ossie" element={<Ossie me={me} />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

describe("Apache Ossie page", () => {
  it("reads the EKOS extension and counts reviewable meanings", () => {
    expect(ekosData(MODEL.datasets[0].custom_extensions).kind).toBe("Table");
    expect(ekosData([{ vendor_name: "EKOS", data: "not json" }])).toEqual({});
    expect(meaningCount(MODEL.datasets[0])).toBe(1);
  });

  it("lists datasets and shows one's fields, meanings, metrics and relationships", async () => {
    mockApi();
    renderOssie();
    expect(await screen.findByText("shop")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /orders/ }));
    expect(screen.getByText("Customer orders")).toBeTruthy();
    expect(screen.getByText("also called: purchases")).toBeTruthy();
    expect(screen.getByText("Codes: 'O' = open.")).toBeTruthy();
    expect(screen.getByText("order_count")).toBeTruthy();
    expect(screen.getByText("orders(customer_id) → customers(id)")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("filter datasets"), { target: { value: "cust" } });
    expect(screen.queryByRole("button", { name: /^orders/ })).toBeNull();
  });

  it("says why a workspace without tables is empty", async () => {
    mockApi({ empty: true, reason: "no tables or views to export" });
    renderOssie();
    expect(await screen.findByText("no tables or views to export")).toBeTruthy();
  });

  it("validates, then applies, an edited export as the reviewer", async () => {
    mockApi();
    const plan: ImportPlan = {
      decisions: [
        { id: "e1", kind: "EnumMeaning", path: "datasets.orders.fields.status.'O'", action: "edit",
          changes: { label: ["open", "open order"] }, note: null },
      ],
      warnings: [],
      errors: [],
      applied: 0,
    };
    const post = vi
      .spyOn(client, "apiPost")
      .mockResolvedValueOnce(plan as never)
      .mockResolvedValueOnce({ ...plan, applied: 1 } as never);
    renderOssie();
    fireEvent.change(await screen.findByLabelText("edited Ossie model"), { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: "Validate" }));
    expect(await screen.findByText("datasets.orders.fields.status.'O'")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Apply 1 decision/ }));
    await waitFor(() => expect(screen.getByText(/Applied 1 decision/)).toBeTruthy());
    expect(post.mock.calls.map((c) => c[0])).toEqual([
      "/workspaces/shop/semantics/ossie/validate",
      "/workspaces/shop/semantics/ossie/import",
    ]);
  });

  it("a reader can validate but not apply", async () => {
    mockApi();
    renderOssie(reader);
    expect(await screen.findByText("applying needs the write role")).toBeTruthy();
  });
});
