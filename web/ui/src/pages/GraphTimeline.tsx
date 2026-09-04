// RFC 0134 — the timelapse slider docked under the graph. Ticks + an activity histogram come
// from `ekos ledger timeline` (day buckets); dragging emits an RFC 3339 instant (or null =
// live). The graph is monotonic, so the canvas just filters `firstSeen <= value` — no refetch.

import { useEffect, useRef, useState } from "react";
import { bucketEnd } from "./graph-shared";

export interface TimelinePoint {
  t: string; // bucket label, e.g. "2026-08-26"
  objects: number; // cumulative
  relationships: number; // cumulative
}

export function GraphTimeline({
  points,
  value,
  onChange,
}: {
  points: TimelinePoint[];
  value: string | null; // null = live / latest
  onChange: (v: string | null) => void;
}) {
  const n = points.length;
  const live = n; // slider index n == "live"; 0..n-1 select a bucket
  const found = points.findIndex((p) => bucketEnd(p.t) === value);
  const idx = value === null ? live : found === -1 ? live : found;

  const [playing, setPlaying] = useState(false);
  const idxRef = useRef(idx);
  idxRef.current = idx;

  const setIdx = (i: number) => {
    const c = Math.max(0, Math.min(live, i));
    onChange(c === live ? null : bucketEnd(points[c].t));
  };

  useEffect(() => {
    if (!playing) return;
    const h = window.setInterval(() => {
      const next = idxRef.current + 1;
      setIdx(next);
      if (next >= live) setPlaying(false);
    }, 700);
    return () => window.clearInterval(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playing, live]);

  if (n === 0) return null;

  const maxDelta = Math.max(
    1,
    ...points.map((p, i) => p.objects - (i > 0 ? points[i - 1].objects : 0)),
  );

  return (
    <div className="graph-timeline">
      <button
        className="gt-play"
        onClick={() => {
          if (idx >= live) setIdx(0);
          setPlaying((p) => !p);
        }}
        title={playing ? "pause" : "play the compile history"}
      >
        {playing ? "❚❚" : "▶"}
      </button>

      <div className="gt-track">
        <div className="gt-hist" aria-hidden>
          {points.map((p, i) => {
            const delta = p.objects - (i > 0 ? points[i - 1].objects : 0);
            return (
              <span
                key={p.t}
                className={idx === live || i <= idx ? "gt-bar on" : "gt-bar"}
                style={{ height: `${8 + 92 * (delta / maxDelta)}%` }}
                title={`${p.t}: +${delta} objects (${p.objects} total)`}
              />
            );
          })}
        </div>
        <input
          type="range"
          min={0}
          max={live}
          step={1}
          value={idx}
          aria-label="graph as of"
          onChange={(e) => {
            setPlaying(false);
            setIdx(Number(e.target.value));
          }}
        />
      </div>

      <span className="gt-label mono">{idx >= live ? "live" : points[idx].t}</span>
      {idx < live && (
        <button className="gt-latest" onClick={() => setIdx(live)} title="jump to latest">
          ⤒ latest
        </button>
      )}
    </div>
  );
}
