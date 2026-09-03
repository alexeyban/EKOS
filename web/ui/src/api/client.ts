// Minimal fetch wrapper. The console token lives in localStorage for the skeleton; Phase 1
// replaces this with a real session.

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

export async function api<T>(path: string): Promise<T> {
  const headers: Record<string, string> = {};
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;

  const res = await fetch(`/api${path}`, { headers });
  if (!res.ok) {
    throw new Error(`${res.status} ${res.statusText} — GET /api${path}`);
  }
  return res.json() as Promise<T>;
}
