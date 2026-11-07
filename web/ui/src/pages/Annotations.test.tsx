import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { Annotation } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { Annotations } from "./Annotations";
import { annotationOrder, parseRule, reach, stateChip, targetLabel, validTarget } from "./annotations-shared";

const ID = "cc57a3dc-bd4c-507e-94fe-54f967a3594f";
const TARGET = "11111111-2222-4333-8444-555555555555";

const note = (over: Partial<Annotation> = {}): Annotation => ({
  id: ID,
  target: TARGET,
  kind: "comment",
  text: "amount is in cents",
  rule: null,
  term: null,
  author: "ann",
  scope: "personal",
  approved_by: null,
  state: "active",
  updated_at: "2026-11-07T10:00:00Z",
  ...over,
});

function renderAs(me: Me) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/ws1/annotations"]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={me} />}>
            <Route path="annotations" element={<Annotations />} />
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

describe("annotation helpers", () => {
  it("says in words what an annotation does for others", () => {
    expect(reach(note())).toBe("visible to everyone");
    expect(reach(note({ kind: "instruction" }))).toBe("applies to its author's requests only");
    expect(reach(note({ kind: "instruction", scope: "shared" }))).toBe("waiting for approval");
    expect(reach(note({ kind: "synonym", scope: "shared", approved_by: "bob" }))).toBe(
      "approved by bob: applies to everyone",
    );
    expect(reach(note({ state: "stale" }))).toContain("confirm it again");
    expect(reach(note({ state: "withdrawn" }))).toBe("withdrawn");
    expect(stateChip("stale")).toBe("chip warn");
  });

  it("puts what waits for a person first and withdrawn last", () => {
    const items = [
      note({ id: "w", state: "withdrawn" }),
      note({ id: "ok", kind: "comment" }),
      note({ id: "wait", kind: "instruction", scope: "shared" }),
      note({ id: "stale", state: "stale" }),
    ];
    expect(annotationOrder(items).map((a) => a.id)).toEqual(["stale", "wait", "ok", "w"]);
  });

  it("accepts only an entity id or the project as a target", () => {
    expect(validTarget("project")).toBe(true);
    expect(validTarget(TARGET)).toBe(true);
    expect(validTarget(" " + TARGET + " ")).toBe(true);
    for (const bad of ["sales", "", "--all", TARGET + "x"]) expect(validTarget(bad)).toBe(false);
    expect(targetLabel("project")).toBe("project");
    expect(targetLabel(TARGET)).toBe("11111111");
  });

  it("reads the optional rule box", () => {
    expect(parseRule("  ")).toEqual({ rule: null });
    expect(parseRule('{"rule":"fiscal_year_start","month":4}').rule).toEqual({
      rule: "fiscal_year_start",
      month: 4,
    });
    for (const bad of ["{", "[]", "null", '{"month":4}', '"x"']) {
      expect(parseRule(bad).error).toBeTruthy();
    }
  });
});

describe("Annotations page", () => {
  it("lists annotations; a reader cannot write", async () => {
    const api = vi.spyOn(client, "api").mockResolvedValue([note()]);
    renderAs(reader);
    expect(await screen.findByText("amount is in cents")).toBeInTheDocument();
    expect(api.mock.calls[0][0]).toBe("/workspaces/ws1/annotations");
    expect(screen.queryByLabelText("add annotation")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "11111111" }));
    expect(await screen.findByText("visible to everyone")).toBeInTheDocument();
    expect(screen.queryByText("Withdraw")).toBeNull();
  });

  it("shows a person's markup as text, not as markup", async () => {
    vi.spyOn(client, "api").mockResolvedValue([note({ text: "<img src=x onerror=alert(1)> **b**" })]);
    const { container } = renderAs(reader);
    expect(await screen.findByText("<img src=x onerror=alert(1)> **b**")).toBeInTheDocument();
    expect(container.querySelector("img")).toBeNull();
  });

  it("adds an annotation: needs a valid target, a name in token mode, and sends the rule", async () => {
    vi.spyOn(client, "api").mockResolvedValue([]);
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ author: "token:Ann" });
    renderAs(writer);
    const add = await screen.findByRole("button", { name: "Add" });
    expect(add).toBeDisabled();

    fireEvent.change(screen.getByLabelText("target"), { target: { value: "sales" } });
    fireEvent.change(screen.getByLabelText("text"), { target: { value: "FY starts in April" } });
    fireEvent.change(screen.getByLabelText("author"), { target: { value: "Ann" } });
    expect(add).toBeDisabled(); // "sales" is a name, not an id

    fireEvent.change(screen.getByLabelText("target"), { target: { value: "project" } });
    fireEvent.change(screen.getByLabelText("kind"), { target: { value: "instruction" } });
    fireEvent.change(screen.getByLabelText("rule"), { target: { value: "{" } });
    expect(add).toBeDisabled();
    expect(screen.getByText("the rule is not valid JSON")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("rule"), {
      target: { value: '{"rule":"fiscal_year_start","month":4}' },
    });
    fireEvent.click(screen.getByLabelText("shared"));
    expect(add).toBeEnabled();
    fireEvent.click(add);
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(post.mock.calls[0]).toEqual([
      "/workspaces/ws1/annotations",
      {
        target: "project",
        kind: "instruction",
        text: "FY starts in April",
        rule: { rule: "fiscal_year_start", month: 4 },
        term: null,
        shared: true,
        reviewer: "Ann",
      },
    ]);
    expect(await screen.findByText("Recorded as token:Ann.")).toBeInTheDocument();
  });

  it("a synonym needs its word", async () => {
    vi.spyOn(client, "api").mockResolvedValue([]);
    renderAs(writer);
    await screen.findByRole("button", { name: "Add" });
    fireEvent.change(screen.getByLabelText("target"), { target: { value: TARGET } });
    fireEvent.change(screen.getByLabelText("kind"), { target: { value: "synonym" } });
    fireEvent.change(screen.getByLabelText("text"), { target: { value: "turnover means invoices" } });
    fireEvent.change(screen.getByLabelText("author"), { target: { value: "Ann" } });
    expect(screen.getByRole("button", { name: "Add" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("term"), { target: { value: "turnover" } });
    expect(screen.getByRole("button", { name: "Add" })).toBeEnabled();
  });

  it("offers approve for a waiting shared instruction and confirm for a stale one", async () => {
    vi.spyOn(client, "api").mockResolvedValue([
      note({ kind: "instruction", scope: "shared" }),
      note({ id: "11111111-aaaa-4bbb-8ccc-222222222222", state: "stale", text: "old" }),
    ]);
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ author: "token:Bob" });
    renderAs(writer);
    fireEvent.click((await screen.findAllByRole("button", { name: "11111111" }))[0]);
    // The stale one sorts first: confirm, not approve.
    expect(await screen.findByText("Confirm against the entity as it is now")).toBeInTheDocument();
    expect(screen.queryByText("Approve for everyone")).toBeNull();
    fireEvent.change(screen.getByLabelText("reviewer"), { target: { value: "Bob" } });
    fireEvent.click(screen.getByText("Confirm against the entity as it is now"));
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(post.mock.calls[0]).toEqual([
      "/workspaces/ws1/annotations/11111111-aaaa-4bbb-8ccc-222222222222/confirm",
      { reviewer: "Bob" },
    ]);
  });
});
