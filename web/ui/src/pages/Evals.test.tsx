import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import { Evals } from "./Evals";

function renderEvals() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/w/ws1/evals"]}>
        <Routes>
          <Route path="/w/:id/evals" element={<Evals />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
});

const report = (overrides: Partial<Record<string, unknown>> = {}) => ({
  file: "20260905T150000Z-architecture.json",
  dataset: "architecture",
  agent: "ollama (llama3:latest)",
  runtime: "local",
  generated_at: "2026-09-05T15:00:00.123Z",
  status_pass: true,
  scenarios: 20,
  passed: 17,
  failed: 3,
  answer_correctness: 0.85,
  evidence_groundedness: 0.9,
  completeness: 0.8,
  recall_at_10: null,
  hallucination_rate: 0.05,
  avg_tokens: 1221,
  p95_latency_ms: 49400,
  cache_hits: 2,
  cache_misses: 18,
  tokens_saved: 2442,
  peak_rss_kb: 71372,
  total_cpu_time_ms: 14700,
  ...overrides,
});

describe("Evals", () => {
  it("shows an empty-state message when no reports have been saved", async () => {
    vi.spyOn(client, "api").mockResolvedValue([]);
    renderEvals();
    expect(await screen.findByText("no eval runs saved yet")).toBeInTheDocument();
  });

  it("lists a saved report with its dataset, agent, status chip, and pass counts", async () => {
    vi.spyOn(client, "api").mockResolvedValue([report()]);
    renderEvals();

    expect(await screen.findByText("architecture")).toBeInTheDocument();
    expect(screen.getByText("ollama (llama3:latest)")).toBeInTheDocument();
    expect(screen.getByText("PASS")).toHaveClass("chip", "ok");
    expect(screen.getByText("17/20 passed")).toBeInTheDocument();
    expect(screen.getByText("answer 85.0%")).toBeInTheDocument();
    expect(screen.getByText("halluc 5.0%")).toBeInTheDocument();
  });

  it("shows a FAIL chip for a report whose gate didn't pass", async () => {
    vi.spyOn(client, "api").mockResolvedValue([report({ status_pass: false })]);
    renderEvals();
    expect(await screen.findByText("FAIL")).toHaveClass("chip", "bad");
  });

  it("links each row to its detail page by the report's own filename", async () => {
    vi.spyOn(client, "api").mockResolvedValue([report()]);
    renderEvals();
    const link = await screen.findByRole("link", { name: "architecture" });
    expect(link).toHaveAttribute(
      "href",
      "/w/ws1/evals/20260905T150000Z-architecture.json",
    );
  });

  it("fetches the report list scoped to the workspace id from the route", async () => {
    const apiSpy = vi.spyOn(client, "api").mockResolvedValue([]);
    renderEvals();
    await screen.findByText("no eval runs saved yet");
    expect(apiSpy).toHaveBeenCalledWith("/workspaces/ws1/evals/reports");
  });

  it("renders newest report first", async () => {
    vi.spyOn(client, "api").mockResolvedValue([
      report({ file: "older.json", dataset: "older-run", generated_at: "2026-09-05T14:00:00Z" }),
      report({ file: "newer.json", dataset: "newer-run", generated_at: "2026-09-05T15:00:00Z" }),
    ]);
    renderEvals();
    await screen.findByText("newer-run");
    const links = screen
      .getAllByRole("link")
      .filter((l) => l.getAttribute("href")?.includes("/evals/"));
    expect(links.map((l) => l.textContent)).toEqual(["newer-run", "older-run"]);
  });
});
