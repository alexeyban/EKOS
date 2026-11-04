import { describe, expect, it } from "vitest";
import { parseMarkdown } from "./markdown";

describe("parseMarkdown", () => {
  it("reads headings, lists, tables and code", () => {
    const text = "# Title\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | x \\| y |\n\n```sql\nselect 1\n```\nplain\ntext";
    expect(parseMarkdown(text)).toEqual([
      { kind: "heading", level: 1, text: "Title" },
      { kind: "list", items: ["one", "two"] },
      { kind: "table", header: ["a", "b"], rows: [["1", "x | y"]] },
      { kind: "code", lang: "sql", text: "select 1" },
      { kind: "paragraph", text: "plain text" },
    ]);
  });

  it("keeps an empty heading and does not take seven hashes for one", () => {
    expect(parseMarkdown("#   ")).toEqual([{ kind: "heading", level: 1, text: "" }]);
    expect(parseMarkdown("####### x")).toEqual([{ kind: "paragraph", text: "####### x" }]);
  });

  it("ends on a table row with no rule under it (this used to loop forever)", () => {
    expect(parseMarkdown("| a | b |")).toEqual([{ kind: "paragraph", text: "| a | b |" }]);
  });

  it("stays linear on long runs of whitespace", () => {
    const spaces = " ".repeat(50_000);
    const start = performance.now();
    parseMarkdown(`#${spaces}x\u2028\n| a |\n${spaces}x`);
    expect(performance.now() - start).toBeLessThan(1000);
  });
});
