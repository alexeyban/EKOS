// RFC 0220: a run's structured log (`/runs/<id>/events`) as rows of the run page's steps table.

export type RunEvent = Record<string, unknown> & {
  stage?: string;
  msg?: string;
  cmd?: string;
  t_ms?: number;
};

export interface StepRow {
  cmd: string;
  stage: string;
  msg: string;
  /** The line's counts as `key value`, in the order the CLI wrote them. */
  counts: string;
  tMs: number | null;
  tone: "" | "ok" | "warn" | "bad";
}

const OWN = new Set(["stage", "msg", "cmd", "t_ms"]);

function show(v: unknown): string {
  if (v === null || v === undefined) return "—";
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

function tone(e: RunEvent): StepRow["tone"] {
  if (e.stage === "error" || (e.stage === "run" && e.ok === false)) return "bad";
  if (e.stage === "warning") return "warn";
  // The model-call line: green when the step made none, so "no LLM" is visible at a glance.
  if (e.stage === "llm") return e.count === 0 && e.embeddings === 0 ? "ok" : "warn";
  return "";
}

export function stepRows(events: RunEvent[]): StepRow[] {
  return events.map((e) => ({
    cmd: show(e.cmd ?? ""),
    stage: show(e.stage ?? ""),
    msg: show(e.msg ?? ""),
    counts: Object.entries(e)
      .filter(([k]) => !OWN.has(k))
      .map(([k, v]) => `${k} ${show(v)}`)
      .join(" · "),
    tMs: typeof e.t_ms === "number" ? e.t_ms : null,
    tone: tone(e),
  }));
}

/** Model calls over the whole run: the sum of every command's `llm` line. */
export function modelCalls(events: RunEvent[]): { completions: number; embeddings: number } | null {
  const lines = events.filter((e) => e.stage === "llm");
  if (lines.length === 0) return null;
  const sum = (k: string) => lines.reduce((n, e) => n + (typeof e[k] === "number" ? (e[k] as number) : 0), 0);
  return { completions: sum("count"), embeddings: sum("embeddings") };
}
