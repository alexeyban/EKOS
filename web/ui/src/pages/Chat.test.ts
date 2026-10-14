import { describe, expect, it } from "vitest";
import { itemsOf, splitEvents } from "./Chat";

describe("chat page", () => {
  it("splits a stream into events and keeps a partial one", () => {
    const [events, rest] = splitEvents('data: {"type":"text","text":"a"}\n\ndata: {"type":"done"}\n\ndata: {"ty');
    expect(events).toEqual([{ type: "text", text: "a" }, { type: "done" }]);
    expect(rest).toBe('data: {"ty');
  });

  it("draws a stored conversation", () => {
    const items = itemsOf([
      { role: "user", content: "customers?" },
      { role: "assistant", content: "", tool_calls: [{ id: "a", name: "ekos_search", arguments: { query: "c" } }] },
      { role: "tool", tool_call_id: "a", name: "ekos_search", content: "1 hit", ok: true },
      { role: "assistant", content: "A table [t1]." },
    ]);
    expect(items.map((i) => i.kind)).toEqual(["user", "tool", "answer"]);
    expect(items[1]).toMatchObject({ name: "ekos_search", ok: true, result: "1 hit" });
  });
});
