// RFC 0253 §3.5–3.6 — a proposal card in the chat: what the model understood from a statement,
// each line quoting the person's own words. Only the person saves it, with their own text or
// their edit of it; a line they untick is not saved. The model wrote none of what is recorded.

import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { apiPost } from "../api/client";
import type { ProposalCard } from "../api/types";

export function TellCard({
  workspace,
  card,
  reviewer,
}: {
  workspace: string;
  card: ProposalCard;
  reviewer: string | null;
}) {
  const [keep, setKeep] = useState<boolean[]>(card.items.map(() => true));
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(card.text);
  const [state, setState] = useState<"open" | "saved" | "discarded">("open");
  const base = `/workspaces/${workspace}/proposals/${card.proposal}`;
  const save = useMutation({
    mutationFn: () =>
      apiPost<{ by: string }>(`${base}/save`, {
        text: text.trim() === card.text ? null : text.trim(),
        keep: keep.flatMap((k, n) => (k ? [n] : [])),
        reviewer,
      }),
    onSuccess: () => setState("saved"),
  });
  const discard = useMutation({
    mutationFn: () => apiPost(`${base}/discard`),
    onSuccess: () => setState("discarded"),
  });

  return (
    <div className="card tell-card" aria-label="proposal card">
      <strong>I understood:</strong>
      {card.items.length === 0 && (
        <p className="muted">
          Nothing to link it to; saving keeps your words alone.
        </p>
      )}
      <ul className="rel-list">
        {card.lines.map((line, n) => (
          <li key={n}>
            <label>
              <input
                type="checkbox"
                checked={keep[n]}
                disabled={state !== "open"}
                onChange={(e) =>
                  setKeep((k) =>
                    k.map((v, i) => (i === n ? e.target.checked : v)),
                  )
                }
              />{" "}
              {line}
            </label>
          </li>
        ))}
      </ul>
      {card.dropped > 0 && (
        <p className="muted small">
          {card.dropped} more proposal(s) dropped: they did not quote your
          words.
        </p>
      )}
      {editing ? (
        <textarea
          aria-label="your words"
          rows={3}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      ) : (
        <p>
          Saved as your words: <q>{text}</q>
        </p>
      )}
      {state === "open" ? (
        <div className="btnrow">
          <button
            disabled={save.isPending || !text.trim()}
            onClick={() => save.mutate()}
          >
            Save
          </button>
          <button onClick={() => setEditing((e) => !e)}>
            {editing ? "Done editing" : "Edit"}
          </button>
          <button disabled={discard.isPending} onClick={() => discard.mutate()}>
            Discard
          </button>
        </div>
      ) : (
        <p className={state === "saved" ? "ok-line" : "muted"}>
          {state === "saved"
            ? `Saved as what ${save.data?.by ?? "you"} said.`
            : "Discarded."}
        </p>
      )}
      {save.isError && <p className="err">{String(save.error)}</p>}
      {discard.isError && <p className="err">{String(discard.error)}</p>}
    </div>
  );
}
