// Minimal fetch wrapper. The console token lives in localStorage for the skeleton; Phase 3
// replaces this with real users + a read/write role split.

const TOKEN_KEY = "ekos-console-token";

export function getToken(): string {
  try {
    return localStorage.getItem(TOKEN_KEY) ?? "";
  } catch {
    return "";
  }
}

export function setToken(value: string): void {
  try {
    localStorage.setItem(TOKEN_KEY, value);
  } catch {
    /* private mode / storage disabled — the request will just 401 */
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";

  const res = await fetch(`/api${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    let detail = `${res.status} ${res.statusText}`;
    try {
      const j = await res.json();
      if (j?.detail) detail = typeof j.detail === "string" ? j.detail : JSON.stringify(j.detail);
    } catch {
      /* keep the status line */
    }
    throw new Error(`${detail} — ${method} /api${path}`);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export const api = <T>(path: string) => request<T>("GET", path);
export const apiPost = <T>(path: string, body: unknown) => request<T>("POST", path, body);
export const apiDelete = (path: string) => request<void>("DELETE", path);
