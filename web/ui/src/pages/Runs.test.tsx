import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import { Runs } from "./Runs";

function renderRuns() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/w/ws1/runs"]}>
        <Routes>
          <Route path="/w/:id/runs" element={<Runs />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Runs", () => {
  it("shows an empty-state message when there is no run history", async () => {
    vi.spyOn(client, "api").mockResolvedValue([]);
    renderRuns();
    expect(await screen.findByText("nothing run yet")).toBeInTheDocument();
  });

  it("lists each run with its command, status chip, and truncated timestamp", async () => {
    vi.spyOn(client, "api").mockResolvedValue([
      { id: "r1", command: "build", status: "succeeded", created_at: "2026-09-05T10:00:00.123Z" },
      { id: "r2", command: "test", status: "failed", created_at: "2026-09-05T11:00:00.000Z" },
    ]);

    renderRuns();

    expect(await screen.findByText("build")).toBeInTheDocument();
    expect(screen.getByText("test")).toBeInTheDocument();
    expect(screen.getByText("succeeded")).toHaveClass("chip", "ok");
    expect(screen.getByText("failed")).toHaveClass("chip", "bad");
    expect(screen.getByText("2026-09-05 10:00:00")).toBeInTheDocument();
  });

  it("maps every terminal status to the right chip class", async () => {
    vi.spyOn(client, "api").mockResolvedValue([
      { id: "1", command: "a", status: "cancelled", created_at: "2026-01-01T00:00:00Z" },
      { id: "2", command: "b", status: "timed_out", created_at: "2026-01-01T00:00:00Z" },
      { id: "3", command: "c", status: "running", created_at: "2026-01-01T00:00:00Z" },
    ]);

    renderRuns();

    expect(await screen.findByText("cancelled")).toHaveClass("chip", "warn");
    expect(screen.getByText("timed_out")).toHaveClass("chip", "bad");
    expect(screen.getByText("running")).toHaveClass("chip");
    expect(screen.getByText("running")).not.toHaveClass("ok", "bad", "warn");
  });

  it("fetches the run list scoped to the workspace id from the route", async () => {
    const apiSpy = vi.spyOn(client, "api").mockResolvedValue([]);
    renderRuns();
    await screen.findByText("nothing run yet");
    expect(apiSpy).toHaveBeenCalledWith("/runs?workspace=ws1&limit=100");
  });
});
