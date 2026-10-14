import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as client from "../api/client";
import { Settings, type SettingField } from "./Settings";

const field = (over: Partial<SettingField>): SettingField => ({
  name: "port",
  group: "Server",
  help: "",
  secret: false,
  editable: true,
  source: "default",
  set: false,
  value: 8000,
  type: "int",
  ...over,
});

const FIELDS: SettingField[] = [
  field({ name: "port", group: "Server", value: 8000, type: "int", help: "Port to listen on" }),
  field({ name: "mode", group: "Server", value: "local", type: "str" }),
  field({ name: "production", group: "Server", value: false, type: "bool" }),
  field({ name: "owners", group: "Sign-in", value: "a@b.c", type: "str", source: "environment", editable: false }),
  field({ name: "oidc_client_secret", group: "Sign-in", secret: true, set: true, source: "keychain", value: null, type: "str" }),
];

interface World {
  supervised?: boolean;
  file?: string | null;
  error?: string;
  keychain?: boolean;
  names?: string[];
  uninstall?: { available: boolean; reason: string | null };
}

function mockApi(w: World = {}) {
  return vi.spyOn(client, "api").mockImplementation((path: string) => {
    if (path === "/platform/settings")
      return Promise.resolve({
        fields: FIELDS,
        file: w.file === undefined ? "/home/me/.ekos/control/console.toml" : w.file,
        error: w.error ?? "",
        keychain: { available: w.keychain ?? true, backend: "SecretService" },
        supervised: w.supervised ?? true,
      }) as never;
    if (path === "/platform/keychain")
      return Promise.resolve({ available: w.keychain ?? true, backend: "SecretService", names: w.names ?? [] }) as never;
    if (path === "/platform/uninstall")
      return Promise.resolve(w.uninstall ?? { available: false, reason: "only in local mode" }) as never;
    return Promise.reject(new Error(`unexpected ${path}`));
  });
}

function renderSettings() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Settings />
    </QueryClientProvider>,
  );
}

afterEach(() => vi.restoreAllMocks());

describe("Settings page", () => {
  it("shows every group, its fields and where each value comes from", async () => {
    mockApi();
    renderSettings();
    expect(await screen.findByText("Console settings")).toBeTruthy();
    expect(screen.getByText("Server")).toBeTruthy();
    expect(screen.getByText("Sign-in")).toBeTruthy();
    expect(screen.getByText("Port to listen on")).toBeTruthy();
    expect(screen.getByText("/home/me/.ekos/control/console.toml")).toBeTruthy();
    // A value set by the environment is shown but cannot be changed.
    expect((screen.getByLabelText("owners") as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText("environment")).toBeTruthy();
    // A choice field is a select; a bool a checkbox.
    expect(screen.getByLabelText("mode").tagName).toBe("SELECT");
    expect((screen.getByLabelText("production") as HTMLInputElement).type).toBe("checkbox");
  });

  it("saves only what changed, typed, then offers a restart", async () => {
    mockApi();
    const put = vi.spyOn(client, "apiPut").mockResolvedValue({});
    const post = vi.spyOn(client, "apiPost").mockResolvedValue({});
    renderSettings();
    await screen.findByText("Console settings");
    const save = screen.getByRole("button", { name: "Save settings" }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);

    fireEvent.change(screen.getByLabelText("port"), { target: { value: "9000" } });
    fireEvent.click(screen.getByLabelText("production"));
    expect(screen.getByText("2 changed")).toBeTruthy();
    fireEvent.click(save);
    await waitFor(() =>
      expect(put).toHaveBeenCalledWith("/platform/settings", {
        values: { port: 9000, production: true },
        secrets: {},
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Restart now" }));
    await waitFor(() => expect(post).toHaveBeenCalledWith("/platform/restart"));
    expect(await screen.findByText(/Restarting/)).toBeTruthy();
  });

  it("without a launcher, says to restart by hand", async () => {
    mockApi({ supervised: false });
    vi.spyOn(client, "apiPut").mockResolvedValue({});
    renderSettings();
    await screen.findByText("Console settings");
    fireEvent.change(screen.getByLabelText("port"), { target: { value: "9001" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(await screen.findByText(/restart it yourself/)).toBeTruthy();
  });

  it("a secret is typed into the keychain, never shown, and can be removed", async () => {
    mockApi();
    const put = vi.spyOn(client, "apiPut").mockResolvedValue({});
    renderSettings();
    await screen.findByText("Console settings");
    const secret = screen.getByLabelText("oidc_client_secret") as HTMLInputElement;
    expect(secret.type).toBe("password");
    expect(secret.placeholder).toMatch(/set/);
    fireEvent.change(secret, { target: { value: "s3cret" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() =>
      expect(put).toHaveBeenCalledWith("/platform/settings", { values: {}, secrets: { oidc_client_secret: "s3cret" } }),
    );
    fireEvent.click(screen.getByRole("button", { name: "remove" }));
    await waitFor(() =>
      expect(put).toHaveBeenCalledWith("/platform/settings", { secrets: { oidc_client_secret: null } }),
    );
  });

  it("without a keychain, a secret points to the environment", async () => {
    mockApi({ keychain: false });
    renderSettings();
    expect(await screen.findByText(/no keychain here; use an environment variable/)).toBeTruthy();
    expect(await screen.findByText(/No keychain is usable here/)).toBeTruthy();
  });

  it("without a home, says the settings come from the environment, and shows a broken file", async () => {
    mockApi({ file: null, error: "port must be a number" });
    renderSettings();
    expect(await screen.findByText(/has no EKOS home/)).toBeTruthy();
    expect(screen.getByText(/The saved settings do not start: port must be a number/)).toBeTruthy();
  });

  it("a save the server refuses shows its reason", async () => {
    mockApi();
    vi.spyOn(client, "apiPut").mockRejectedValue(new Error("port: not a number"));
    renderSettings();
    await screen.findByText("Console settings");
    fireEvent.change(screen.getByLabelText("port"), { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(await screen.findByText(/port: not a number/)).toBeTruthy();
  });

  it("stores and removes a model key in the keychain", async () => {
    mockApi({ names: ["ANTHROPIC_API_KEY"] });
    const put = vi.spyOn(client, "apiPut").mockResolvedValue({});
    const del = vi.spyOn(client, "apiDelete").mockResolvedValue(undefined);
    renderSettings();
    expect(await screen.findByText("ANTHROPIC_API_KEY")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("variable name"), { target: { value: " OPENAI_API_KEY " } });
    fireEvent.change(screen.getByLabelText("value"), { target: { value: "sk-1" } });
    fireEvent.click(screen.getByRole("button", { name: "Store" }));
    await waitFor(() => expect(put).toHaveBeenCalledWith("/platform/keychain/OPENAI_API_KEY", { value: "sk-1" }));
    fireEvent.click(screen.getAllByRole("button", { name: "remove" }).at(-1)!);
    await waitFor(() => expect(del).toHaveBeenCalledWith("/platform/keychain/ANTHROPIC_API_KEY"));
  });

  it("uninstall: the reason when unavailable, the page on this machine when available", async () => {
    mockApi({ uninstall: { available: false, reason: "only in local mode" } });
    const first = renderSettings();
    expect(await screen.findByText("only in local mode")).toBeTruthy();
    first.unmount();
    vi.restoreAllMocks();

    mockApi({ uninstall: { available: true, reason: null } });
    const assign = vi.fn();
    vi.stubGlobal("location", { ...window.location, assign });
    vi.spyOn(client, "apiPost").mockResolvedValue({ url: "http://127.0.0.1:41234/?t=abc" });
    renderSettings();
    fireEvent.click(await screen.findByRole("button", { name: "Uninstall EKOS…" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith("http://127.0.0.1:41234/?t=abc"));
    vi.unstubAllGlobals();
  });

  it("uninstall: an address off this machine is not followed", async () => {
    mockApi({ uninstall: { available: true, reason: null } });
    vi.spyOn(client, "apiPost").mockResolvedValue({ url: "http://evil.example/?t=x" });
    renderSettings();
    fireEvent.click(await screen.findByRole("button", { name: "Uninstall EKOS…" }));
    expect(await screen.findByText(/off this machine; not opened/)).toBeTruthy();
  });

  it("an unreadable settings page shows the error", async () => {
    vi.spyOn(client, "api").mockRejectedValue(new Error("403 forbidden"));
    renderSettings();
    expect(await screen.findByText(/403 forbidden/)).toBeTruthy();
  });
});
