// RFC 0253 §3.10 — "What people said" about one entity, on its panel: testimony in the person's
// own words, with who said it, where and when. Read-only. Like the notes beside it, it is not
// asked for by someone who sees only part of the project (the route refuses them) or in a
// time-travel view.

import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type { Testimony } from "../api/types";

export function ObjectTestimony({
  workspace,
  objectId,
  enabled,
}: {
  workspace: string;
  objectId: string;
  enabled: boolean;
}) {
  const said = useQuery({
    queryKey: ["testimony", workspace, "object", objectId],
    queryFn: () =>
      api<Testimony[]>(
        `/workspaces/${workspace}/testimony?about=${encodeURIComponent(objectId)}`,
      ),
    enabled,
  });
  const shown = (said.data ?? []).filter((t) => t.status !== "withdrawn");
  if (!enabled || (!said.isError && shown.length === 0)) return null;
  return (
    <details open>
      <summary>What people said ({shown.length})</summary>
      {said.isError && <p className="err">{String(said.error)}</p>}
      <ul className="rel-list">
        {shown.map((t) => (
          <li key={t.id}>
            <span className="muted">
              {t.author}
              {t.on_behalf_of ? ` for ${t.on_behalf_of}` : ""} ({t.channel},{" "}
              {t.sent_at.slice(0, 10)})
              {t.permalink && (
                <>
                  {" "}
                  <a href={t.permalink} target="_blank" rel="noreferrer">
                    message
                  </a>
                </>
              )}
            </span>
            <div>{t.text}</div>
          </li>
        ))}
      </ul>
    </details>
  );
}
