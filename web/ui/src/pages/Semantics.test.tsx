import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { LinkmlSchema, SemanticsDetail, SemanticsItem } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { sections } from "./Linkml";
import { Semantics } from "./Semantics";
import { annotation, previousReview, prop, reviewOrder, summary } from "./semantics-shared";

const ID = "3aec3aa7-3fc0-51fd-b90f-a91baa49d3c1";

const concept = (over: Partial<SemanticsItem["properties"]> = {}, id = ID): SemanticsItem => ({
  id,
  kind: "BusinessConcept",
  name: "PartsNotObsolete",
  properties: { status: "hypothesis", definition: "parts.obsolete IS FALSE", confidence: 0.6, ...over },
});

const detail: SemanticsDetail = {
  ...concept(),
  evidence: [{ path: "sql/modules/Parts.sql", line: 12, fragment: "parts.obsolete IS FALSE (x#1, Where)" }],
  links: [{ kind: "Describes", to: "parts (Table)" }],
};

function renderAt(url: string, me: Me) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={me} />}>
            <Route path="semantics" element={<Semantics />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const writer: Me = { mode: "token", email: null, role: "write" };
const reader: Me = { mode: "token", email: null, role: "read" };

afterEach(() => {
  vi.restoreAllMocks();
  try {
    localStorage.clear();
  } catch {
    /* no storage */
  }
});

describe("semantics helpers", () => {
  it("orders the review queue: needs review first, then hypotheses by confidence", () => {
    const items = [
      concept({ status: "confirmed" }, "a"),
      concept({ status: "hypothesis", confidence: 0.4 }, "b"),
      concept({ status: "needs_review" }, "c"),
      concept({ status: "hypothesis", confidence: 0.6 }, "d"),
    ];
    expect(reviewOrder(items).map((i) => i.id)).toEqual(["c", "d", "b", "a"]);
  });

  it("prefers the expert's label and reads both annotation forms", () => {
    const code: SemanticsItem = {
      id: "e",
      kind: "EnumMeaning",
      name: "oe.oe_class_id = 4",
      properties: { label: "RFQ", expert_label: "Request for quotation" },
    };
    expect(summary(code)).toBe("means “Request for quotation”");
    expect(annotation({ ekos_status: "confirmed" }, "ekos_status")).toBe("confirmed");
    expect(annotation({ ekos_status: { value: "rejected" } }, "ekos_status")).toBe("rejected");
  });

  it("renders lists and past decisions readably", () => {
    const c = concept({
      seen_in: ["a#1", "b#2"],
      previous_review: { status: "confirmed", reviewed_by: "ann@x.io", reviewed_at: "2026-10-03T10:00:00Z", review_note: null },
    });
    expect(prop(c, "seen_in")).toBe("a#1, b#2");
    expect(previousReview(c)).toBe("confirmed by ann@x.io on 2026-10-03");
  });

  it("splits a schema into recovered concepts, tables and enums", () => {
    const schema: LinkmlSchema = {
      id: "x",
      name: "x",
      classes: {
        Parts: { description: "table" },
        PartsNotObsolete: { is_a: "Parts", annotations: { ekos_id: ID } },
      },
      enums: { OeClassId: { permissible_values: {} } },
    };
    const s = sections(schema);
    expect(s.concepts.map(([n]) => n)).toEqual(["PartsNotObsolete"]);
    expect(s.tables.map(([n]) => n)).toEqual(["Parts"]);
    expect(s.enums.map(([n]) => n)).toEqual(["OeClassId"]);
  });
});

describe("Semantics review", () => {
  it("lists hypotheses and opens one with its evidence", async () => {
    vi.spyOn(client, "api").mockImplementation(async (path: string) =>
      path.includes(`/items/${ID}`) ? detail : [concept()],
    );
    renderAt("/w/ws1/semantics", reader);
    fireEvent.click(await screen.findByText("PartsNotObsolete"));
    expect(await screen.findByText("sql/modules/Parts.sql:12")).toBeInTheDocument();
    expect(screen.getByText("Reviewing needs the write role.")).toBeInTheDocument();
    expect(screen.queryByText("Confirm")).not.toBeInTheDocument();
  });

  it("confirms as a named token-mode reviewer, and needs a note to reject", async () => {
    vi.spyOn(client, "api").mockImplementation(async (path: string) =>
      path.includes(`/items/${ID}`) ? detail : [concept()],
    );
    const post = vi
      .spyOn(client, "apiPost")
      .mockResolvedValue({ reviewer: "token:Ann", message: "ok" });
    renderAt("/w/ws1/semantics", writer);
    fireEvent.click(await screen.findByText("PartsNotObsolete"));
    const confirm = await screen.findByRole("button", { name: "Confirm" });
    expect(confirm).toBeDisabled(); // no reviewer name yet
    expect(screen.getByRole("button", { name: "Reject" })).toBeDisabled();

    fireEvent.change(screen.getByLabelText(/Reviewer/), { target: { value: "Ann" } });
    expect(screen.getByRole("button", { name: "Reject" })).toBeDisabled(); // no note
    fireEvent.click(confirm);
    await waitFor(() => expect(post).toHaveBeenCalled());
    expect(post).toHaveBeenCalledWith(
      `/workspaces/ws1/semantics/items/${ID}/review`,
      expect.objectContaining({ action: "confirm", reviewer: "Ann" }),
    );
    expect(await screen.findByText("Recorded as token:Ann.")).toBeInTheDocument();
  });
});

describe("bulk review", () => {
  it("confirms every selected item in one all-or-nothing call", async () => {
    const two = [concept(), concept({ definition: "parts.assembly IS TRUE" }, "5d04c82a-bbf0-5307-a0c3-f27beda79347")];
    two[1].name = "PartsAssembly";
    vi.spyOn(client, "api").mockResolvedValue(two);
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ count: 2, reviewer: "token:Ann" });
    renderAt("/w/ws1/semantics", writer);
    fireEvent.click(await screen.findByLabelText(/select all 2 shown/));
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    const confirm = screen.getByRole("button", { name: "Confirm 2" });
    expect(confirm).toBeDisabled(); // token mode: who?
    fireEvent.change(screen.getByLabelText("bulk reviewer"), { target: { value: "Ann" } });
    expect(screen.getByRole("button", { name: "Reject 2" })).toBeDisabled(); // no note
    fireEvent.click(confirm);
    await waitFor(() => expect(post).toHaveBeenCalled());
    const [url, body] = post.mock.calls[0] as [string, { ids: string[] }];
    expect(url).toBe("/workspaces/ws1/semantics/review-bulk");
    expect(body).toMatchObject({ action: "confirm", note: null, reviewer: "Ann" });
    expect([...body.ids].sort()).toEqual([ID, "5d04c82a-bbf0-5307-a0c3-f27beda79347"].sort());
    expect(await screen.findByText("2 item(s) confirmed — recorded as token:Ann.")).toBeInTheDocument();
    expect(screen.queryByText("2 selected")).not.toBeInTheDocument();
  });

  it("offers no selection to a reader", async () => {
    vi.spyOn(client, "api").mockResolvedValue([concept()]);
    renderAt("/w/ws1/semantics", reader);
    await screen.findByText("PartsNotObsolete");
    expect(screen.queryByLabelText(/select all/)).not.toBeInTheDocument();
  });
});

describe("LinkML editor", () => {
  it("validates edits into decisions before it can apply them", async () => {
    vi.spyOn(client, "api").mockResolvedValue({ empty: false, yaml: "name: t\n" });
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({
      decisions: [
        {
          id: ID,
          kind: "concept",
          path: "classes.ActivePart",
          action: "edit",
          changes: { name: ["PartsNotObsolete", "ActivePart"] },
          note: null,
        },
      ],
      warnings: [],
      errors: [],
      applied: 0,
    });
    renderAt("/w/ws1/semantics?view=linkml", { mode: "oidc", email: "ann@x.io", role: "write" });
    fireEvent.click(await screen.findByRole("button", { name: "YAML editor" }));
    const apply = await screen.findByRole("button", { name: /^Apply/ });
    expect(apply).toBeDisabled();
    const validate = screen.getByRole("button", { name: "Validate" });
    await waitFor(() => expect(validate).toBeEnabled()); // the YAML has loaded
    fireEvent.click(validate);
    expect(await screen.findByText("classes.ActivePart")).toBeInTheDocument();
    expect(post).toHaveBeenCalledWith("/workspaces/ws1/semantics/linkml/validate", {
      yaml: "name: t\n",
    });
    await waitFor(() => expect(screen.getByRole("button", { name: /^Apply/ })).toBeEnabled());
  });

  it("shows edits side by side against the current export", async () => {
    vi.spyOn(client, "api").mockResolvedValue({ empty: false, yaml: "name: t\nclasses:\n  A: {}\n" });
    renderAt("/w/ws1/semantics?view=linkml", { mode: "oidc", email: "ann@x.io", role: "write" });
    fireEvent.click(await screen.findByRole("button", { name: "YAML editor" }));
    const ta = await screen.findByDisplayValue(/name: t/);
    fireEvent.change(ta, { target: { value: "name: t\nclasses:\n  B: {}\n" } });
    fireEvent.click(screen.getByRole("button", { name: /Diff vs current export/ }));
    expect(await screen.findByText("1 changed block(s). Left: current export · right: your edits.")).toBeInTheDocument();
    expect(screen.getByText("A: {}")).toHaveClass("lm-old");
    expect(screen.getByText("B: {}")).toHaveClass("lm-new");
  });
});
