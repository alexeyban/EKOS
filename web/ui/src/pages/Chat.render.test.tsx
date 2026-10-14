import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import { Chat } from "./Chat";

const MODEL = { configured: true, provider: "ollama", model: "qwen2.5:1.5b", metered: false };

function mockApi(over: { model?: object; list?: object[]; conversation?: object } = {}) {
  return vi.spyOn(client, "api").mockImplementation((path: string) => {
    if (path === "/chat/model") return Promise.resolve(over.model ?? MODEL) as never;
    if (path === "/workspaces/shop/chat") return Promise.resolve(over.list ?? []) as never;
    if (path.startsWith("/workspaces/shop/chat/"))
      return Promise.resolve(over.conversation ?? { messages: [] }) as never;
    return Promise.reject(new Error(`unexpected ${path}`));
  });
}

/** A streamed response of Server-Sent Events, delivered in the given chunks. */
function sse(chunks: string[], init: ResponseInit = {}) {
  const enc = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(ctrl) {
      for (const c of chunks) ctrl.enqueue(enc.encode(c));
      ctrl.close();
    },
  });
  return new Response(body, { status: 200, ...init });
}

const event = (e: object) => `data: ${JSON.stringify(e)}\n\n`;

function renderChat() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/shop/chat"]}>
        <Routes>
          <Route path="/w/:id/chat" element={<Chat />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("Chat page", () => {
  it("says how to set a model when none is configured", async () => {
    mockApi({ model: { configured: false, provider: "", model: "", metered: false } });
    renderChat();
    expect(await screen.findByText(/No chat model is set/)).toBeTruthy();
  });

  it("labels a local model as not paid, a hosted one as paid per call", async () => {
    mockApi();
    const first = renderChat();
    expect(await screen.findByText(/runs locally, not paid per call/)).toBeTruthy();
    first.unmount();
    vi.restoreAllMocks();
    mockApi({ model: { ...MODEL, provider: "anthropic", model: "claude", metered: true } });
    renderChat();
    expect(await screen.findByText(/paid per call/)).toBeTruthy();
  });

  it("streams a turn: the question, each tool call with its result, the answer and usage", async () => {
    mockApi();
    const fetchMock = vi.fn().mockResolvedValue(
      sse([
        event({ type: "conversation", id: "c1" }),
        // An event split across two chunks is still read whole.
        'data: {"type":"tool_call","id":"t1","name":"ekos_search",',
        '"arguments":{"query":"customers"}}\n\n',
        event({ type: "tool_result", id: "t1", ok: true, summary: "3 hits" }),
        event({ type: "text", text: "customers is a table [t1]." }),
        event({ type: "usage", input: 120, output: 30 }),
        event({ type: "done" }),
      ]),
    );
    vi.stubGlobal("fetch", fetchMock);
    renderChat();
    await screen.findByText(/runs locally/);
    fireEvent.change(screen.getByLabelText("message"), { target: { value: "  what is customers?  " } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(await screen.findByText("customers is a table [t1].")).toBeTruthy();
    expect(screen.getByText("what is customers?")).toBeTruthy();
    expect(screen.getByText("ekos_search")).toBeTruthy();
    expect(screen.getByText("3 hits")).toBeTruthy();
    expect(screen.getByText(/120 tokens in, 30 out/)).toBeTruthy();
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/api/workspaces/shop/chat");
    expect(JSON.parse(init.body)).toEqual({ message: "what is customers?", conversation_id: null });
  });

  it("Enter sends; the next question continues the same conversation", async () => {
    mockApi();
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(sse([event({ type: "conversation", id: "c9" }), event({ type: "text", text: "first" })]))
      .mockResolvedValueOnce(sse([event({ type: "text", text: "second" })]));
    vi.stubGlobal("fetch", fetchMock);
    renderChat();
    await screen.findByText(/runs locally/);
    const box = screen.getByLabelText("message");
    fireEvent.change(box, { target: { value: "one" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await screen.findByText("first");
    fireEvent.change(box, { target: { value: "two" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await screen.findByText("second");
    expect(JSON.parse(fetchMock.mock.calls[1][1].body).conversation_id).toBe("c9");
  });

  it("shows the server's refusal, and an error event", async () => {
    mockApi();
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ detail: "metered calls are off for this tenant" }), { status: 403 }),
        )
        .mockResolvedValueOnce(sse([event({ type: "error", message: "the model is not reachable" })])),
    );
    renderChat();
    await screen.findByText(/runs locally/);
    const box = screen.getByLabelText("message");
    fireEvent.change(box, { target: { value: "q1" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText("metered calls are off for this tenant")).toBeTruthy();
    fireEvent.change(box, { target: { value: "q2" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText("the model is not reachable")).toBeTruthy();
  });

  it("opens, starts over and deletes stored conversations", async () => {
    mockApi({
      list: [{ id: "c1", title: "about customers", updated_at: "2026-10-14" }],
      conversation: {
        messages: [
          { role: "user", content: "customers?" },
          { role: "assistant", content: "", tool_calls: [{ id: "a", name: "ekos_search", arguments: {} }] },
          { role: "tool", tool_call_id: "a", name: "ekos_search", content: "failed", ok: false },
          { role: "assistant", content: "Not found." },
        ],
      },
    });
    const del = vi.spyOn(client, "apiDelete").mockResolvedValue(undefined);
    renderChat();
    fireEvent.click(await screen.findByRole("button", { name: "about customers" }));
    expect(await screen.findByText("Not found.")).toBeTruthy();
    expect(screen.getByText("✗", { exact: false })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "New conversation" }));
    expect(screen.queryByText("Not found.")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "about customers" }));
    await screen.findByText("Not found.");
    fireEvent.click(screen.getByRole("button", { name: "delete about customers" }));
    await waitFor(() => expect(del).toHaveBeenCalledWith("/workspaces/shop/chat/c1"));
    await waitFor(() => expect(screen.queryByText("Not found.")).toBeNull());
  });

  it("Stop aborts a running turn without an error line", async () => {
    mockApi();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockImplementation(
        (_url: string, init: RequestInit) =>
          new Promise((_resolve, reject) => {
            init.signal?.addEventListener("abort", () =>
              reject(Object.assign(new Error("aborted"), { name: "AbortError" })),
            );
          }),
      ),
    );
    renderChat();
    await screen.findByText(/runs locally/);
    fireEvent.change(screen.getByLabelText("message"), { target: { value: "long question" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
    expect(await screen.findByRole("button", { name: "Send" })).toBeTruthy();
    expect(document.querySelector(".err")).toBeNull();
  });
});
