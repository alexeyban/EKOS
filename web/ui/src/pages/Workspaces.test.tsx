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
  it("shows the health payload once loaded", async () => {
    stubWorkspaces([]);
    renderWorkspaces();
    expect(await screen.findByText(/ekos-console-api/)).toBeInTheDocument();
    expect(screen.getByText("v1.2.3")).toBeInTheDocument();
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
    expect(await screen.findByText("none registered yet")).toBeInTheDocument();
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

  it("shows a failed-with-retries chip and no-server chip appropriately", async () => {
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
    expect(screen.getByText("no server")).toBeInTheDocument();
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
    await screen.findByText("none registered yet");

    await user.type(screen.getByPlaceholderText("id"), "w1");
    await user.type(screen.getByPlaceholderText("name"), "Main");
    await user.type(screen.getByPlaceholderText("/abs/path/to/workspace"), "/repo");
    await user.click(screen.getByRole("button", { name: "Add" }));

    expect(postSpy).toHaveBeenCalledWith("/workspaces", { id: "w1", name: "Main", path: "/repo" });
    expect(await screen.findByPlaceholderText("id")).toHaveValue("");
  });

  it("shows the server's rejection (e.g. outside the configured workspaces root) on failure", async () => {
    stubWorkspaces([]);
    vi.spyOn(client, "apiPost").mockRejectedValue(
      new client.ApiError(400, "/etc is outside the configured workspaces root /repos"),
    );
    const user = userEvent.setup();

    renderWorkspaces();
    await screen.findByText("none registered yet");

    await user.type(screen.getByPlaceholderText("id"), "x");
    await user.type(screen.getByPlaceholderText("name"), "X");
    await user.type(screen.getByPlaceholderText("/abs/path/to/workspace"), "/etc");
    await user.click(screen.getByRole("button", { name: "Add" }));

    expect(await screen.findByText(/outside the configured workspaces root/)).toBeInTheDocument();
    // The form is not cleared on failure — the user can fix and resubmit.
    expect(screen.getByPlaceholderText("id")).toHaveValue("x");
  });
});
