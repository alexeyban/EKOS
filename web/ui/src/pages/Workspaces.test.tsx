import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { Workspace } from "../api/types";
import { Workspaces } from "./Workspaces";

function renderWorkspaces() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <Workspaces />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function stubWorkspaces(workspaces: Workspace[]) {
  vi.spyOn(client, "api").mockImplementation((path: string) => {
    if (path === "/health") {
      return Promise.resolve({ status: "ok", service: "ekos-console-api", version: "1.2.3" });
    }
    if (path === "/workspaces") return Promise.resolve(workspaces);
    throw new Error(`unexpected path ${path}`);
  });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Workspaces health banner", () => {
  it("shows no banner while the API answers (the status bar carries the version)", async () => {
    stubWorkspaces([]);
    renderWorkspaces();
    expect(await screen.findByText(/none registered yet/)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows an unreachable message when the health check fails", async () => {
    vi.spyOn(client, "api").mockImplementation((path: string) => {
      if (path === "/health") return Promise.reject(new Error("network down"));
      return Promise.resolve([]);
    });
    renderWorkspaces();
    expect(await screen.findByText(/unreachable/)).toBeInTheDocument();
  });
});

describe("Workspaces list", () => {
  it("shows an empty-state message when none are registered", async () => {
    stubWorkspaces([]);
    renderWorkspaces();
    expect(await screen.findByText(/none registered yet/)).toBeInTheDocument();
  });

  it("lists each workspace with its path and a ready server chip", async () => {
    stubWorkspaces([
      {
        id: "w1",
        name: "Main",
        path: "/repo",
        server: { state: "ready", port: 7400, retries: 0, detail: "" },
      },
    ]);
    renderWorkspaces();

    expect(await screen.findByText("Main")).toBeInTheDocument();
    expect(screen.getByText("/repo")).toBeInTheDocument();
    expect(screen.getByText("ready")).toHaveClass("chip", "ok");
  });

  it("shows a failed-with-retries chip and an idle chip appropriately", async () => {
    stubWorkspaces([
      {
        id: "w1",
        name: "Failing",
        path: "/a",
        server: { state: "failed", port: 0, retries: 3, detail: "boom" },
      },
      { id: "w2", name: "NoServer", path: "/b", server: null },
    ]);
    renderWorkspaces();

    expect(await screen.findByText("failed (3 retries)")).toHaveClass("chip", "bad");
    expect(screen.getByText("idle")).toHaveAttribute("title", "its server starts when first needed");
  });

  it("calls the delete endpoint when remove is clicked", async () => {
    stubWorkspaces([{ id: "w1", name: "Main", path: "/repo", server: null }]);
    const deleteSpy = vi.spyOn(client, "apiDelete").mockResolvedValue(undefined);
    const user = userEvent.setup();

    renderWorkspaces();
    await user.click(await screen.findByRole("button", { name: "remove" }));

    expect(deleteSpy).toHaveBeenCalledWith("/workspaces/w1");
  });
});

describe("RegisterForm", () => {
  it("submits the entered id/name/path and clears the form on success", async () => {
    stubWorkspaces([]);
    const postSpy = vi.spyOn(client, "apiPost").mockResolvedValue({});
    const user = userEvent.setup();

    renderWorkspaces();
    await screen.findByText(/none registered yet/);

    await user.click(screen.getByLabelText("Existing workspace"));
    await user.type(screen.getByPlaceholderText("id"), "w1");
    await user.type(screen.getByPlaceholderText("name"), "Main");
    await user.type(screen.getByPlaceholderText("/abs/path/to/workspace"), "/repo");
    await user.click(screen.getByRole("button", { name: "Add" }));

    expect(postSpy).toHaveBeenCalledWith("/workspaces", { id: "w1", name: "Main", path: "/repo" });
    expect(await screen.findByPlaceholderText("id")).toHaveValue("");
  });

  it("creates a project from a source directory by default", async () => {
    stubWorkspaces([]);
    const postSpy = vi.spyOn(client, "apiPost").mockResolvedValue({});
    const user = userEvent.setup();

    renderWorkspaces();
    await screen.findByText(/none registered yet/);

    await user.type(screen.getByPlaceholderText("id"), "shop");
    await user.type(screen.getByPlaceholderText("name"), "Shop");
    await user.type(screen.getByPlaceholderText("/abs/path/to/sources"), "/sources/shop");
    await user.click(screen.getByRole("button", { name: "Add" }));

    expect(postSpy).toHaveBeenCalledWith("/workspaces", {
      id: "shop",
      name: "Shop",
      source: "/sources/shop",
    });
  });

  it("shows the server's rejection (e.g. outside the configured workspaces root) on failure", async () => {
    stubWorkspaces([]);
    vi.spyOn(client, "apiPost").mockRejectedValue(
      new client.ApiError(400, "/etc is outside the configured workspaces root /repos"),
    );
    const user = userEvent.setup();

    renderWorkspaces();
    await screen.findByText(/none registered yet/);

    await user.type(screen.getByPlaceholderText("id"), "x");
    await user.type(screen.getByPlaceholderText("name"), "X");
    await user.type(screen.getByPlaceholderText("/abs/path/to/sources"), "/etc");
    await user.click(screen.getByRole("button", { name: "Add" }));

    expect(await screen.findByText(/outside the configured workspaces root/)).toBeInTheDocument();
    // The form is not cleared on failure — the user can fix and resubmit.
    expect(screen.getByPlaceholderText("id")).toHaveValue("x");
  });
});

describe("next step line", () => {
  it("names the first step not done, and the review as waiting for you", async () => {
    const { nextStepLine } = await import("./Workspaces");
    const step = (n: number, id: string, title: string, state: string) =>
      ({ n, id, title, state, detail: "" }) as never;
    expect(
      nextStepLine([step(1, "analyse", "Analyse sources", "done"), step(2, "docs", "Documentation", "done"),
        step(3, "review", "Review", "todo"), step(4, "req", "Requirements", "todo")]),
    ).toBe("Step 3 of 4: the review is waiting for you");
    expect(nextStepLine([step(1, "analyse", "Analyse sources", "done")])).toBe("Every step is done");
    expect(nextStepLine(undefined)).toBeNull();
  });
});
