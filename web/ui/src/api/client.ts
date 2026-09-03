// Session-cookie auth (RFC 0131). The browser authenticates once — OIDC redirect, or a
// token-login in the fallback mode — and every subsequent request carries the signed session
// cookie (so `EventSource`, which can't set headers, works for the SSE log stream).

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
    public body?: unknown,
  ) {
    super(message);
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["Content-Type"] = "application/json";

  const res = await fetch(`/api${path}`, {
    method,
    headers,
    credentials: "include",
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    let detail: unknown = `${res.status} ${res.statusText}`;
    try {
      const j = await res.json();
      detail = j?.detail ?? detail;
    } catch {
      /* keep the status line */
    }
    const msg = typeof detail === "string" ? detail : JSON.stringify(detail);
    throw new ApiError(res.status, `${msg} — ${method} /api${path}`, detail);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export const api = <T>(path: string) => request<T>("GET", path);
export const apiPost = <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {});
export const apiPut = <T>(path: string, body: unknown) => request<T>("PUT", path, body);
export const apiDelete = (path: string) => request<void>("DELETE", path);

export const tokenLogin = (token: string) => apiPost<{ role: string }>("/auth/token-login", { token });
export const logout = () => apiPost("/auth/logout");
