import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { Annotation } from "../api/types";
import { ObjectNotes } from "./ObjectNotes";

const OBJ = "11111111-2222-4333-8444-555555555555";
const note = (over: Partial<Annotation> = {}): Annotation => ({
  id: "cc57a3dc-bd4c-507e-94fe-54f967a3594f",
  target: OBJ,
  kind: "comment",
  text: "amount is in cents",
  rule: null,
  term: null,
  author: "ann",
  scope: "personal",
  approved_by: null,
  state: "active",
  updated_at: "t",
  ...over,
});

function show(enabled: boolean) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <ObjectNotes workspace="ws1" objectId={OBJ} enabled={enabled} />
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

describe("ObjectNotes", () => {
  it("asks for this entity's notes only and shows them with their state", async () => {
    const api = vi.spyOn(client, "api").mockResolvedValue([
      note(),
      note({ id: "b", state: "stale", text: "old note", kind: "synonym", term: "turnover" }),
      note({ id: "c", state: "withdrawn", text: "gone" }),
    ]);
    show(true);
    expect(await screen.findByText("amount is in cents")).toBeInTheDocument();
    expect(api.mock.calls[0][0]).toBe(`/workspaces/ws1/annotations?target=${OBJ}`);
    expect(screen.getByText("Notes from people (2)")).toBeInTheDocument();
    expect(screen.getByText("old note")).toBeInTheDocument();
    expect(screen.queryByText("gone")).toBeNull();
  });

  it("does not ask at all when disabled (a restricted reader, or a time-travel view)", () => {
    const api = vi.spyOn(client, "api").mockResolvedValue([note()]);
    const { container } = show(false);
    expect(api).not.toHaveBeenCalled();
    expect(container).toBeEmptyDOMElement();
  });

  it("draws nothing when there are no notes", async () => {
    const api = vi.spyOn(client, "api").mockResolvedValue([]);
    const { container } = show(true);
    await vi.waitFor(() => expect(api).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });
});
