import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { ReviewItem, SubjectStatus, WorkflowStep } from "../api/types";
import { StepBar } from "../StepBar";
import type { Me } from "../WorkspaceShell";
import { Review } from "./Review";
import { decisionLine, factsLine, signLine, stepChip, visibleItems } from "./review-shared";

const DIGEST = "7cb1be557988dc697ca0aa5ca46ec31eadac0f2882ad874ca4c5a41a610a8370";

const status = (over: Partial<SubjectStatus> = {}): SubjectStatus => ({
  subject: "scope",
  items: 2,
  decided: 0,
  outdated: 0,
  blocking: 0,
  digest: DIGEST,
  signoff: { state: "none" },
  ...over,
});

const item = (over: Partial<ReviewItem> = {}): ReviewItem => ({
  id: "table:t_report",
  label: "T_REPORT",
  facts: { table: "t_report", class: "transient" },
  info: { rows: 12 },
  proposed: "out",
  decision: null,
  outdated: false,
  blocking: false,
  ...over,
});

function mockApi(subjects: SubjectStatus[], rows: ReviewItem[]) {
  return vi.spyOn(client, "api").mockImplementation(async (path: string) => {
    if (path.endsWith("/review")) return { subjects } as never;
    const s = subjects.find((x) => path.includes(`/review/${x.subject}`)) ?? subjects[0];
    return { ...s, rows } as never;
  });
}

function renderAs(me: Me) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/ws1/review"]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={me} />}>
            <Route path="review" element={<Review />} />
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

describe("review helpers", () => {
  it("names what changed since a stale sign-off", () => {
    expect(
      signLine({
        state: "stale",
        signed_by: "ann",
        signed_at: "2026-10-25T09:34:09Z",
        added: ["C_BPARTNER.EKOS_CONTACT_EMAIL"],
        removed: [],
        changed: [],
      }),
    ).toBe("stale — signed by ann; since then 1 added");
    expect(signLine({ state: "none" })).toBe("not signed off");
    expect(stepChip("blocked")).toBe("chip bad");
  });

  it("shows signed facts, decisions and blocking items first", () => {
    expect(factsLine(item())).toBe("table=t_report · class=transient");
    expect(factsLine(item({ facts: { from: "0ed316fc-97fa-581e-ae7a-e8a30ccc7e78" } }))).toBe(
      "from=0ed316fc…",
    );
    expect(decisionLine(item())).toBe("proposed: out");
    expect(decisionLine(item({ decision: { scope: "in", reviewed_by: "ann" } }))).toBe("in");
    const rows = [item({ label: "B" }), item({ label: "A" }), item({ label: "Z", blocking: true })];
    expect(visibleItems(rows, "").map((r) => r.label)).toEqual(["Z", "A", "B"]);
    expect(visibleItems(rows, "z").map((r) => r.label)).toEqual(["Z"]);
  });
});

describe("Review page", () => {
  it("lets a reader see items and the digest but decide nothing", async () => {
    mockApi([status()], [item()]);
    renderAs(reader);
    expect(await screen.findByText("T_REPORT")).toBeInTheDocument();
    expect(screen.getByText(DIGEST)).toBeInTheDocument();
    expect(screen.getByText("Deciding needs the reviewer role.")).toBeInTheDocument();
    expect(screen.queryByText("Sign off")).toBeNull();
  });

  it("decides the selected items with the reviewer's name", async () => {
    mockApi([status()], [item()]);
    const post = vi
      .spyOn(client, "apiPost")
      .mockResolvedValue({ reviewer: "token:Ann", message: "1 scope decision(s)" });
    renderAs(writer);
    fireEvent.click(await screen.findByLabelText("select T_REPORT"));
    expect(screen.getByText("In scope")).toBeDisabled(); // no reviewer name yet
    fireEvent.change(screen.getByLabelText("reviewer"), { target: { value: "Ann" } });
    fireEvent.change(screen.getByLabelText("reason"), { target: { value: "needed" } });
    fireEvent.click(screen.getByText("In scope"));
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(post.mock.calls[0]).toEqual([
      "/workspaces/ws1/review/scope/decide",
      { targets: ["T_REPORT"], verdict: "in", mask: null, reason: "needed", reviewer: "Ann" },
    ]);
  });

  it("signs off only on the shown digest after the subject's name is typed", async () => {
    mockApi([status()], [item()]);
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ message: "signed" });
    renderAs(writer);
    await screen.findByText("T_REPORT");
    fireEvent.change(screen.getByLabelText("reviewer"), { target: { value: "Ann" } });
    const sign = screen.getByText("Sign off");
    expect(sign).toBeDisabled();
    fireEvent.change(screen.getByLabelText("confirm"), { target: { value: "scope" } });
    fireEvent.click(sign);
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(post.mock.calls[0]).toEqual([
      "/workspaces/ws1/review/scope/signoff",
      { digest: DIGEST, confirm: "scope", note: null, reviewer: "Ann" },
    ]);
  });

  it("offers no sign-off while items block, and sends conflicts to their own page", async () => {
    mockApi([status({ subject: "conflicts", blocking: 3 })], [item({ blocking: true })]);
    renderAs(writer);
    expect(await screen.findByText(/3 blocking item\(s\) must be decided/)).toBeInTheDocument();
    expect(screen.queryByText("Sign off")).toBeNull();
    expect(screen.getByRole("link", { name: "Conflicts" })).toHaveAttribute(
      "href",
      "/w/ws1/conflicts",
    );
  });
});

describe("StepBar", () => {
  it("shows every step with its state and links the review step", async () => {
    const steps: WorkflowStep[] = [
      { n: 1, id: "analyse", title: "Analyse sources", state: "done", detail: "fresh" },
      { n: 3, id: "review", title: "Review and sign-off", state: "stale", detail: "1 of 6" },
      { n: 4, id: "requirements", title: "Requirements", state: "blocked", detail: "x" },
    ];
    vi.spyOn(client, "api").mockResolvedValue({ steps } as never);
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={qc}>
        <MemoryRouter>
          <StepBar workspace="ws1" />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Requirements")).toBeInTheDocument();
    expect(screen.getByText("blocked")).toHaveClass("chip", "bad");
    expect(screen.getByRole("link", { name: /Review and sign-off/ })).toHaveAttribute(
      "href",
      "/w/ws1/review",
    );
  });
});
