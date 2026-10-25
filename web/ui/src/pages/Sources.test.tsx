import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { DatabaseTest } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { Sources } from "./Sources";

const listing = {
  sources: [{ name: "idempiere", kind: "git", path: "/src/idempiere", exists: true, git: true }],
  databases: [
    {
      name: "erp",
      kind: "oracle",
      connect: "db:1521/XEPDB1",
      user: "ekos_ro",
      password_env: "EKOS_DB_DEFAULT_SHOP_ERP",
      password_stored: true,
      owners: ["ADEMPIERE"],
      profile: "p2",
      dictionary: "idempiere",
    },
  ],
};

const tested: DatabaseTest = {
  database: "erp",
  ok: true,
  ms: 6066,
  read_only: true,
  invalid_objects: 0,
  test: {
    label: "oracle:ekos_ro@db:1521/XEPDB1",
    family: "dba",
    owners: [{ owner: "ADEMPIERE", columns: 30457, census: { TABLE: { valid: 903, invalid: 0 } } }],
    any_privileges: [],
  },
  census_vs_compiled: [
    { what: "tables", census: 903, compiled: 903, equal: true },
    { what: "routines", census: 62, compiled: 61, equal: false },
  ],
};

function renderAs(me: Me) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/ws1/sources"]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={me} />}>
            <Route path="sources" element={<Sources />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const operator: Me = { mode: "oidc", email: "ann@x.io", role: "write" };
const reader: Me = { mode: "oidc", email: "bob@x.io", role: "read" };

afterEach(() => vi.restoreAllMocks());

describe("Sources page", () => {
  it("lists directories and databases; a reader gets no buttons", async () => {
    vi.spyOn(client, "api").mockResolvedValue(listing as never);
    renderAs(reader);
    expect(await screen.findByText("/src/idempiere")).toBeInTheDocument();
    expect(screen.getByText("password stored")).toBeInTheDocument();
    expect(screen.queryByText("Test connection")).toBeNull();
    expect(screen.queryByText("Add an Oracle database")).toBeNull();
  });

  it("adds a database and clears the password field once it is stored", async () => {
    vi.spyOn(client, "api").mockResolvedValue({ sources: [], databases: [] } as never);
    const post = vi
      .spyOn(client, "apiPost")
      .mockResolvedValue({ message: "Added database erp", password_env: "EKOS_DB_X" });
    renderAs(operator);
    fireEvent.click(await screen.findByText("Add an Oracle database"));
    fireEvent.change(screen.getByLabelText("database name"), { target: { value: "erp" } });
    fireEvent.change(screen.getByLabelText("connect"), { target: { value: "db:1521/XEPDB1" } });
    fireEvent.change(screen.getByLabelText("user"), { target: { value: "ekos_ro" } });
    fireEvent.change(screen.getByLabelText("password"), { target: { value: "pw" } });
    fireEvent.change(screen.getByLabelText("owners"), { target: { value: "ADEMPIERE, OTHER" } });
    fireEvent.change(screen.getByLabelText("profile"), { target: { value: "p2" } });
    fireEvent.click(screen.getByText("Add database"));
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(post.mock.calls[0]).toEqual([
      "/workspaces/ws1/databases",
      {
        name: "erp",
        connect: "db:1521/XEPDB1",
        user: "ekos_ro",
        password: "pw",
        owners: ["ADEMPIERE", "OTHER"],
        profile: "p2",
        dictionary: null,
      },
    ]);
    await screen.findByText("Added database erp");
    expect(screen.getByLabelText("password")).toHaveValue("");
  });

  it("tests a connection and shows the census next to what was compiled", async () => {
    vi.spyOn(client, "api").mockResolvedValue(listing as never);
    vi.spyOn(client, "apiPost").mockResolvedValue(tested);
    renderAs(operator);
    fireEvent.click(await screen.findByText("Test connection"));
    expect(await screen.findByText("writes refused (read-only)")).toBeInTheDocument();
    expect(screen.getByText("equal")).toBeInTheDocument();
    expect(screen.getByText("differs")).toBeInTheDocument();
    expect(screen.getByText(/30457 columns/)).toBeInTheDocument();
  });
});
