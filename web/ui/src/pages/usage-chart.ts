// RFC 0252: rows per month of one code value, as the latest `ekos profile run` counted them.

import type { MonthCount } from "../api/types";

export interface Bar {
  month: string;
  /** Rows that month; null when fewer than `min_cell` rows held the value (count withheld). */
  rows: number | null;
  /** No row that month. */
  empty: boolean;
}

/** `YYYY-MM` → months since year 0. */
function index(month: string): number {
  const [y, m] = month.split("-").map(Number);
  return y * 12 + (m - 1);
}

function label(i: number): string {
  const y = Math.floor(i / 12);
  const m = (i % 12) + 1;
  return `${String(y).padStart(4, "0")}-${String(m).padStart(2, "0")}`;
}

/** Every month from the first to the last of `series` (at most `max`, the latest), gaps empty. */
export function bars(series: MonthCount[], max = 60): Bar[] {
  const known = series.filter((m) => /^\d{4}-\d{2}/.test(m.month));
  if (known.length === 0) return [];
  const by = new Map(known.map((m) => [m.month.slice(0, 7), m.rows]));
  const months = known.map((m) => index(m.month));
  const last = Math.max(...months);
  const first = Math.max(Math.min(...months), last - max + 1);
  const out: Bar[] = [];
  for (let i = first; i <= last; i++) {
    const month = label(i);
    const has = by.has(month);
    out.push({ month, rows: has ? (by.get(month) ?? null) : 0, empty: !has });
  }
  return out;
}

/** The tallest counted month (1 at least, so a chart of withheld months still draws). */
export function peak(b: Bar[]): number {
  return Math.max(1, ...b.map((x) => x.rows ?? 0));
}

/** One sentence for a screen reader: span, peak, withheld months. */
export function describe(b: Bar[], minCell: number): string {
  if (b.length === 0) return "no dated rows";
  const withheld = b.filter((x) => !x.empty && x.rows === null).length;
  const top = b.reduce((a, x) => ((x.rows ?? 0) > (a.rows ?? 0) ? x : a), b[0]);
  const parts = [`${b[0].month} to ${b[b.length - 1].month}`];
  if ((top.rows ?? 0) > 0) parts.push(`most rows in ${top.month} (${top.rows})`);
  if (withheld > 0) parts.push(`${withheld} month(s) with fewer than ${minCell} rows`);
  return parts.join(", ");
}
