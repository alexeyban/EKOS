// RFC 0247 — what people wrote about one entity, on the entity's panel. Read-only: writing is on
// the Notes tab. A reader who sees only part of a project is refused the annotations route, so the
// panel does not ask; a time-travel view has no annotations (they describe the entity now).

import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import type { Annotation } from "../api/types";
import { reach, stateChip } from "./annotations-shared";

export function ObjectNotes({
  workspace,
  objectId,
  enabled,
}: {
  workspace: string;
  objectId: string;
  enabled: boolean;
}) {
  const notes = useQuery({
    queryKey: ["annotations", workspace, "object", objectId],
    queryFn: () => api<Annotation[]>(`/workspaces/${workspace}/annotations?target=${objectId}`),
    enabled,
  });
  const shown = (notes.data ?? []).filter((a) => a.state !== "withdrawn");
  if (!enabled || (!notes.isError && shown.length === 0)) return null;
  return (
    <details open>
      <summary>Notes from people ({shown.length})</summary>
      {notes.isError && <p className="err">{String(notes.error)}</p>}
      <ul className="rel-list">
        {shown.map((a) => (
          <li key={a.id}>
            <span className={stateChip(a.state)}>{a.state}</span>{" "}
            <span className="muted">
              {a.kind}
              {a.term ? ` “${a.term}”` : ""} by {a.author} · {reach(a)}
            </span>
            <div>{a.text}</div>
          </li>
        ))}
      </ul>
    </details>
  );
}
