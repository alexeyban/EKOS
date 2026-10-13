import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createMemoryRouter, Outlet, RouterProvider } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import { Admin } from "./Admin";

function renderAdmin(me: { owner?: boolean }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createMemoryRouter([
    { path: "/", element: <Outlet context={me} />, children: [{ index: true, element: <Admin /> }] },
  ]);
  return render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

const BINDINGS = [
  { id: "b1", principal_kind: "user", principal: "vic@acme.io", role: "viewer", workspace_id: null },
  { id: "b2", principal_kind: "group", principal: "ops", role: "operator", workspace_id: "acme.api" },
];

function stub(tenants: unknown[], extra: Record<string, unknown> = {}) {
  const data: Record<string, unknown> = {
    "/tenants": tenants,
    "/workspaces": [{ id: "acme.api", name: "API", path: "/a", tenant_id: "acme", server: null }],
    "/tenants/acme/bindings": BINDINGS,
    "/tenants/acme/groups": [],
    "/tenants/acme/tokens": [],
    "/tenants/acme/audit?limit=100": [
      { at: "2026-10-13T10:00:00", actor: "vic@acme.io", action: "operate: POST /x", workspace_id: "acme.api", outcome: "denied" },
    ],
    ...extra,
  };
  vi.spyOn(client, "api").mockImplementation((path: string) => {
    if (path in data) return Promise.resolve(data[path]);
    return Promise.reject(new Error(`unexpected path ${path}`));
  });
}

const ACME = { id: "acme", name: "Acme", roles: ["admin"], source_roots: ["/sources/acme"] };

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Admin", () => {
  it("says so when the caller administers no tenant", async () => {
    stub([{ id: "acme", name: "Acme", roles: ["viewer"] }]);
    renderAdmin({});
    expect(await screen.findByText("You do not administer any tenant.")).toBeInTheDocument();
    expect(screen.queryByText("Roles")).not.toBeInTheDocument();
  });

  it("lists who holds which role, and on what", async () => {
    stub([ACME]);
    renderAdmin({});
    const row = (await screen.findByText("vic@acme.io", { selector: "td" })).closest("tr")!;
    expect(within(row).getByText("viewer")).toBeInTheDocument();
    expect(within(row).getByText("whole tenant")).toBeInTheDocument();
    const group = screen.getByText("ops").closest("tr")!;
    expect(within(group).getByText("group")).toHaveClass("chip");
    expect(within(group).getByText("acme.api")).toBeInTheDocument();
  });

  it("grants a role by e-mail on one workspace", async () => {
    stub([ACME]);
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({});
    const user = userEvent.setup();
    renderAdmin({});

    await user.type(await screen.findByLabelText("e-mail"), "new@acme.io");
    await user.selectOptions(screen.getByLabelText("role"), "operator");
    const scope = screen.getAllByLabelText("scope")[0];
    await within(scope).findByText("API");
    await user.selectOptions(scope, "acme.api");
    await user.click(screen.getByRole("button", { name: "Grant" }));

    expect(post).toHaveBeenCalledWith("/tenants/acme/bindings", {
      email: "new@acme.io",
      role: "operator",
      workspace_id: "acme.api",
    });
  });

  it("limits a viewer to part of one workspace", async () => {
    stub([ACME]);
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({});
    const user = userEvent.setup();
    renderAdmin({});

    await user.type(await screen.findByLabelText("e-mail"), "pia@partner.io");
    // No limit is offered until the grant is a viewer of one workspace.
    expect(screen.queryByLabelText("limit")).not.toBeInTheDocument();
    const scope = screen.getAllByLabelText("scope")[0];
    await within(scope).findByText("API");
    await user.selectOptions(scope, "acme.api");
    await user.selectOptions(screen.getByLabelText("limit"), "deny");
    // A limit that names nothing cannot be granted.
    expect(screen.getByRole("button", { name: "Grant" })).toBeDisabled();
    await user.type(screen.getByLabelText("source"), "payments");
    await user.type(screen.getByLabelText("path"), "secrets/**");
    await user.click(screen.getByRole("button", { name: "Grant" }));

    expect(post).toHaveBeenCalledWith("/tenants/acme/bindings", {
      email: "pia@partner.io",
      role: "viewer",
      workspace_id: "acme.api",
      effect: "deny",
      source: "payments",
      path: "secrets/**",
    });
  });

  it("shows a binding's rule in words", async () => {
    stub([ACME], {
      "/tenants/acme/bindings": [
        { id: "b3", principal_kind: "user", principal: "pia@partner.io", role: "viewer",
          workspace_id: "acme.api", effect: "deny", source: "payments", path: "secrets/**" },
        { id: "b4", principal_kind: "user", principal: "sam@partner.io", role: "viewer",
          workspace_id: "acme.api", effect: "allow", source: "docs", path: null },
      ],
    });
    renderAdmin({});
    expect(
      await screen.findByText("except source payments, path secrets/**"),
    ).toBeInTheDocument();
    expect(screen.getByText("only source docs")).toBeInTheDocument();
  });

  it("offers no workspace scope for admin, which is a tenant role", async () => {
    stub([ACME]);
    const user = userEvent.setup();
    renderAdmin({});
    await user.selectOptions(await screen.findByLabelText("role"), "admin");
    // Only the token form's scope select is left.
    expect(screen.getAllByLabelText("scope")).toHaveLength(1);
  });

  it("removes a binding", async () => {
    stub([ACME]);
    const del = vi.spyOn(client, "apiDelete").mockResolvedValue(undefined);
    const user = userEvent.setup();
    renderAdmin({});
    await user.click(await screen.findByRole("button", { name: "remove viewer from vic@acme.io" }));
    expect(del).toHaveBeenCalledWith("/tenants/acme/bindings/b1");
  });

  it("shows a new token once, and offers only viewer and operator", async () => {
    stub([ACME]);
    vi.spyOn(client, "apiPost").mockResolvedValue({
      id: "t1",
      name: "ci",
      prefix: "ekos_abc123",
      roles: [],
      expires_at: null,
      last_used_at: null,
      revoked: false,
      token: "ekos_abc123-the-secret-part",
    });
    const user = userEvent.setup();
    renderAdmin({});

    const role = await screen.findByLabelText("token role");
    expect(within(role).getAllByRole("option").map((o) => o.textContent)).toEqual([
      "viewer",
      "operator",
    ]);
    await user.type(screen.getByLabelText("token name"), "ci");
    await user.click(screen.getByRole("button", { name: "Create token" }));

    expect(await screen.findByText("ekos_abc123-the-secret-part")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "I have copied it" }));
    expect(screen.queryByText("ekos_abc123-the-secret-part")).not.toBeInTheDocument();
  });

  it("shows the audit log with denied requests marked", async () => {
    stub([ACME]);
    renderAdmin({});
    const denied = await screen.findByText("denied");
    expect(denied).toHaveClass("chip", "bad");
    expect(screen.getByText("operate: POST /x")).toBeInTheDocument();
  });

  it("keeps tenant creation and source roots for a platform owner", async () => {
    stub([ACME]);
    renderAdmin({});
    await screen.findByText("Roles");
    expect(screen.queryByRole("button", { name: "Create tenant" })).not.toBeInTheDocument();
  });

  it("lets an owner create a tenant and edit source roots", async () => {
    stub([ACME]);
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({});
    const patch = vi.spyOn(client, "apiPatch").mockResolvedValue({});
    const user = userEvent.setup();
    renderAdmin({ owner: true });

    await user.type(await screen.findByLabelText("tenant id"), "globex");
    await user.click(screen.getByRole("button", { name: "Create tenant" }));
    expect(post).toHaveBeenCalledWith("/tenants", { id: "globex", name: "globex" });

    const roots = screen.getByLabelText("Source roots of Acme");
    expect(roots).toHaveValue("/sources/acme");
    await user.type(roots, "\n/sources/shared");
    await user.click(screen.getByRole("button", { name: "Save source roots" }));
    expect(patch).toHaveBeenCalledWith("/tenants/acme", {
      source_roots: ["/sources/acme", "/sources/shared"],
    });
  });
});
