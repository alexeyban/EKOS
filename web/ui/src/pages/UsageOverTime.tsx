// RFC 0252 §3.7: code usage over time — rows per month of one code value (or of every value no
// code names), from the latest `ekos profile run`. Counts only; a month held by fewer than the
// run's `min_cell` rows is drawn without its count. When the code's history is known, a marker
// shows the month the code first named the value (RFC 0252 §3.2(b)).

import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type { ProfileColumn, ProfileShow } from "../api/types";
import { type Bar, bars, describe, peak } from "./usage-chart";

const H = 60;
const W = 6;

function Chart({
  b,
  minCell,
  marker,
  label,
}: {
  b: Bar[];
  minCell: number;
  marker?: string;
  label: string;
}) {
  const top = peak(b);
  const at = marker ? b.findIndex((x) => x.month === marker.slice(0, 7)) : -1;
  return (
    <svg
      className="usage-chart"
      role="img"
      aria-label={`${label}: ${describe(b, minCell)}`}
      viewBox={`0 0 ${b.length * W} ${H}`}
      preserveAspectRatio="none"
      width="100%"
      height={H}
    >
      {b.map((x, i) => {
        if (x.empty) return null;
        const counted = x.rows !== null;
        const h = counted ? Math.max(1, ((x.rows as number) / top) * (H - 4)) : 3;
        return (
          <rect
            key={x.month}
            x={i * W + 0.5}
            y={H - h}
            width={W - 1}
            height={h}
            className={counted ? "usage-bar" : "usage-bar withheld"}
          >
            <title>{`${x.month}: ${counted ? `${x.rows} row(s)` : `fewer than ${minCell} rows`}`}</title>
          </rect>
        );
      })}
      {at >= 0 && (
        <line x1={at * W} x2={at * W} y1={0} y2={H} className="usage-code-mark">
          <title>{`code names it from ${marker}`}</title>
        </line>
      )}
    </svg>
  );
}

function series(c: ProfileColumn, value: string | null) {
  const m = c.coverage.monthly;
  if (!m) return null;
  return value === null ? (m.other ?? []) : (m.literals[value] ?? []);
}

/** Rows per month of `value` in `table.column` (`value` null: the values no code names). */
export function UsageOverTime({
  workspace,
  table,
  column,
  value,
}: {
  workspace: string;
  table: string;
  column: string;
  value: string | null;
}) {
  const q = useQuery({
    queryKey: ["profile", workspace, table],
    queryFn: () =>
      api<ProfileShow>(`/workspaces/${workspace}/profile?table=${encodeURIComponent(table)}`),
  });
  if (q.isLoading || q.isError || !q.data?.measured) return null;
  const cols = (q.data.columns ?? []).filter(
    (c) => c.column.toLowerCase() === column.toLowerCase() && c.coverage.monthly,
  );
  if (cols.length === 0) return null;
  return (
    <div className="usage-over-time">
      <strong className="sem-sub">Usage over time</strong>
      {cols.map((c) => {
        const s = series(c, value);
        const b = s ? bars(s) : [];
        const hist = value !== null ? c.code_history?.[value] : undefined;
        const fresh = value === null ? c.values_no_code_names : undefined;
        const minCell = c.coverage.monthly?.min_cell ?? 0;
        return (
          <div key={`${c.database}.${c.owner}`}>
            <p className="muted">
              {c.database}.{c.owner}.{c.table}
              {c.time_column ? `, dated by ${c.time_column}` : ""}
              {b.length === 0 ? " — no dated rows" : ""}
            </p>
            {b.length > 0 && (
              <Chart
                b={b}
                minCell={minCell}
                marker={hist?.introduced}
                label={value === null ? "values no code names" : `value ${value}`}
              />
            )}
            {hist && (
              <p className="muted">
                Code names it since {hist.introduced.slice(0, 10)} ({hist.path}:{hist.line});
                the data first holds it in {hist.data_first_seen}
                {hist.order === "data_before_code" ? " — before the code knew it." : "."}
              </p>
            )}
            {fresh?.new && (
              <p className="muted">
                Every one of these values first appeared after the code last changed
                {fresh.code_last_changed ? ` (${fresh.code_last_changed.date.slice(0, 10)})` : ""}.
              </p>
            )}
          </div>
        );
      })}
      <p className="muted">Bars are rows per month; a short grey bar is a month with rows but fewer than the reporting threshold.</p>
    </div>
  );
}
