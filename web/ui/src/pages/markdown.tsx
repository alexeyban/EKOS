// RFC 0207: Markdown from an extension (a proposal), rendered by the console itself. Only
// headings, paragraphs, lists, tables, fenced code, inline code and bold: React elements, never
// HTML strings, so raw HTML in the text is shown as text.

import type { ReactNode } from "react";

export type Block =
  | { kind: "heading"; level: number; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; items: string[] }
  | { kind: "table"; header: string[]; rows: string[][] }
  | { kind: "code"; lang: string; text: string };

function cells(line: string): string[] {
  return line
    .trim()
    .replaceAll(/^\||\|$/g, "")
    .split(/(?<!\\)\|/)
    .map((c) => c.trim().replaceAll("\\|", "|"));
}

/** A block read from `lines` at some index, and the index after it (`block` is null for
 * a skipped blank line). */
type Read = { block: Block | null; next: number };

const LIST_ITEM = /^\s*[-*]\s+/;
const TABLE_ROW = /^\s*\|/;
// `\s*(?:\|\s*)?` rather than `\s*\|?\s*`: the two adjacent `\s*` backtrack quadratically.
const TABLE_RULE = /^\s*(?:\|\s*)?:?-{3,}/;
// `\s+(\S.*)?$` rather than `\s+(.*)$`: `\s+` and `.*` overlap, so a failing match backtracks.
const HEADING = /^(#{1,6})\s+(\S.*)?$/;
const PARAGRAPH_END = /^(#{1,6}\s|```|\s*[-*]\s|\s*\|)/;

function fenceAt(lines: string[], i: number): Read | null {
  const fence = lines[i].match(/^```\s*([\w-]*)/);
  if (!fence) return null;
  const body: string[] = [];
  let j = i + 1;
  while (j < lines.length && !lines[j].startsWith("```")) body.push(lines[j++]);
  return { block: { kind: "code", lang: fence[1], text: body.join("\n") }, next: j + 1 };
}

function headingAt(lines: string[], i: number): Read | null {
  const h = HEADING.exec(lines[i]);
  if (!h) return null;
  return { block: { kind: "heading", level: h[1].length, text: h[2] ?? "" }, next: i + 1 };
}

function tableAt(lines: string[], i: number): Read | null {
  if (!TABLE_ROW.test(lines[i]) || i + 1 >= lines.length || !TABLE_RULE.test(lines[i + 1])) return null;
  const header = cells(lines[i]);
  let j = i + 2;
  const rows: string[][] = [];
  while (j < lines.length && TABLE_ROW.test(lines[j])) rows.push(cells(lines[j++]));
  return { block: { kind: "table", header, rows }, next: j };
}

function listAt(lines: string[], i: number): Read | null {
  if (!LIST_ITEM.test(lines[i])) return null;
  const items: string[] = [];
  let j = i;
  while (j < lines.length && LIST_ITEM.test(lines[j])) items.push(lines[j++].replace(LIST_ITEM, ""));
  return { block: { kind: "list", items }, next: j };
}

function blankAt(lines: string[], i: number): Read | null {
  return lines[i].trim() === "" ? { block: null, next: i + 1 } : null;
}

/** The first line is always taken: a line no reader accepts (`| a |` with no rule under it)
 * still ends a paragraph, and taking nothing would never advance. */
function paragraphAt(lines: string[], i: number): Read {
  const para: string[] = [lines[i]];
  let j = i + 1;
  while (j < lines.length && lines[j].trim() !== "" && !PARAGRAPH_END.test(lines[j])) para.push(lines[j++]);
  return { block: { kind: "paragraph", text: para.join(" ") }, next: j };
}

/** Tried in order at each line; a paragraph is what is left. */
const READERS = [fenceAt, headingAt, tableAt, listAt, blankAt];

/** Splits Markdown into blocks. */
export function parseMarkdown(text: string): Block[] {
  const lines = text.replaceAll("\r\n", "\n").split("\n");
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    let read: Read | null = null;
    for (const reader of READERS) {
      read = reader(lines, i);
      if (read) break;
    }
    read ??= paragraphAt(lines, i);
    if (read.block) out.push(read.block);
    i = read.next;
  }
  return out;
}

/** Inline `code`, **bold** and *italic*, as React nodes. */
export function inline(text: string): ReactNode[] {
  const parts: ReactNode[] = [];
  const re = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*\s][^*]*\*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let k = 0;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    const tok = m[0];
    parts.push(
      tok.startsWith("`") ? (
        <code key={k++}>{tok.slice(1, -1)}</code>
      ) : tok.startsWith("**") ? (
        <strong key={k++}>{tok.slice(2, -2)}</strong>
      ) : (
        <em key={k++}>{tok.slice(1, -1)}</em>
      ),
    );
    last = m.index + tok.length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

export function Markdown({ text }: { text: string }) {
  return (
    <div className="ext-markdown">
      {parseMarkdown(text).map((b, i) => {
        switch (b.kind) {
          case "heading": {
            const H = (`h${Math.min(b.level + 2, 6)}`) as "h3" | "h4" | "h5" | "h6";
            return <H key={i}>{inline(b.text)}</H>;
          }
          case "paragraph":
            return <p key={i}>{inline(b.text)}</p>;
          case "list":
            return (
              <ul key={i}>
                {b.items.map((it, j) => (
                  <li key={j}>{inline(it)}</li>
                ))}
              </ul>
            );
          case "table":
            return (
              <table key={i} className="lm-table">
                <thead>
                  <tr>
                    {b.header.map((h, j) => (
                      <th key={j}>{inline(h)}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {b.rows.map((r, j) => (
                    <tr key={j}>
                      {r.map((c, k) => (
                        <td key={k}>{inline(c)}</td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            );
          case "code":
            return (
              <pre key={i} className="ext-text">
                {b.text}
              </pre>
            );
        }
      })}
    </div>
  );
}
