import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { ImportPlan, LinkmlSchema } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { Linkml } from "./Linkml";

const SCHEMA: LinkmlSchema = {
  id: "https://example.com/shop",
  name: "shop_semantics",
  description: "Recovered business semantics for shop",
  classes: {
    Customer: {
      is_a: "customers",
      description: "A person who buys",
      comments: ["GAP: no definition of a dormant customer", "seen in 3 views"],
      annotations: { ekos_id: "c-1", ekos_status: "needs_review", ekos_confidence: "0.8" },
      slot_usage: {
        status: {
          required: true,
          pattern: "^[AL]$",
          annotations: { ekos_enum: "CustomerStatus", ekos_id: "s-1", ekos_constraint: "status in (A, L)" },
        },
      },
    },
    customers: {
      attributes: {
        id: { identifier: true, range: "integer" },
        balance: { minimum_value: 0, maximum_value: 1000, annotations: { ekos_sql_type: "NUMERIC" } },
      },
    },
  },
  enums: {
    CustomerStatus: {
      description: "Customer status codes",
      permissible_values: {
        A: { description: "active", annotations: { ekos_status: "confirmed", ekos_source: "CHECK", ekos_usage_sites: "4", ekos_id: "e-1" } },
        L: { annotations: { ekos_status: "hypothesis", ekos_gap: "no label found", ekos_id: "e-2" } },
      },
    },
  },
};

const YAML = "id: https://example.com/shop\nname: shop_semantics\nclasses:\n  Customer:\n    description: A person who buys\n";

const plan = (over: Partial<ImportPlan> = {}): ImportPlan =>
  ({
    decisions: [
      {
        id: "c-1",
        kind: "BusinessConcept",
        path: "classes.Customer",
        action: "edit",
        changes: { description: ["A person who buys", "A person who has bought"] },
        note: null,
      },
    ],
    errors: [],
    warnings: ["ekos_confidence is ignored"],
    applied: 0,
    ...over,
  }) as ImportPlan;

const writer: Me = { mode: "oidc", email: "a@b.c", role: "write" };
const reader: Me = { mode: "oidc", email: "r@b.c", role: "read" };
const tokenWriter: Me = { mode: "token", email: null, role: "write" };

function mockApi(over: { json?: object; yaml?: object } = {}) {
  return vi.spyOn(client, "api").mockImplementation((path: string) => {
    if (path.startsWith("/workspaces/shop/semantics/linkml/yaml"))
      return Promise.resolve(over.yaml ?? { empty: false, yaml: YAML }) as never;
    if (path.startsWith("/workspaces/shop/semantics/linkml"))
      return Promise.resolve(over.json ?? { empty: false, schema: SCHEMA }) as never;
    return Promise.reject(new Error(`unexpected ${path}`));
  });
}

function renderLinkml(me: Me = writer) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/shop/linkml"]}>
        <Routes>
          <Route path="/w/:id/linkml" element={<Linkml me={me} />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  try {
    localStorage.clear();
  } catch {
    /* no storage */
  }
});

describe("LinkML viewer", () => {
  it("shows concepts, enums and tables, and filters them", async () => {
    mockApi();
    renderLinkml();
    expect(await screen.findByText("shop_semantics")).toBeTruthy();
    expect(screen.getByText("Recovered business semantics for shop")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Concepts \(1\)/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Enums \(1\)/ })).toBeTruthy();
    // Tables start folded.
    expect(screen.queryByRole("button", { name: "customers" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Tables \(1\)/ }));
    expect(screen.getByRole("button", { name: "customers" })).toBeTruthy();
    fireEvent.change(screen.getByLabelText("filter schema"), { target: { value: "status" } });
    expect(screen.getByRole("button", { name: /Concepts \(0\)/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Enums \(1\)/ })).toBeTruthy();
  });

  it("a concept shows its description, gaps, annotations and constraints, and links onward", async () => {
    mockApi();
    renderLinkml();
    fireEvent.click(await screen.findByRole("button", { name: /^Customer\s*needs review/ }));
    expect(screen.getByRole("heading", { name: "Customer" })).toBeTruthy();
    expect(screen.getByText("A person who buys")).toBeTruthy();
    expect(screen.getByText(/GAP: no definition/).className).toBe("warn-line");
    expect(screen.getByText("confidence")).toBeTruthy();
    expect(screen.getByText("Constraints (slot_usage)")).toBeTruthy();
    expect(screen.getByText("required, pattern ^[AL]$")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Review…" })).toBeTruthy();
    // The code list of a slot opens the enum.
    fireEvent.click(screen.getByRole("button", { name: "codes: CustomerStatus" }));
    expect(screen.getByRole("heading", { name: "CustomerStatus" })).toBeTruthy();
    expect(screen.getByText("active")).toBeTruthy();
    expect(screen.getByText("unknown")).toBeTruthy(); // L has no label yet
    expect(screen.getAllByRole("button", { name: "review" })).toHaveLength(2);
  });

  it("a table shows its columns, and is_a leads to it", async () => {
    mockApi();
    renderLinkml();
    fireEvent.click(await screen.findByRole("button", { name: /^Customer\s*needs review/ }));
    fireEvent.click(screen.getByRole("button", { name: "customers" }));
    expect(screen.getByRole("heading", { name: "customers" })).toBeTruthy();
    expect(screen.getByText("identifier")).toBeTruthy();
    expect(screen.getByText("≥ 0, ≤ 1000")).toBeTruthy();
    expect(screen.getAllByText("NUMERIC").length).toBeGreaterThan(0);
  });

  it("an empty export says why, an error shows", async () => {
    mockApi({ json: { empty: true, reason: "run ekos commit with [semantics] enabled" } });
    const first = renderLinkml();
    expect(await screen.findByText("run ekos commit with [semantics] enabled")).toBeTruthy();
    first.unmount();
    vi.restoreAllMocks();
    vi.spyOn(client, "api").mockRejectedValue(new Error("502 the export failed"));
    renderLinkml();
    expect(await screen.findByText(/502 the export failed/)).toBeTruthy();
  });

  it("the status filter changes what is asked for", async () => {
    const api = mockApi();
    renderLinkml();
    await screen.findByText("shop_semantics");
    fireEvent.change(screen.getByLabelText("export status"), { target: { value: "confirmed" } });
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith("/workspaces/shop/semantics/linkml?status=confirmed"),
    );
  });
});

describe("LinkML editor", () => {
  const openEditor = async () => {
    fireEvent.click(await screen.findByRole("button", { name: "YAML editor" }));
    await waitFor(() => expect((document.querySelector("textarea") as HTMLTextAreaElement).value).toBe(YAML));
  };

  it("validates, shows the decisions, then applies them", async () => {
    mockApi();
    const post = vi
      .spyOn(client, "apiPost")
      .mockResolvedValueOnce(plan())
      .mockResolvedValueOnce(plan({ applied: 1 }));
    renderLinkml();
    await openEditor();
    const apply = () => screen.getByRole("button", { name: /^Apply/ }) as HTMLButtonElement;
    expect(apply().disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Validate" }));
    expect(await screen.findByText("classes.Customer")).toBeTruthy();
    expect(screen.getByText("ekos_confidence is ignored")).toBeTruthy();
    expect(apply().textContent).toMatch(/1 decision/);
    expect(apply().disabled).toBe(false);
    fireEvent.click(apply());
    expect(await screen.findByText(/Applied 1 decision\(s\) to the ledger/)).toBeTruthy();
    expect(post).toHaveBeenLastCalledWith("/workspaces/shop/semantics/linkml/import", { yaml: YAML, reviewer: null });
  });

  it("an edit after validation must be validated again; the diff shows it", async () => {
    mockApi();
    vi.spyOn(client, "apiPost").mockResolvedValue(plan());
    renderLinkml();
    await openEditor();
    fireEvent.click(screen.getByRole("button", { name: "Validate" }));
    await screen.findByText("classes.Customer");
    const box = document.querySelector("textarea") as HTMLTextAreaElement;
    fireEvent.change(box, { target: { value: YAML.replace("buys", "has bought") } });
    expect(screen.getByText(/edited since validation/)).toBeTruthy();
    expect((screen.getByRole("button", { name: /^Apply/ }) as HTMLButtonElement).disabled).toBe(true);
    // A tab key inserts two spaces (YAML forbids tabs).
    box.setSelectionRange(0, 0);
    fireEvent.keyDown(box, { key: "Tab" });
    expect(box.value.startsWith("  id:")).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: /Diff vs current export/ }));
    expect(screen.getByRole("table", { name: "diff vs current export" })).toBeTruthy();
    expect(screen.getByText(/changed block\(s\)/)).toBeTruthy();
  });

  it("an unchanged text says so in the diff; errors keep Apply off", async () => {
    mockApi();
    vi.spyOn(client, "apiPost").mockResolvedValue(plan({ errors: ["line 3: unknown ekos_id"], decisions: [] }));
    renderLinkml();
    await openEditor();
    fireEvent.click(screen.getByRole("button", { name: /Diff vs current export/ }));
    expect(screen.getByText(/No edits: the text matches/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.click(screen.getByRole("button", { name: "Validate" }));
    expect(await screen.findByText("line 3: unknown ekos_id")).toBeTruthy();
    expect((screen.getByRole("button", { name: /^Apply/ }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("no decisions: the file matches the ledger", async () => {
    mockApi();
    vi.spyOn(client, "apiPost").mockResolvedValue(plan({ decisions: [], warnings: [] }));
    renderLinkml();
    await openEditor();
    fireEvent.click(screen.getByRole("button", { name: "Validate" }));
    expect(await screen.findByText(/the file matches the ledger/)).toBeTruthy();
  });

  it("a token applies under a named reviewer; a reader cannot apply", async () => {
    mockApi();
    const post = vi.spyOn(client, "apiPost").mockResolvedValue(plan());
    const first = renderLinkml(tokenWriter);
    await openEditor();
    fireEvent.click(screen.getByRole("button", { name: "Validate" }));
    await screen.findByText("classes.Customer");
    const apply = screen.getByRole("button", { name: /^Apply/ }) as HTMLButtonElement;
    expect(apply.disabled).toBe(true); // no reviewer name yet
    fireEvent.change(screen.getByRole("textbox", { name: /Reviewer/ }), { target: { value: "ci-bot" } });
    expect(apply.disabled).toBe(false);
    fireEvent.click(apply);
    await waitFor(() =>
      expect(post).toHaveBeenLastCalledWith("/workspaces/shop/semantics/linkml/import", { yaml: YAML, reviewer: "ci-bot" }),
    );
    first.unmount();

    renderLinkml(reader);
    await openEditor();
    expect(screen.getByText("applying needs the write role")).toBeTruthy();
  });

  it("a refused validation shows the server's reason; an empty export its reason", async () => {
    mockApi({ yaml: { empty: true, reason: "nothing recovered yet", yaml: "" } });
    renderLinkml();
    fireEvent.click(await screen.findByRole("button", { name: "YAML editor" }));
    expect(await screen.findByText("nothing recovered yet")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Validate" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Download .yaml" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("downloads the text and loads a file", async () => {
    mockApi();
    vi.spyOn(client, "apiPost").mockRejectedValue(new Error("400 not YAML"));
    const created = vi.fn(() => "blob:x");
    const revoked = vi.fn();
    Object.assign(URL, { createObjectURL: created, revokeObjectURL: revoked });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    renderLinkml();
    await openEditor();
    fireEvent.click(screen.getByRole("button", { name: "Download .yaml" }));
    expect(created).toHaveBeenCalled();
    expect(click).toHaveBeenCalled();
    expect(revoked).toHaveBeenCalledWith("blob:x");

    const input = document.querySelector("input[type=file]") as HTMLInputElement;
    const file = new File(["name: loaded\n"], "s.yaml");
    Object.defineProperty(file, "text", { value: () => Promise.resolve("name: loaded\n") });
    fireEvent.change(input, { target: { files: [file] } });
    await waitFor(() => expect((document.querySelector("textarea") as HTMLTextAreaElement).value).toBe("name: loaded\n"));
    fireEvent.click(screen.getByRole("button", { name: "Validate" }));
    expect(await screen.findByText(/400 not YAML/)).toBeTruthy();
  });
});
