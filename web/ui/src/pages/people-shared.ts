// RFC 0253/0254 — shared by the Inbox, Questions and Chat pages.

import type { InboxItem, Question, QuestionStatus } from "../api/types";
import type { Me } from "../WorkspaceShell";

export const QUESTION_TYPES: Record<string, string> = {
  meaning_of_value: "what a value means",
  unexplained_values: "values no code names",
  dead_code: "code for a value no longer seen",
  which_is_right: "sources disagree",
  logic_vs_data: "logic misses data",
  is_this_a_reference: "is this a reference?",
  what_is_a_row: "what is a row?",
  still_true: "is a note still true?",
};

export function questionChip(s: QuestionStatus): string {
  if (s === "open") return "chip warn";
  if (s === "answered") return "chip ok";
  return "chip";
}

/** Who a person is, as a question's addressee names them (their e-mail when signed in). */
export function whoAmI(me: Me, typed: string): string {
  return me.mode === "token" ? `token:${typed.trim()}` : (me.email ?? "");
}

/** "Not me" is for the person the question is with, and nobody else. */
export function isMine(q: Question, me: Me, typed: string): boolean {
  const who = whoAmI(me, typed).toLowerCase();
  return !!who && !!q.addressee && q.addressee.toLowerCase() === who;
}

/** Pending first (they wait for someone), then newest. */
export function inboxOrder(items: InboxItem[]): InboxItem[] {
  const rank = (i: InboxItem) => (i.status === "pending" ? 0 : 1);
  return [...items].sort(
    (a, b) =>
      rank(a) - rank(b) ||
      (b.received_at ?? "").localeCompare(a.received_at ?? ""),
  );
}

export function sentBy(i: InboxItem): string {
  const s = i.senders?.[0];
  return s ? `${s.by} (${s.channel})` : "?";
}

export function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** A file's bytes as base64, for the inbox's JSON body. */
export function fileToBase64(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onerror = () => reject(r.error ?? new Error("cannot read the file"));
    r.onload = () => {
      const url = String(r.result ?? "");
      resolve(url.slice(url.indexOf(",") + 1));
    };
    r.readAsDataURL(file);
  });
}

/** Revealed values as rows to show; anything else (a withheld note) as text. */
export function revealedRows(
  revealed: unknown,
): { rows: Record<string, unknown>[] } | { note: string } {
  if (Array.isArray(revealed))
    return { rows: revealed.filter((r) => r && typeof r === "object") };
  if (revealed && typeof revealed === "object" && "withheld" in revealed)
    return { note: String((revealed as { withheld: unknown }).withheld) };
  return {
    note: revealed == null ? "nothing to show" : JSON.stringify(revealed),
  };
}
