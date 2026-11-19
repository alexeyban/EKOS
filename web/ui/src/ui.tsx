// RFC 0261 §2.7, the page pass: the pieces every page is built from, so each one has the same
// header, a loading skeleton, an empty state that says what to do next, and an error that names
// the request that failed (the API client puts `GET /api/…` in every error's message).

import type { UseQueryResult } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

/** A page's title row: the title, a short line under it, and the page's own actions on the right. */
export function PageHead({
  title,
  sub,
  actions,
  children,
}: {
  title: ReactNode;
  sub?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <header className="page-head">
      <div>
        <h1 className="page-title">{title}</h1>
        {sub != null && <p className="page-lede">{sub}</p>}
        {children}
      </div>
      {actions != null && <div className="page-actions">{actions}</div>}
    </header>
  );
}

/** Grey bars where content is coming. `rows` lines, or a grid of `tiles` blocks. */
export function Skeleton({ rows = 3, tiles = 0, label = "loading" }: { rows?: number; tiles?: number; label?: string }) {
  if (tiles > 0) {
    return (
      <div className="skeleton-tiles" aria-busy="true" aria-label={label}>
        {Array.from({ length: tiles }, (_, i) => (
          <span key={i} className="skeleton skeleton-tile" />
        ))}
      </div>
    );
  }
  return (
    <div className="skeleton-rows" aria-busy="true" aria-label={label}>
      {Array.from({ length: rows }, (_, i) => (
        <span key={i} className="skeleton skeleton-row" style={{ width: `${92 - ((i * 17) % 40)}%` }} />
      ))}
    </div>
  );
}

/** Nothing to show yet: what this place is for, and the next thing to do about it. */
export function Empty({
  title,
  children,
  action,
  icon = "info",
}: {
  title: ReactNode;
  children?: ReactNode;
  action?: ReactNode;
  icon?: IconName;
}) {
  return (
    <div className="empty">
      <span className="empty-icon" aria-hidden="true">
        <Icon name={icon} size={18} />
      </span>
      <div>
        <p className="empty-title">{title}</p>
        {children != null && <div className="empty-body muted">{children}</div>}
        {action != null && <div className="empty-action">{action}</div>}
      </div>
    </div>
  );
}

/** A failed request, said once, with the request it was (the message carries `METHOD /api/…`). */
export function ErrorNote({ error, what }: { error: unknown; what?: string }) {
  const text = error instanceof Error ? error.message : String(error);
  return (
    <p className="err" role="alert">
      {what ? <strong>{what}: </strong> : null}
      {text}
    </p>
  );
}

/**
 * One query's three states in one place: a skeleton while it loads, its error, then `children`
 * with the data — or `empty` when `isEmpty` says the data holds nothing.
 */
export function QueryView<T>({
  q,
  what,
  rows = 3,
  tiles = 0,
  isEmpty,
  empty,
  children,
}: {
  q: Pick<UseQueryResult<T>, "data" | "error" | "isError" | "isPending">;
  what?: string;
  rows?: number;
  tiles?: number;
  isEmpty?: (data: T) => boolean;
  empty?: ReactNode;
  children: (data: T) => ReactNode;
}) {
  if (q.isError) return <ErrorNote error={q.error} what={what} />;
  if (q.data === undefined) return q.isPending ? <Skeleton rows={rows} tiles={tiles} label={what ? `loading ${what}` : "loading"} /> : null;
  if (isEmpty?.(q.data) && empty != null) return <>{empty}</>;
  return <>{children(q.data)}</>;
}
