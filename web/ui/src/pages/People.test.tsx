// RFC 0253/0254 pages: the inbox, the questions, the channels page and the chat's tell mode.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Outlet, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import type { InboxItem, ProposalCard, Question } from "../api/types";
import type { Me } from "../WorkspaceShell";
import { Channels } from "./Channels";
import { Chat, isLoneLink, storedNote } from "./Chat";
import { Inbox } from "./Inbox";
import {
  fileToBase64,
  inboxOrder,
  isMine,
  revealedRows,
  sizeText,
} from "./people-shared";
import { Questions } from "./Questions";

const QID = "0b6a1c2e-1111-5222-8333-944455556666";

const item = (over: Partial<InboxItem> = {}): InboxItem => ({
  id: "ab12cd34ef567890",
  name: "invoice-spec.pdf",
  status: "pending",
  size: 2048,
  received_at: "2026-11-10T10:00:00Z",
  senders: [{ by: "bob@x.io", channel: "telegram" }],
  ...over,
});

const question = (over: Partial<Question> = {}): Question => ({
  id: QID,
  type: "unexplained_values",
  status: "open",
  table: "orders",
  column: "status",
  text: "4% of rows in orders.status hold 2 values no code mentions. What are they?",
  addressee: "ann@x.io",
  addressees: ["ann@x.io", "bob@x.io"],
  answers: [],
  ...over,
});

function renderAt(path: string, element: React.ReactNode, me: Me) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/w/:id" element={<Outlet context={me} />}>
            <Route path="*" element={element} />
          </Route>
          <Route path="/" element={<Outlet context={me} />}>
            <Route path="channels" element={element} />
          </Route>
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const ann: Me = {
  mode: "oidc",
  email: "ann@x.io",
  role: "write",
  canReview: true,
  canAdmin: true,
  canOperate: true,
};
const reader: Me = {
  mode: "oidc",
  email: "bob@x.io",
  role: "read",
  canReview: false,
  canAdmin: false,
  canOperate: false,
};

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("people helpers", () => {
  it("orders the inbox, sizes and links", () => {
    const old = item({
      id: "1",
      status: "accepted",
      received_at: "2026-11-01",
    });
    const fresh = item({
      id: "2",
      status: "accepted",
      received_at: "2026-11-09",
    });
    const waiting = item({ id: "3", received_at: "2026-10-01" });
    expect(inboxOrder([old, fresh, waiting]).map((i) => i.id)).toEqual([
      "3",
      "2",
      "1",
    ]);
    expect(sizeText(10)).toBe("10 B");
    expect(sizeText(2048)).toBe("2.0 KB");
    expect(isLoneLink(" https://wiki.example/p/1 ")).toBe(true);
    expect(isLoneLink("see https://wiki.example/p/1")).toBe(false);
    expect(storedNote({ name: "a.pdf", status: "pending" })).toMatch(
      /waits for a reviewer/,
    );
  });

  it("knows whose question it is", () => {
    expect(isMine(question(), ann, "")).toBe(true);
    expect(isMine(question(), reader, "")).toBe(false);
    const tokenMe: Me = { mode: "token", email: null, role: "write" };
    expect(isMine(question({ addressee: "token:Ann" }), tokenMe, "Ann")).toBe(
      true,
    );
    expect(isMine(question({ addressee: null }), ann, "")).toBe(false);
  });

  it("shows revealed values or what was withheld", () => {
    expect(revealedRows([{ value: "9", rows: 120 }])).toEqual({
      rows: [{ value: "9", rows: 120 }],
    });
    expect(revealedRows({ withheld: "a personal-data column" })).toEqual({
      note: "a personal-data column",
    });
  });

  it("reads a file as base64", async () => {
    expect(await fileToBase64(new Blob(["hello"]))).toBe("aGVsbG8=");
  });
});

describe("Inbox page", () => {
  it("accepts and purges with the id typed; a reader only sees and sends", async () => {
    vi.spyOn(client, "api").mockResolvedValue([item()] as never);
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({} as never);
    const view = renderAt("/w/shop/inbox", <Inbox />, ann);
    expect(await screen.findByText("invoice-spec.pdf")).toBeTruthy();
    expect(screen.getByText(/from bob@x.io \(telegram\)/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        "/workspaces/shop/inbox/ab12cd34ef567890/accept",
        {
          reviewer: null,
        },
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Purge…" }));
    const purge = screen.getByRole("button", { name: "Purge the original" });
    expect((purge as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("type ab12cd34ef567890 to purge"), {
      target: { value: "ab12cd34ef567890" },
    });
    fireEvent.click(purge);
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        "/workspaces/shop/inbox/ab12cd34ef567890/purge",
        {
          reviewer: null,
          confirm: "ab12cd34ef567890",
        },
      ),
    );
    view.unmount();
    renderAt("/w/shop/inbox", <Inbox />, reader);
    await screen.findByText("invoice-spec.pdf");
    expect(screen.queryByRole("button", { name: "Accept" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Purge…" })).toBeNull();
    expect(screen.getByRole("button", { name: "Send" })).toBeTruthy();
  });

  it("sends a link", async () => {
    vi.spyOn(client, "api").mockResolvedValue([] as never);
    const post = vi
      .spyOn(client, "apiPost")
      .mockResolvedValue({ name: "wiki.html", status: "pending" } as never);
    renderAt("/w/shop/inbox", <Inbox />, reader);
    expect(await screen.findByText("The inbox is empty.")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("link"), {
      target: { value: "https://wiki.example/p" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(
      await screen.findByText(/Stored wiki.html \(pending\)/),
    ).toBeTruthy();
    expect(post).toHaveBeenCalledWith("/workspaces/shop/inbox", {
      url: "https://wiki.example/p",
      reviewer: null,
    });
  });
});

describe("Questions page", () => {
  it("filters, reveals values, records an answer heard elsewhere and says not me", async () => {
    const api = vi.spyOn(client, "api").mockImplementation((path: string) => {
      if (path.endsWith("/reveal"))
        return Promise.resolve(
          question({ revealed: [{ value: "9", rows: 120 }] }),
        ) as never;
      return Promise.resolve([question()]) as never;
    });
    const post = vi
      .spyOn(client, "apiPost")
      .mockResolvedValue({ by: "ann@x.io" } as never);
    renderAt("/w/shop/questions", <Questions />, ann);
    fireEvent.click(
      await screen.findByRole("button", { name: question().text }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Reveal values" }));
    expect(await screen.findByLabelText("revealed values")).toBeTruthy();
    expect(screen.getByText("120")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("said by"), {
      target: { value: "Bob" },
    });
    fireEvent.change(screen.getByLabelText("answer"), {
      target: { value: "Refunds." },
    });
    fireEvent.click(screen.getByRole("button", { name: "Record the answer" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        `/workspaces/shop/questions/${QID}/answer`,
        {
          text: "Refunds.",
          said_by: "Bob",
          reviewer: null,
        },
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Not me" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        `/workspaces/shop/questions/${QID}/decline`,
        {
          reviewer: null,
        },
      ),
    );
    fireEvent.change(screen.getByLabelText("reassign to"), {
      target: { value: "cy@x.io" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Reassign" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith(
        `/workspaces/shop/questions/${QID}/reassign`,
        {
          to: "cy@x.io",
        },
      ),
    );
    fireEvent.change(screen.getByLabelText("type"), {
      target: { value: "dead_code" },
    });
    fireEvent.change(screen.getByLabelText("table"), {
      target: { value: "orders" },
    });
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith(
        "/workspaces/shop/questions?status=open&type=dead_code&table=orders",
      ),
    );
  });

  it("a reader who is not the addressee only reads", async () => {
    vi.spyOn(client, "api").mockResolvedValue([
      question({ type: "dead_code" }),
    ] as never);
    renderAt("/w/shop/questions", <Questions />, reader);
    fireEvent.click(
      await screen.findByRole("button", { name: question().text }),
    );
    expect(screen.queryByRole("button", { name: "Not me" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Record the answer" }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Reassign" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Reveal values" })).toBeNull();
  });
});

describe("Channels page", () => {
  it("links an account with the code; an owner binds and sends the digest", async () => {
    vi.spyOn(client, "api").mockImplementation((path: string) => {
      if (path === "/channels/me") return Promise.resolve([]) as never;
      return Promise.resolve({
        configured: { telegram: true, slack: false, teams: false },
        public: false,
        digest_cron: "",
        accounts: [],
        bindings: [
          {
            id: "slack:C0FIN",
            channel: "slack",
            conversation: "C0FIN",
            workspace_id: "shop",
            created_by: "o",
          },
        ],
      }) as never;
    });
    const post = vi
      .spyOn(client, "apiPost")
      .mockImplementation(
        (path: string) =>
          Promise.resolve(
            path === "/channels/link"
              ? { channel: "telegram", label: "ann@x.io" }
              : { sent: 2 },
          ) as never,
      );
    renderAt("/channels", <Channels />, { ...ann, owner: true } as Me);
    fireEvent.change(await screen.findByLabelText("code"), {
      target: { value: "abcd2345" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    expect(
      await screen.findByText(/Linked your telegram account as ann@x.io/),
    ).toBeTruthy();
    expect(post).toHaveBeenCalledWith("/channels/link", { code: "ABCD2345" });
    expect(await screen.findByText(/Running: telegram/)).toBeTruthy();
    expect(screen.getByText(/slack C0FIN → shop/)).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "Send the questions digest now" }),
    );
    expect(await screen.findByText("2 question(s) sent")).toBeTruthy();
  });
});

describe("Chat tell mode", () => {
  const card: ProposalCard = {
    proposal: "p1",
    text: "Status 7 is when the customer cancels.",
    items: [
      {
        kind: "meaning",
        about: "orders.status = 7",
        meaning: "cancelled",
        quote: "Status 7",
      },
    ],
    lines: ['orders.status = 7 means "cancelled"  [from: "Status 7"]'],
    dropped: 1,
    tools: ["ekos_search"],
    answers: null,
  };

  function mockChat() {
    vi.spyOn(client, "api").mockImplementation((path: string) => {
      if (path === "/chat/model")
        return Promise.resolve({
          configured: true,
          provider: "ollama",
          model: "m",
          metered: false,
        }) as never;
      return Promise.resolve([]) as never;
    });
  }

  it("a statement becomes a card the person saves in their own (edited) words", async () => {
    mockChat();
    const post = vi
      .spyOn(client, "apiPost")
      .mockImplementation(
        (path: string) =>
          Promise.resolve(
            path.endsWith("/chat/tell") ? card : { by: "ann@x.io" },
          ) as never,
      );
    renderAt("/w/shop/chat", <Chat />, ann);
    await screen.findByText(/runs locally/);
    fireEvent.click(screen.getByLabelText(/Tell EKOS something/));
    fireEvent.change(screen.getByLabelText("message"), {
      target: { value: card.text },
    });
    fireEvent.click(screen.getByRole("button", { name: "Tell" }));
    expect(await screen.findByText(/orders.status = 7 means/)).toBeTruthy();
    expect(screen.getByText(/1 more proposal\(s\) dropped/)).toBeTruthy();
    expect(post).toHaveBeenCalledWith("/workspaces/shop/chat/tell", {
      message: card.text,
    });
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.change(screen.getByLabelText("your words"), {
      target: { value: "Status 7: cancelled." },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText("Saved as what ann@x.io said."),
    ).toBeTruthy();
    expect(post).toHaveBeenCalledWith("/workspaces/shop/proposals/p1/save", {
      text: "Status 7: cancelled.",
      keep: [0],
      reviewer: null,
    });
  });

  it("a lone link goes to the inbox, not to the model", async () => {
    mockChat();
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const post = vi
      .spyOn(client, "apiPost")
      .mockResolvedValue({ name: "wiki.html", status: "accepted" } as never);
    renderAt("/w/shop/chat", <Chat />, ann);
    await screen.findByText(/runs locally/);
    fireEvent.change(screen.getByLabelText("message"), {
      target: { value: "https://wiki.example/p" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(
      await screen.findByText(/Stored wiki.html in the inbox/),
    ).toBeTruthy();
    expect(post).toHaveBeenCalledWith("/workspaces/shop/inbox", {
      url: "https://wiki.example/p",
      reviewer: null,
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
