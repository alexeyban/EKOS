// RFC 0187: ask a project questions; a configured model answers with the project's EKOS tools,
// each call shown. The server streams Server-Sent Events from a POST, so the stream is read from
// `fetch` (an `EventSource` cannot POST).
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { useParams } from "react-router-dom";
import { api, apiDelete } from "../api/client";

interface ModelInfo {
  configured: boolean;
  provider: string;
  model: string;
  metered: boolean;
}
interface ConversationRow {
  id: string;
  title: string;
  updated_at: string;
}
type Stored =
  | { role: "user"; content: string }
  | { role: "assistant"; content: string; tool_calls?: { id: string; name: string; arguments: unknown }[] }
  | { role: "tool"; tool_call_id: string; name: string; content: string; ok?: boolean };

export type Item =
  | { kind: "user"; text: string }
  | { kind: "answer"; text: string }
  | { kind: "tool"; id: string; name: string; args: unknown; ok?: boolean; result?: string }
  | { kind: "error"; text: string };

/** A stored conversation as the items the page draws. */
export function itemsOf(messages: Stored[]): Item[] {
  const out: Item[] = [];
  for (const m of messages) {
    if (m.role === "user") out.push({ kind: "user", text: m.content });
    else if (m.role === "assistant") {
      for (const c of m.tool_calls ?? []) out.push({ kind: "tool", id: c.id, name: c.name, args: c.arguments });
      if (m.content && !(m.tool_calls ?? []).length) out.push({ kind: "answer", text: m.content });
    } else {
      const t = out.find((i) => i.kind === "tool" && i.id === m.tool_call_id);
      if (t && t.kind === "tool") {
        t.ok = m.ok !== false;
        t.result = m.content.slice(0, 400);
      }
    }
  }
  return out;
}

/** Complete `data:` events in `buffer`, and what is left of it. */
export function splitEvents(buffer: string): [unknown[], string] {
  const parts = buffer.split("\n\n");
  const rest = parts.pop() ?? "";
  const events = parts
    .map((p) => p.split("\n").find((l) => l.startsWith("data: ")))
    .filter((l): l is string => !!l)
    .map((l) => JSON.parse(l.slice(6)));
  return [events, rest];
}

export function Chat() {
  const { id = "" } = useParams();
  const qc = useQueryClient();
  const model = useQuery({ queryKey: ["chat-model"], queryFn: () => api<ModelInfo>("/chat/model") });
  const list = useQuery({
    queryKey: ["chats", id],
    queryFn: () => api<ConversationRow[]>(`/workspaces/${id}/chat`),
    enabled: id !== "",
  });
  const [current, setCurrent] = useState<string | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [usage, setUsage] = useState({ input: 0, output: 0 });
  const abort = useRef<AbortController | null>(null);

  const open = async (cid: string) => {
    const conv = await api<{ messages: Stored[] }>(`/workspaces/${id}/chat/${cid}`);
    setCurrent(cid);
    setItems(itemsOf(conv.messages));
    setUsage({ input: 0, output: 0 });
  };

  const send = async () => {
    const text = input.trim();
    if (!text || busy) return;
    setInput("");
    setBusy(true);
    setItems((xs) => [...xs, { kind: "user", text }]);
    const ctrl = new AbortController();
    abort.current = ctrl;
    try {
      const res = await fetch(`/api/workspaces/${id}/chat`, {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ message: text, conversation_id: current }),
        signal: ctrl.signal,
      });
      if (!res.ok || !res.body) {
        const j = await res.json().catch(() => ({}));
        throw new Error(typeof j.detail === "string" ? j.detail : `${res.status} ${res.statusText}`);
      }
      const reader = res.body.getReader();
      const dec = new TextDecoder();
      let buf = "";
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += dec.decode(value, { stream: true });
        const [events, rest] = splitEvents(buf);
        buf = rest;
        for (const e of events as Record<string, unknown>[]) {
          if (e.type === "conversation") setCurrent(String(e.id));
          else if (e.type === "tool_call")
            setItems((xs) => [...xs, { kind: "tool", id: String(e.id), name: String(e.name), args: e.arguments }]);
          else if (e.type === "tool_result")
            setItems((xs) =>
              xs.map((x) => (x.kind === "tool" && x.id === e.id ? { ...x, ok: Boolean(e.ok), result: String(e.summary) } : x)),
            );
          else if (e.type === "text") setItems((xs) => [...xs, { kind: "answer", text: String(e.text) }]);
          else if (e.type === "usage")
            setUsage((u) => ({ input: u.input + Number(e.input ?? 0), output: u.output + Number(e.output ?? 0) }));
          else if (e.type === "error") setItems((xs) => [...xs, { kind: "error", text: String(e.message) }]);
        }
      }
    } catch (err) {
      if ((err as Error).name !== "AbortError") setItems((xs) => [...xs, { kind: "error", text: String((err as Error).message) }]);
    } finally {
      setBusy(false);
      abort.current = null;
      void qc.invalidateQueries({ queryKey: ["chats", id] });
    }
  };

  if (model.data && !model.data.configured) {
    return (
      <section className="card">
        <strong>Chat</strong>
        <p className="muted">
          No chat model is set. An owner chooses one on the console's Settings page (Chat), or in <code>ekos setup</code>: a
          local Ollama model, or a hosted one.
        </p>
      </section>
    );
  }

  return (
    <div className="chat-page">
      <section className="card chat-list">
        <button
          onClick={() => {
            setCurrent(null);
            setItems([]);
            setUsage({ input: 0, output: 0 });
          }}
        >
          New conversation
        </button>
        <ul>
          {(list.data ?? []).map((c) => (
            <li key={c.id}>
              <button className={`linkish ${current === c.id ? "on" : ""}`} onClick={() => void open(c.id)}>
                {c.title || "untitled"}
              </button>{" "}
              <button
                className="linkish muted"
                aria-label={`delete ${c.title}`}
                onClick={async () => {
                  await apiDelete(`/workspaces/${id}/chat/${c.id}`);
                  if (current === c.id) {
                    setCurrent(null);
                    setItems([]);
                  }
                  void qc.invalidateQueries({ queryKey: ["chats", id] });
                }}
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      </section>

      <section className="card chat-main">
        {model.data && (
          <p className="muted small">
            {model.data.provider} · {model.data.model}
            {model.data.metered ? " · paid per call" : " · runs locally, not paid per call"} · answers from this project's EKOS
            tools only
            {usage.output > 0 && ` · ${usage.input} tokens in, ${usage.output} out`}
          </p>
        )}
        <div className="chat-items">
          {items.length === 0 && <p className="muted">Ask about this project: what a table holds, what depends on a module, where a term is defined.</p>}
          {items.map((it, i) =>
            it.kind === "user" ? (
              <div key={i} className="chat-user">
                {it.text}
              </div>
            ) : it.kind === "answer" ? (
              <div key={i} className="chat-answer">
                {it.text}
              </div>
            ) : it.kind === "tool" ? (
              <details key={i} className="chat-tool">
                <summary>
                  {it.ok === undefined ? "…" : it.ok ? "✓" : "✗"} <code>{it.name}</code> <code className="muted">{JSON.stringify(it.args)}</code>
                </summary>
                {it.result && <pre>{it.result}</pre>}
              </details>
            ) : (
              <p key={i} className="err">
                {it.text}
              </p>
            ),
          )}
        </div>
        <form
          className="chat-input"
          onSubmit={(e) => {
            e.preventDefault();
            void send();
          }}
        >
          <textarea
            aria-label="message"
            value={input}
            placeholder="Ask about this project"
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
          />
          {busy ? (
            <button type="button" onClick={() => abort.current?.abort()}>
              Stop
            </button>
          ) : (
            <button type="submit" disabled={!input.trim()}>
              Send
            </button>
          )}
        </form>
      </section>
    </div>
  );
}
