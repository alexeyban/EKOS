// RFC 0187: ask a project questions; a configured model answers with the project's EKOS tools,
// each call shown. The server streams Server-Sent Events from a POST, so the stream is read from
// `fetch` (an `EventSource` cannot POST).
//
// RFC 0253 adds two ways to tell EKOS something: attach a file or paste a lone link (it goes to
// the project's inbox as yours), and Tell mode, where the model only proposes — a card quoting
// your words, saved by you or not at all.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { useOutletContext, useParams } from "react-router-dom";
import { api, apiDelete, apiPost } from "../api/client";
import type { ProposalCard } from "../api/types";
import { Icon } from "../Icon";
import { Empty, PageHead } from "../ui";
import type { Me } from "../WorkspaceShell";
import { fileToBase64 } from "./people-shared";
import { TellCard } from "./TellCard";

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
  | { kind: "error"; text: string }
  | { kind: "card"; card: ProposalCard }
  | { kind: "note"; text: string };

/** A message that is only a link: it goes to the inbox, not to the model. */
export function isLoneLink(text: string): boolean {
  return /^https?:\/\/\S{3,2000}$/.test(text.trim());
}

/** What the inbox said, as a line in the conversation. */
export function storedNote(r: { name?: string; status?: string }): string {
  const name = r.name ?? "it";
  return r.status === "pending"
    ? `Stored ${name} in the inbox; it waits for a reviewer to accept it.`
    : `Stored ${name} in the inbox; it is compiled on the next build.`;
}

/** A stored conversation as the items the page draws. */
export function itemsOf(messages: Stored[]): Item[] {
  const out: Item[] = [];
  for (const m of messages) {
    if (m.role === "user") out.push({ kind: "user", text: m.content });
    else if (m.role === "assistant") out.push(...assistantItems(m));
    else attachResult(out, m);
  }
  return out;
}

/** An assistant message's tool calls, or its answer when it made none. */
function toolMark(ok: boolean | undefined): string {
  if (ok === undefined) return "…";
  return ok ? "✓" : "✗";
}

/** One line of the conversation. */
function ChatItem({ item, workspace, reviewer }: { item: Item; workspace: string; reviewer: string | null }) {
  switch (item.kind) {
    case "user":
      return (
        <div className="chat-msg chat-msg-user">
          <span className="chat-who">You</span>
          <div className="chat-user">{item.text}</div>
        </div>
      );
    case "answer":
      return (
        <div className="chat-msg chat-msg-answer">
          <span className="chat-who">EKOS</span>
          <div className="chat-answer">{item.text}</div>
        </div>
      );
    case "tool":
      return (
        <details className="chat-tool">
          <summary>
            {toolMark(item.ok)} <code>{item.name}</code> <code className="muted">{JSON.stringify(item.args)}</code>
          </summary>
          {item.result && <pre>{item.result}</pre>}
        </details>
      );
    case "card":
      return <TellCard workspace={workspace} card={item.card} reviewer={reviewer} />;
    case "note":
      return <p className="muted small chat-note">{item.text}</p>;
    default:
      return (
        <p className="err" role="alert">
          {item.text}
        </p>
      );
  }
}

function assistantItems(m: Extract<Stored, { role: "assistant" }>): Item[] {
  const calls = m.tool_calls ?? [];
  const items: Item[] = calls.map((c) => ({ kind: "tool", id: c.id, name: c.name, args: c.arguments }));
  if (m.content && !calls.length) items.push({ kind: "answer", text: m.content });
  return items;
}

/** Puts a tool result on the call it answers. */
function attachResult(out: Item[], m: Extract<Stored, { role: "tool" }>): void {
  const t = out.find((i) => i.kind === "tool" && i.id === m.tool_call_id);
  if (t && t.kind === "tool") {
    t.ok = m.ok !== false;
    t.result = m.content.slice(0, 400);
  }
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

type Usage = { input: number; output: number };

/** What one streamed event changes on the page. */
interface EventSink {
  setCurrent: (id: string) => void;
  setItems: (f: (xs: Item[]) => Item[]) => void;
  setUsage: (f: (u: Usage) => Usage) => void;
}

function applyEvent(e: Record<string, unknown>, sink: EventSink): void {
  switch (e.type) {
    case "conversation":
      sink.setCurrent(String(e.id));
      break;
    case "tool_call":
      sink.setItems((xs) => [...xs, { kind: "tool", id: String(e.id), name: String(e.name), args: e.arguments }]);
      break;
    case "tool_result":
      sink.setItems((xs) =>
        xs.map((x) => (x.kind === "tool" && x.id === e.id ? { ...x, ok: Boolean(e.ok), result: String(e.summary) } : x)),
      );
      break;
    case "text":
      sink.setItems((xs) => [...xs, { kind: "answer", text: String(e.text) }]);
      break;
    case "usage":
      sink.setUsage((u) => ({ input: u.input + Number(e.input ?? 0), output: u.output + Number(e.output ?? 0) }));
      break;
    case "error":
      sink.setItems((xs) => [...xs, { kind: "error", text: String(e.message) }]);
      break;
  }
}

/** Reads a Server-Sent Events body to its end, handing each event to `onEvent`. */
async function readEvents(body: ReadableStream<Uint8Array>, onEvent: (e: Record<string, unknown>) => void): Promise<void> {
  const reader = body.getReader();
  const dec = new TextDecoder();
  let buf = "";
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += dec.decode(value, { stream: true });
    const [events, rest] = splitEvents(buf);
    buf = rest;
    for (const e of events as Record<string, unknown>[]) onEvent(e);
  }
}

/** The error a refused chat request carries: the server's `detail`, else the status line. */
async function refusal(res: Response): Promise<Error> {
  const j = await res.json().catch(() => ({}));
  return new Error(typeof j.detail === "string" ? j.detail : `${res.status} ${res.statusText}`);
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
  const me = useOutletContext<Me | undefined>();
  const [mode, setMode] = useState<"ask" | "tell">("ask");
  const [name, setName] = useState("");
  // Token mode has no identity: what is sent or saved is recorded as token:<name>.
  const reviewer = (me != null && me.mode !== "oidc") ? name : null;

  const toInbox = async (body: Record<string, unknown>, shown: string) => {
    setItems((xs) => [...xs, { kind: "user", text: shown }]);
    try {
      const r = await apiPost<{ name?: string; status?: string }>(`/workspaces/${id}/inbox`, { ...body, reviewer });
      setItems((xs) => [...xs, { kind: "note", text: storedNote(r) }]);
    } catch (err) {
      setItems((xs) => [...xs, { kind: "error", text: String((err as Error).message) }]);
    }
  };

  const attach = async (file: File | undefined) => {
    if (!file) return;
    await toInbox({ name: file.name, content_base64: await fileToBase64(file) }, `Attached ${file.name}`);
  };

  const tell = async (text: string) => {
    setItems((xs) => [...xs, { kind: "user", text }]);
    try {
      const card = await apiPost<ProposalCard>(`/workspaces/${id}/chat/tell`, { message: text });
      setItems((xs) => [...xs, { kind: "card", card }]);
    } catch (err) {
      setItems((xs) => [...xs, { kind: "error", text: String((err as Error).message) }]);
    }
  };

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
    if (isLoneLink(text) || mode === "tell") {
      setBusy(true);
      try {
        await (isLoneLink(text) ? toInbox({ url: text }, text) : tell(text));
      } finally {
        setBusy(false);
      }
      return;
    }
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
      if (!res.ok || !res.body) throw await refusal(res);
      await readEvents(res.body, (e) => applyEvent(e, { setCurrent, setItems, setUsage }));
    } catch (err) {
      if ((err as Error).name !== "AbortError") setItems((xs) => [...xs, { kind: "error", text: String((err as Error).message) }]);
    } finally {
      setBusy(false);
      abort.current = null;
      void qc.invalidateQueries({ queryKey: ["chats", id] });
    }
  };

  const startOver = () => {
    setCurrent(null);
    setItems([]);
    setUsage({ input: 0, output: 0 });
  };

  const remove = async (cid: string) => {
    await apiDelete(`/workspaces/${id}/chat/${cid}`);
    if (current === cid) {
      setCurrent(null);
      setItems([]);
    }
    void qc.invalidateQueries({ queryKey: ["chats", id] });
  };

  const head = (
    <PageHead
      title="Chat"
      sub="Ask this project questions. A model answers with the project's EKOS tools, and every tool call is shown."
    />
  );

  if (model.data && !model.data.configured) {
    return (
      <>
        {head}
        <section className="card">
          <Empty title="No chat model yet" icon="chat">
            <p>
              No chat model is set. An owner chooses one on the console's Settings page (Chat), or in{" "}
              <code>ekos setup</code>: a local Ollama model, or a hosted one.
            </p>
          </Empty>
        </section>
      </>
    );
  }

  return (
    <>
      {head}
      <div className="chat-page">
        <ConversationList
          list={list.data ?? []}
          current={current}
          onNew={startOver}
          onOpen={(cid) => void open(cid)}
          onDelete={(cid) => void remove(cid)}
        />

        <section className="card chat-main" aria-label="conversation">
          {model.data && <ModelLine info={model.data} usage={usage} />}
          <div className="chat-items" aria-live="polite">
            {items.length === 0 && (
              <Empty title="Ask about this project" icon="chat">
                What a table holds, what depends on a module, where a term is defined.
              </Empty>
            )}
            {items.map((it, i) => (
              <ChatItem key={i} item={it} workspace={id} reviewer={reviewer} />
            ))}
          </div>
          <div className="chat-composer">
            <div className="btnrow chat-tell">
              <div className="x-seg" role="radiogroup" aria-label="mode">
                <label>
                  <input type="radio" name="chat-mode" checked={mode === "ask"} onChange={() => setMode("ask")} /> Ask
                </label>
                <label>
                  <input type="radio" name="chat-mode" checked={mode === "tell"} onChange={() => setMode("tell")} />{" "}
                  Tell EKOS something
                </label>
              </div>
              <label className="chat-attach">
                <Icon name="file" /> Attach a file
                <input
                  type="file"
                  className="x-visually-hidden"
                  aria-label="attach a file"
                  onChange={(e) => {
                    void attach(e.target.files?.[0]);
                    e.target.value = "";
                  }}
                />
              </label>
              {me != null && me.mode !== "oidc" && (
                <input aria-label="your name" placeholder="your name" value={name} onChange={(e) => setName(e.target.value)} />
              )}
              <span className="muted small">a lone link goes to the project's inbox</span>
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
                placeholder={mode === "tell" ? "What do you know? (you save it in your own words)" : "Ask about this project"}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void send();
                  }
                }}
              />
              <SendButton busy={busy} mode={mode} empty={!input.trim()} onStop={() => abort.current?.abort()} />
            </form>
          </div>
        </section>
      </div>
    </>
  );
}

function SendButton({
  busy,
  mode,
  empty,
  onStop,
}: {
  busy: boolean;
  mode: "ask" | "tell";
  empty: boolean;
  onStop: () => void;
}) {
  if (busy) {
    return (
      <button type="button" onClick={onStop}>
        Stop
      </button>
    );
  }
  return (
    <button type="submit" className="primary" disabled={empty}>
      {mode === "tell" ? "Tell" : "Send"}
    </button>
  );
}

function ModelLine({ info, usage }: { info: ModelInfo; usage: Usage }) {
  return (
    <p className="muted small chat-model">
      {info.provider} · {info.model}
      {info.metered ? " · paid per call" : " · runs locally, not paid per call"} · answers from this project's EKOS tools
      only
      {usage.output > 0 && ` · ${usage.input} tokens in, ${usage.output} out`}
    </p>
  );
}

function ConversationList({
  list,
  current,
  onNew,
  onOpen,
  onDelete,
}: {
  list: ConversationRow[];
  current: string | null;
  onNew: () => void;
  onOpen: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  return (
    <section className="card chat-list" aria-label="conversations">
      <button type="button" className="chat-new" onClick={onNew}>
        New conversation
      </button>
      {list.length === 0 && <p className="muted small">No saved conversations yet.</p>}
      <ul>
        {list.map((c) => (
          <li key={c.id} className={current === c.id ? "on" : undefined}>
            <button
              type="button"
              className={`linkish chat-open ${current === c.id ? "on" : ""}`}
              aria-current={current === c.id ? "true" : undefined}
              onClick={() => onOpen(c.id)}
            >
              {c.title || "untitled"}
            </button>
            <button
              type="button"
              className="linkish muted chat-delete"
              aria-label={`delete ${c.title}`}
              onClick={() => onDelete(c.id)}
            >
              ×
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
