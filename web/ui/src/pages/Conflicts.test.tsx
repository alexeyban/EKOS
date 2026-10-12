import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { Conflict } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { Conflicts } from "./Conflicts";
import { claimValue, claimWhere, conflictOrder, disputedIds } from "./conflicts-shared";

const conflict = (over: Partial<Conflict> = {}): Conflict => ({
  id: "cc57a3dc-bd4c-507e-94fe-54f967a3594f",
  name: "user_preference.columns",
  subject: { id: "s-1", name: "user_preference", kind: "Table" },
  attribute: "columns",
  conflict_type: "duplicate_definition",
  status: "open",
  status_meaning: "unresolved: sources disagree and nobody has decided",
  claims: [
    { value: ["id", "language"], path: "sql/Pg-database.sql", line: 40, source: "sql" },
    { value: ["id", "user_id"], path: "sql/changes/1.9/prefs.sql", line: null, source: "sql" },
  ],
  chosen_by_ekos: ["id", "user_id"],
  picked_claim: null,
  review_note: null,
  measured: null,
  ...over,
});

function renderAs(me: Me) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/ws1/conflicts"]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={me} />}>
            <Route path="conflicts" element={<Conflicts />} />
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

describe("conflict helpers", () => {
  it("prints values and locations readably", () => {
    expect(claimValue(["id", "language"])).toBe("id, language");
    expect(claimValue(null)).toBe("—");
    expect(claimValue(true)).toBe("true");
    expect(claimWhere(conflict().claims![0])).toBe("sql/Pg-database.sql:40");
    expect(claimWhere(conflict().claims![1])).toBe("sql/changes/1.9/prefs.sql");
  });

  it("puts open conflicts first and rings only their subjects", () => {
    const items = [
      conflict({ id: "a", name: "b.x", status: "dismissed", subject: { id: "s-2", name: "b", kind: "Table" } }),
      conflict({ id: "b", name: "z.x" }),
      conflict({ id: "c", name: "a.x" }),
    ];
    expect(conflictOrder(items).map((c) => c.id)).toEqual(["c", "b", "a"]);
    expect([...disputedIds(items)]).toEqual(["s-1"]);
    expect(disputedIds(undefined).size).toBe(0);
  });
});

describe("Conflicts view", () => {
  it("opens a conflict from the keyboard: the row's name is a real button", async () => {
    vi.spyOn(client, "api").mockResolvedValue([conflict()]);
    renderAs(reader);
    const pick = await screen.findByRole("button", { name: "user_preference.columns" });
    expect(pick).toHaveAttribute("aria-pressed", "false");
    pick.focus();
    expect(pick).toHaveFocus();
    fireEvent.click(pick);
    expect(await screen.findByText("sql/Pg-database.sql:40 · sql")).toBeInTheDocument();
    expect(pick).toHaveAttribute("aria-pressed", "true");
  });

  it("lists open conflicts and shows both claims with where each comes from", async () => {
    const api = vi.spyOn(client, "api").mockResolvedValue([conflict()]);
    renderAs(reader);
    fireEvent.click(await screen.findByText("user_preference.columns"));
    expect(api.mock.calls[0][0]).toBe("/workspaces/ws1/conflicts?status=open");
    expect(await screen.findByText("sql/Pg-database.sql:40 · sql")).toBeInTheDocument();
    expect(screen.getByText("sql/changes/1.9/prefs.sql · sql")).toBeInTheDocument();
    // A reader sees the claims but cannot decide.
    expect(screen.getByText("Deciding needs the write role.")).toBeInTheDocument();
    expect(screen.queryByText("Claim 1 is right")).toBeNull();
  });

  it("sends a pick with the reviewer, and needs a note to say both are valid", async () => {
    vi.spyOn(client, "api").mockResolvedValue([conflict()]);
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ reviewer: "token:Ann" });
    renderAs(writer);
    fireEvent.click(await screen.findByText("user_preference.columns"));

    // Token mode has no identity: nothing can be decided before a name is typed.
    expect(await screen.findByText("Claim 2 is right")).toBeDisabled();
    fireEvent.change(screen.getByLabelText("reviewer"), { target: { value: "Ann" } });
    expect(screen.getByText("Both are valid")).toBeDisabled();
    fireEvent.click(screen.getByText("Claim 2 is right"));
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(post.mock.calls[0]).toEqual([
      "/workspaces/ws1/conflicts/cc57a3dc-bd4c-507e-94fe-54f967a3594f/resolve",
      { pick: 2, note: null, reviewer: "Ann" },
    ]);
    expect(await screen.findByText("Recorded as token:Ann.")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("note"), { target: { value: "two facets" } });
    fireEvent.click(screen.getByText("Both are valid"));
    await waitFor(() => expect(post).toHaveBeenCalledTimes(2));
    expect(post.mock.calls[1][1]).toEqual({ both_valid: true, note: "two facets", reviewer: "Ann" });
  });

  it("says so when nothing is open", async () => {
    vi.spyOn(client, "api").mockResolvedValue([]);
    renderAs(reader);
    expect(await screen.findByText(/No open conflicts/)).toBeInTheDocument();
  });
});
