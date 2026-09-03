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
import type { Doctor, KindCount, QueryStats, Stats, Timeline } from "../api/types";

const CHART = "#7c3aed";
const CHART_B = "#059669";

function useWs<T>(id: string, key: string, suffix: string) {
  return useQuery({
    queryKey: ["ws", id, key],
    queryFn: () => api<T>(`/workspaces/${id}${suffix}`),
    enabled: id !== "",
  });
}

export function Dashboard() {
  const { id = "" } = useParams();
  const stats = useWs<Stats>(id, "stats", "/stats");
  const doctor = useWs<Doctor>(id, "health", "/health");
  const kinds = useWs<KindCount[]>(id, "kinds", "/stats/kinds");
  const timeline = useWs<Timeline>(id, "timeline", "/stats/timeline?bucket=day");
  const queries = useWs<QueryStats>(id, "queries", "/stats/queries");

  return (
    <>
      <p>
        <Link to="/" className="linkish">
          ← workspaces
        </Link>
      </p>

      {stats.isError && <p className="err">{String(stats.error)}</p>}

      {stats.data && (
        <section className="tiles">
          <Tile label="entries" value={stats.data.entries} />
          <Tile label="objects" value={stats.data.objects} />
          <Tile label="relationships" value={stats.data.relationships} />
          <Tile label="evidence" value={stats.data.evidence ?? "—"} />
        </section>
      )}
      {stats.data && (
        <p className="muted">
          backend <code>{stats.data.backend}</code>
          {stats.data.last_write && <> · last write {stats.data.last_write.slice(0, 10)}</>}
        </p>
      )}

      <div className="grid2">
        <Card title="Growth (cumulative, by day)">
          {timeline.data && timeline.data.points.length > 0 ? (
            <ResponsiveContainer width="100%" height={220}>
              <AreaChart data={timeline.data.points}>
                <CartesianGrid strokeOpacity={0.15} />
                <XAxis dataKey="t" fontSize={11} />
                <YAxis fontSize={11} width={44} />
                <Tooltip />
                <Area
                  type="monotone"
                  dataKey="objects"
                  stroke={CHART}
                  fill={CHART}
                  fillOpacity={0.25}
                />
                <Area
                  type="monotone"
                  dataKey="relationships"
                  stroke={CHART_B}
                  fill={CHART_B}
                  fillOpacity={0.2}
                />
              </AreaChart>
            </ResponsiveContainer>
          ) : (
            <Empty q={timeline} />
          )}
        </Card>

        <Card title="Objects by kind">
          {kinds.data && kinds.data.length > 0 ? (
            <ResponsiveContainer width="100%" height={220}>
              <BarChart data={kinds.data.slice(0, 12)} layout="vertical">
                <XAxis type="number" fontSize={11} />
                <YAxis type="category" dataKey="kind" width={110} fontSize={11} />
                <Tooltip />
                <Bar dataKey="count" fill={CHART} />
              </BarChart>
            </ResponsiveContainer>
          ) : (
            <Empty q={kinds} />
          )}
        </Card>

        <Card title="Storage">
          {stats.data ? (
            <ResponsiveContainer width="100%" height={200}>
              <BarChart data={stats.data.storage.components}>
                <XAxis dataKey="name" fontSize={11} />
                <YAxis
                  fontSize={11}
                  width={54}
                  tickFormatter={(v) => `${(v / 1e6).toFixed(1)}M`}
                />
                <Tooltip formatter={(v: number) => `${(v / 1e6).toFixed(2)} MB`} />
                <Bar dataKey="bytes">
                  {stats.data.storage.components.map((_, i) => (
                    <Cell key={i} fill={i % 2 ? CHART_B : CHART} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          ) : (
            <Empty q={stats} />
          )}
        </Card>

        <Card title="Recent queries (RFC 0114 log)">
          {queries.data ? (
            <div className="qstats">
              <p className="muted">
                {queries.data.total} logged · {(queries.data.cache_hit_rate * 100).toFixed(0)}% cache
                hits · p50 {queries.data.p50_ms}ms · p95 {queries.data.p95_ms}ms
              </p>
              <ul>
                {Object.entries(queries.data.by_tool).map(([tool, n]) => (
                  <li key={tool}>
                    <code>{tool}</code> — {n}
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <Empty q={queries} />
          )}
        </Card>
      </div>

      <Card title="doctor">
        {doctor.data ? (
          <ul className="checks">
            {doctor.data.checks.map((c) => (
              <li key={c.name}>
                <span className={`dot ${c.status}`} /> <strong>{c.name}</strong>{" "}
                <span className="muted">{c.detail}</span>
              </li>
            ))}
          </ul>
        ) : (
          <Empty q={doctor} />
        )}
      </Card>
    </>
  );
}

function Tile({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="tile">
      <div className="tile-value">{typeof value === "number" ? value.toLocaleString() : value}</div>
      <div className="tile-label">{label}</div>
    </div>
  );
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="card">
      <strong>{title}</strong>
      {children}
    </section>
  );
}

function Empty({ q }: { q: { isLoading: boolean; isError: boolean; error: unknown } }) {
  if (q.isError) return <p className="err">{String(q.error)}</p>;
  if (q.isLoading) return <p className="muted">loading…</p>;
  return <p className="muted">no data</p>;
}
