import { afterEach, describe, expect, it, vi } from "vitest";
import { api, ApiError, apiDelete, apiPost, apiPut, logout, tokenLogin } from "./client";

function jsonResponse(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: "",
    json: () => Promise.resolve(body),
  } as Response;
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("api (GET)", () => {
  it("calls the /api-prefixed path with credentials included and no body", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { id: "w1" }));
    vi.stubGlobal("fetch", fetchMock);

    const result = await api<{ id: string }>("/workspaces/w1");

    expect(result).toEqual({ id: "w1" });
    expect(fetchMock).toHaveBeenCalledWith("/api/workspaces/w1", {
      method: "GET",
      headers: {},
      credentials: "include",
      body: undefined,
    });
  });

  it("returns undefined for a 204 No Content response", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(jsonResponse(204, null)));
    expect(await api("/workspaces/w1")).toBeUndefined();
  });

  it("throws ApiError with the server's detail message on a non-OK response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(jsonResponse(404, { detail: "unknown workspace 'w1'" })),
    );

    await expect(api("/workspaces/w1")).rejects.toMatchObject({
      status: 404,
      message: expect.stringContaining("unknown workspace 'w1'"),
    });
  });

  it("falls back to the status line when the error body isn't JSON", async () => {
    const res = {
      ok: false,
      status: 502,
      statusText: "Bad Gateway",
      json: () => Promise.reject(new Error("not json")),
    } as unknown as Response;
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(res));

    await expect(api("/x")).rejects.toMatchObject({
      status: 502,
      message: expect.stringContaining("502 Bad Gateway"),
    });
  });

  it("is an instance of both ApiError and Error", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(jsonResponse(500, { detail: "boom" })));
    try {
      await api("/x");
      expect.unreachable("expected api() to throw");
    } catch (e) {
      expect(e).toBeInstanceOf(ApiError);
      expect(e).toBeInstanceOf(Error);
      expect((e as ApiError).body).toBe("boom"); // client.ts unwraps `detail` before storing it
    }
  });
});

describe("apiPost / apiPut / apiDelete", () => {
  it("apiPost sends a JSON body and defaults it to {}", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { ok: true }));
    vi.stubGlobal("fetch", fetchMock);

    await apiPost("/runs");

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/runs",
      expect.objectContaining({
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: "{}",
      }),
    );
  });

  it("apiPut sends the given body as JSON", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, { written: true }));
    vi.stubGlobal("fetch", fetchMock);

    await apiPut("/workspaces/w1/config", { raw: "[observe]\n" });

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/workspaces/w1/config",
      expect.objectContaining({ method: "PUT", body: JSON.stringify({ raw: "[observe]\n" }) }),
    );
  });

  it("apiDelete sends no body", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(204, null));
    vi.stubGlobal("fetch", fetchMock);

    await apiDelete("/workspaces/w1");

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/workspaces/w1",
      expect.objectContaining({ method: "DELETE", headers: {}, body: undefined }),
    );
  });
});

describe("tokenLogin / logout", () => {
  it("tokenLogin posts the token and returns the granted role", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(jsonResponse(200, { role: "write" })));
    expect(await tokenLogin("secret")).toEqual({ role: "write" });
  });

  it("logout posts with no body", async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(200, {}));
    vi.stubGlobal("fetch", fetchMock);

    await logout();

    expect(fetchMock).toHaveBeenCalledWith("/api/auth/logout", expect.objectContaining({ method: "POST" }));
  });
});
