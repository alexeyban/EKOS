import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import { EvalDetail } from "./EvalDetail";

function renderDetail() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={["/w/ws1/evals/report1.json"]}>
        <Routes>
          <Route path="/w/:id/evals/:file" element={<EvalDetail />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
});

const fullReport = {
  dataset: "architecture",
  agent: "ollama (llama3:latest)",
  runtime: "local",
  generated_at: "2026-09-05T16:51:01.790741091Z",
  gates: {
    min_answer_correctness: 0.85,
    min_evidence_groundedness: 0.9,
    min_completeness: 0.8,
    min_recall_at_10: 0.8,
    max_hallucination_rate: 0.05,
  },
  metrics: {
    scenarios: 3,
    passed: 1,
    failed: 2,
    answer_correctness: 0.5,
    evidence_groundedness: 1.0,
    completeness: 0.5,
    recall_at_10: null,
    hallucination_rate: 0.0,
    avg_tokens: 1221,
    p95_latency_ms: 49412.9,
    cache_hits: 0,
    cache_misses: 3,
    tokens_saved: null,
    peak_rss_kb: 71324,
    total_cpu_time_ms: 14710,
    status_pass: false,
  },
  scenarios: [
    {
      id: "arch-001",
      passed: true,
      hallucinated: false,
      answer_score: 1.0,
      evidence_score: null,
      completeness_score: 1.0,
      retrieval_recall: null,
      groundedness_score: null,
      trajectory_score: null,
      input_tokens: 1136,
      output_tokens: 71,
      cache_hit: false,
      rss_kb_end: 70964,
      cpu_time_ms: 7870,
      latency_ms: 17687.6,
      error: null,
    },
    {
      id: "arch-002",
      passed: false,
      hallucinated: true,
      answer_score: 0.0,
      evidence_score: null,
      completeness_score: 0.0,
      retrieval_recall: null,
      groundedness_score: 0.0,
      trajectory_score: null,
      input_tokens: 900,
      output_tokens: 40,
      cache_hit: false,
      rss_kb_end: 71000,
      cpu_time_ms: 6000,
      latency_ms: 4300,
      error: null,
    },
    {
      id: "arch-003",
      passed: false,
      hallucinated: false,
      answer_score: null,
      evidence_score: null,
      completeness_score: null,
      retrieval_recall: null,
      groundedness_score: null,
      trajectory_score: null,
      input_tokens: null,
      output_tokens: null,
      cache_hit: null,
      rss_kb_end: null,
      cpu_time_ms: null,
      latency_ms: 12,
      error: "llm request timed out",
    },
  ],
};

describe("EvalDetail", () => {
  it("shows a loading state before the report arrives", () => {
    vi.spyOn(client, "api").mockReturnValue(new Promise(() => {}));
    renderDetail();
    expect(screen.getByText("loading…")).toBeInTheDocument();
  });

  it("renders the dataset header with a FAIL chip when the gate didn't pass", async () => {
    vi.spyOn(client, "api").mockResolvedValue(fullReport);
    renderDetail();
    expect(await screen.findByText("architecture")).toBeInTheDocument();
    expect(screen.getByText("FAIL")).toHaveClass("chip", "bad");
    expect(screen.getByText(/1\/3 scenarios passed/)).toBeInTheDocument();
  });

  it("renders all eleven headline metric tiles with formatted values", async () => {
    vi.spyOn(client, "api").mockResolvedValue(fullReport);
    renderDetail();
    await screen.findByText("architecture");

    expect(screen.getByText("answer correctness").previousSibling).toHaveTextContent("50.0%");
    expect(screen.getByText("evidence groundedness").previousSibling).toHaveTextContent(
      "100.0%",
    );
    expect(screen.getByText("recall@10").previousSibling).toHaveTextContent("n/a");
    expect(screen.getByText("avg tokens").previousSibling).toHaveTextContent("1,221");
    expect(screen.getByText("p95 latency").previousSibling).toHaveTextContent("49.4s");
    expect(screen.getByText("cache hits").previousSibling).toHaveTextContent("0/3");
    expect(screen.getByText("tokens saved").previousSibling).toHaveTextContent("n/a");
    expect(screen.getByText("peak RSS").previousSibling).toHaveTextContent("69.7 MB");
    expect(screen.getByText("CPU time").previousSibling).toHaveTextContent("14.7s");
  });

  it("lists each scenario with pass/fail and hallucinated chips", async () => {
    vi.spyOn(client, "api").mockResolvedValue(fullReport);
    renderDetail();
    await screen.findByText("architecture");

    expect(screen.getByText("arch-001")).toBeInTheDocument();
    expect(screen.getByText("pass")).toHaveClass("chip", "ok");
    expect(screen.getAllByText("fail")).toHaveLength(2);
    expect(screen.getByText("hallucinated")).toHaveClass("chip", "bad");
  });

  it("shows a scenario's error inline instead of its latency", async () => {
    vi.spyOn(client, "api").mockResolvedValue(fullReport);
    renderDetail();
    expect(await screen.findByText("llm request timed out")).toBeInTheDocument();
  });

  it("fetches the report scoped to workspace id and filename from the route", async () => {
    const apiSpy = vi.spyOn(client, "api").mockResolvedValue(fullReport);
    renderDetail();
    await screen.findByText("architecture");
    expect(apiSpy).toHaveBeenCalledWith("/workspaces/ws1/evals/reports/report1.json");
  });

  it("shows the API error message when the fetch fails", async () => {
    vi.spyOn(client, "api").mockRejectedValue(new Error("no such report"));
    renderDetail();
    expect(await screen.findByText("Error: no such report")).toBeInTheDocument();
  });
});
