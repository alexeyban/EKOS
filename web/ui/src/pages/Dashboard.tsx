import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { api } from "../api/client";
import type { Doctor, KindCount, QueryStats, Stats, Timeline, XrefSummary } from "../api/types";
import { relativeTime } from "../format";
import { Empty, PageHead, QueryView } from "../ui";
import { useChartColors } from "../useChartColors";
import { pct } from "./integrity-shared";

function useWs<T>(id: string, key: string, suffix: string) {
  return useQuery({
    queryKey: ["ws", id, key],
    queryFn: () => api<T>(`/workspaces/${id}${suffix}`),
    enabled: id !== "",
  });
}

const DOCTOR_CHIP: Record<string, string> = { ok: "chip ok", warn: "chip warn", fail: "chip bad" };

export function Dashboard() {
  const { id = "" } = useParams();
  const stats = useWs<Stats>(id, "stats", "/stats");
  const doctor = useWs<Doctor>(id, "health", "/health");
  const kinds = useWs<KindCount[]>(id, "kinds", "/stats/kinds");
  const timeline = useWs<Timeline>(id, "timeline", "/stats/timeline?bucket=day");
  const queries = useWs<QueryStats>(id, "queries", "/stats/queries");
  // RFC 0233: the integrity index's headline numbers; absent until the first commit writes it.
  const integrity = useWs<XrefSummary>(id, "integrity", "/integrity?summary=true");
  const c = useChartColors();
  const axis = { fontSize: 11, fill: c.text };
  const tooltip = {
    contentStyle: { background: c.surface, border: `1px solid ${c.grid}`, borderRadius: 8, fontSize: 12 },
    labelStyle: { color: c.text },
  };

  const compiled = stats.data !== undefined && stats.data.objects > 0;

  return (
    <>
      <PageHead
        title="Dashboard"
        sub={
          stats.data ? (
            <>
              Ledger on <code>{stats.data.backend}</code>
              {stats.data.last_write && <> · last write {relativeTime(stats.data.last_write)}</>}
            </>
          ) : (
            "What this project's ledger holds and how it is doing."
          )
        }
      />

      <QueryView q={stats} what="ledger statistics" tiles={4}>
        {(s) =>
          s.objects === 0 ? (
            <Empty
              title="Nothing is compiled yet"
              action={
                <Link className="button primary" to={`/w/${id}/run`}>
                  Init and compile
                </Link>
              }
            >
              Run the pipeline once to read the sources and fill the ledger. The numbers and charts
              appear here after the first commit.
            </Empty>
          ) : (
            <section className="tiles" aria-label="ledger">
              <Tile label="entries" value={s.entries} />
              <Tile label="objects" value={s.objects} />
              <Tile label="relationships" value={s.relationships} />
              <Tile label="evidence" value={s.evidence ?? "—"} />
            </section>
          )
        }
      </QueryView>

      {integrity.data && (
        <section className="tiles" aria-label="integrity">
          <Tile label="reference resolution" value={pct(integrity.data.metrics.resolution_rate)} />
          <Tile
            label="cross-system resolution"
            value={pct(integrity.data.metrics.cross_system_resolution_rate)}
          />
          <Tile
            label="integrity errors"
            value={integrity.data.by_severity.error ?? 0}
            to={`/w/${id}/integrity`}
          />
          <Tile label="module cycles" value={integrity.data.metrics.cycles ?? 0} />
        </section>
      )}

      {compiled && (
        <div className="grid2">
          <Card title="Growth (cumulative, by day)">
            <QueryView
              q={timeline}
              what="growth"
              rows={5}
              isEmpty={(t) => t.points.length === 0}
              empty={<p className="muted">No commits to chart yet.</p>}
            >
              {(t) => (
                <ResponsiveContainer width="100%" height={220}>
                  <AreaChart data={t.points}>
                    <CartesianGrid stroke={c.grid} vertical={false} />
                    <XAxis dataKey="t" tick={axis} stroke={c.grid} />
                    <YAxis tick={axis} stroke={c.grid} width={44} />
                    <Tooltip {...tooltip} />
                    <Area type="monotone" dataKey="objects" stroke={c.a} fill={c.a} fillOpacity={0.22} />
                    <Area type="monotone" dataKey="relationships" stroke={c.b} fill={c.b} fillOpacity={0.18} />
                  </AreaChart>
                </ResponsiveContainer>
              )}
            </QueryView>
          </Card>

          <Card title="Objects by kind">
            <QueryView
              q={kinds}
              what="objects by kind"
              rows={5}
              isEmpty={(k) => k.length === 0}
              empty={<p className="muted">No objects yet.</p>}
            >
              {(k) => (
                <ResponsiveContainer width="100%" height={220}>
                  <BarChart data={k.slice(0, 12)} layout="vertical">
                    <XAxis type="number" tick={axis} stroke={c.grid} />
                    <YAxis type="category" dataKey="kind" width={110} tick={axis} stroke={c.grid} />
                    <Tooltip {...tooltip} cursor={{ fill: c.grid, opacity: 0.4 }} />
                    <Bar dataKey="count" fill={c.a} radius={[0, 4, 4, 0]} />
                  </BarChart>
                </ResponsiveContainer>
              )}
            </QueryView>
          </Card>

          <Card title="Storage">
            {stats.data && (
              <ResponsiveContainer width="100%" height={200}>
                <BarChart data={stats.data.storage.components}>
                  <XAxis dataKey="name" tick={axis} stroke={c.grid} />
                  <YAxis tick={axis} stroke={c.grid} width={54} tickFormatter={(v) => `${(v / 1e6).toFixed(1)}M`} />
                  <Tooltip
                    {...tooltip}
                    cursor={{ fill: c.grid, opacity: 0.4 }}
                    formatter={(v: number) => `${(v / 1e6).toFixed(2)} MB`}
                  />
                  <Bar dataKey="bytes" radius={[4, 4, 0, 0]}>
                    {stats.data.storage.components.map((_, i) => (
                      <Cell key={i} fill={i % 2 ? c.b : c.a} />
                    ))}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            )}
          </Card>

          <Card title="Recent queries">
            <QueryView
              q={queries}
              what="query log"
              rows={4}
              isEmpty={(q) => q.total === 0}
              empty={<p className="muted">No agent has queried this project yet (the MCP query log, RFC 0114).</p>}
            >
              {(q) => (
                <div className="qstats">
                  <p className="muted">
                    {q.total.toLocaleString()} logged · {(q.cache_hit_rate * 100).toFixed(0)}% cache hits · p50{" "}
                    {q.p50_ms} ms · p95 {q.p95_ms} ms
                  </p>
                  <ul className="kv-list">
                    {Object.entries(q.by_tool)
                      .sort((a, b) => b[1] - a[1])
                      .map(([tool, n]) => (
                        <li key={tool}>
                          <code>{tool}</code>
                          <span>{n.toLocaleString()}</span>
                        </li>
                      ))}
                  </ul>
                </div>
              )}
            </QueryView>
          </Card>
        </div>
      )}

      <Card title="Health checks">
        <QueryView q={doctor} what="health checks" rows={4}>
          {(d) => (
            <ul className="checks">
              {d.checks.map((ch) => (
                <li key={ch.name}>
                  <span className={DOCTOR_CHIP[ch.status] ?? "chip"}>{ch.status}</span>{" "}
                  <strong>{ch.name}</strong> <span className="muted">{ch.detail}</span>
                </li>
              ))}
            </ul>
          )}
        </QueryView>
      </Card>
    </>
  );
}

function Tile({ label, value, to }: { label: string; value: number | string; to?: string }) {
  const body = (
    <>
      <div className="tile-value">{typeof value === "number" ? value.toLocaleString() : value}</div>
      <div className="tile-label">{label}</div>
    </>
  );
  return to ? (
    <Link className="tile tile-link" to={to}>
      {body}
    </Link>
  ) : (
    <div className="tile">{body}</div>
  );
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="card">
      <h2 className="card-label">{title}</h2>
      {children}
    </section>
  );
}
