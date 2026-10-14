import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import { type ArgSpec, type CommandEntry, Commands } from "./Commands";

const arg = (id: string, over: Partial<ArgSpec> = {}): ArgSpec => ({
  id,
  long: id,
  index: null,
  kind: "value",
  multiple: false,
  required: false,
  possible: [],
  default: [],
  value_name: null,
  help: "",
  ...over,
});

const COMMANDS: CommandEntry[] = [
  {
    path: ["query", "find"],
    about: "Search the ledger",
    class: "read",
    metered: false,
    allowed: true,
    confirm: false,
    args: [
      arg("text", { long: null, index: 1, required: true, value_name: "TEXT", help: "what to look for" }),
      arg("mode", { possible: ["lexical", "hybrid"], default: ["lexical"] }),
      arg("json", { kind: "flag" }),
      arg("paths", { multiple: true, kind: "path" }),
      arg("limit", { default: ["10"] }),
    ],
  },
  {
    path: ["semantics", "confirm"],
    about: "Confirm a definition",
    class: "review",
    metered: false,
    allowed: false,
    confirm: false,
    args: [arg("id", { long: null, index: 1 }), arg("by")],
  },
  {
    path: ["clean"],
    about: "Clear the artifact cache",
    class: "admin",
    metered: false,
    allowed: true,
    confirm: true,
    args: [],
  },
  {
    path: ["docs", "generate"],
    about: "Generate docs",
    class: "write",
    metered: true,
    allowed: true,
    confirm: false,
    args: [],
  },
];

function renderCommands() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/w/shop/commands"]}>
        <Routes>
          <Route path="/w/:id/commands" element={<Commands />} />
          <Route path="/runs/:runId" element={<p>run page</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

describe("Commands page", () => {
  it("lists commands by group and filters them", async () => {
    vi.spyOn(client, "api").mockResolvedValue({ commands: COMMANDS });
    renderCommands();
    expect(await screen.findByRole("button", { name: "query find" })).toBeTruthy();
    expect(screen.getByText("semantics")).toBeTruthy();
    expect(screen.getByText("project")).toBeTruthy(); // a one-word command
    expect(screen.getByText(/not for you/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("filter commands"), { target: { value: "cache" } });
    expect(screen.queryByRole("button", { name: "query find" })).toBeNull();
    expect(screen.getByRole("button", { name: "clean" })).toBeTruthy();
  });

  it("builds a form from the arguments, previews the command and runs it", async () => {
    vi.spyOn(client, "api").mockResolvedValue({ commands: COMMANDS });
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({ run_id: "r1" });
    renderCommands();
    fireEvent.click(await screen.findByRole("button", { name: "query find" }));
    expect(screen.getByText("what to look for")).toBeTruthy();
    expect(screen.getByText(/reads the project/)).toBeTruthy();
    fireEvent.change(document.getElementById("arg-text")!, { target: { value: "customers" } });
    fireEvent.change(document.getElementById("arg-mode")!, { target: { value: "hybrid" } });
    fireEvent.click(document.getElementById("arg-json")!);
    fireEvent.change(document.getElementById("arg-paths")!, { target: { value: "sql/a.sql\n\nsql/b.sql" } });
    expect((document.getElementById("arg-limit") as HTMLInputElement).placeholder).toBe("default: 10");
    expect(screen.getByText("ekos query find --mode=hybrid --json --paths=sql/a.sql --paths=sql/b.sql customers")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() =>
      expect(post).toHaveBeenCalledWith("/workspaces/shop/cli", {
        command: ["query", "find"],
        args: { text: "customers", mode: "hybrid", json: true, paths: ["sql/a.sql", "sql/b.sql"] },
        confirm: null,
      }),
    );
    expect(await screen.findByText("run page")).toBeTruthy();
  });

  it("a review command hides the reviewer field and is refused without the role", async () => {
    vi.spyOn(client, "api").mockResolvedValue({ commands: COMMANDS });
    renderCommands();
    fireEvent.click(await screen.findByRole("button", { name: "semantics confirm" }));
    expect(document.getElementById("arg-by")).toBeNull();
    expect(screen.getByText(/recorded under your name/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Run" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/does not allow this command/)).toBeTruthy();
  });

  it("a destructive command needs its name typed", async () => {
    vi.spyOn(client, "api").mockResolvedValue({ commands: COMMANDS });
    const post = vi.spyOn(client, "apiPost").mockRejectedValue(new Error("the run queue is full"));
    renderCommands();
    fireEvent.click(await screen.findByRole("button", { name: "clean" }));
    const run = screen.getByRole("button", { name: "Run" }) as HTMLButtonElement;
    expect(run.disabled).toBe(true);
    fireEvent.change(document.getElementById("confirm")!, { target: { value: "clean" } });
    expect(run.disabled).toBe(false);
    fireEvent.click(run);
    await waitFor(() => expect(post).toHaveBeenCalledWith("/workspaces/shop/cli", { command: ["clean"], args: {}, confirm: "clean" }));
    expect(await screen.findByText(/the run queue is full/)).toBeTruthy();
  });

  it("warns before a command that may call a paid model", async () => {
    vi.spyOn(client, "api").mockResolvedValue({ commands: COMMANDS });
    renderCommands();
    fireEvent.click(await screen.findByRole("button", { name: "docs generate" }));
    expect(screen.getByText(/A hosted model is paid per call/)).toBeTruthy();
  });

  it("shows why the list could not load", async () => {
    vi.spyOn(client, "api").mockRejectedValue(new Error("403 restricted"));
    renderCommands();
    expect(await screen.findByText(/403 restricted/)).toBeTruthy();
  });
});
