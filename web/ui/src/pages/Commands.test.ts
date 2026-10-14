import { describe, expect, it } from "vitest";
import { type CommandEntry, formArgs, groupCommands, preview, visibleArgs } from "./Commands";
import { kindOf, parseField, resolve, type Schema, showField, valueAt } from "./config-schema";
import realSchema from "./__fixtures__/config-schema.json";

const arg = (id: string, over: Partial<CommandEntry["args"][number]> = {}) => ({
  id,
  long: id,
  index: null,
  kind: "value" as const,
  multiple: false,
  required: false,
  possible: [],
  default: [],
  value_name: null,
  help: "",
  ...over,
});

const find: CommandEntry = {
  path: ["query", "find"],
  about: "Search the ledger",
  class: "read",
  metered: false,
  allowed: true,
  confirm: false,
  args: [arg("text", { long: null, index: 1, required: true }), arg("mode"), arg("json", { kind: "flag" })],
};
const confirm: CommandEntry = {
  path: ["semantics", "confirm"],
  about: "Confirm",
  class: "review",
  metered: false,
  allowed: true,
  confirm: false,
  args: [arg("targets", { long: null, index: 1, multiple: true }), arg("by", { long: "as" }), arg("note")],
};
const build: CommandEntry = { ...find, path: ["build"], about: "Observe", class: "write", args: [] };

describe("commands page", () => {
  it("groups by the first word and filters", () => {
    const g = groupCommands([find, confirm, build], "");
    expect(g.map(([k, v]) => [k, v.length])).toEqual([
      ["query", 1],
      ["semantics", 1],
      ["project", 1],
    ]);
    expect(groupCommands([find, confirm, build], "ledger")).toEqual([["query", [find]]]);
  });

  it("sends flags as booleans, lists one per line, and leaves empty fields out", () => {
    expect(formArgs(find, { text: " customers ", json: true, mode: "" })).toEqual({ text: "customers", json: true });
    expect(formArgs(confirm, { targets: "a\n\n b \n" })).toEqual({ targets: ["a", "b"] });
  });

  it("never sends the reviewer of a review: the server sets it", () => {
    expect(visibleArgs(confirm).map((a) => a.id)).toEqual(["targets", "note"]);
    expect(formArgs(confirm, { targets: "a", by: "someone" })).toEqual({ targets: ["a"] });
  });

  it("previews the command line", () => {
    expect(preview(find, { text: "customers", mode: "hybrid", json: true })).toBe(
      "ekos query find --mode=hybrid --json customers",
    );
  });
});

describe("settings form from the real schema", () => {
  const root = realSchema as Schema;
  const def = (name: string) => root.$defs![name];

  it("knows each kind of setting", () => {
    const sem = def("SemanticsConfig").properties!;
    expect(kindOf(root, sem["enabled"]).kind).toBe("bool");
    expect(kindOf(root, sem["min-sites"])).toEqual({ kind: "number", integer: true });
    expect(kindOf(root, sem["ontology"]).kind).toBe("text"); // Option<String>
    expect(kindOf(root, def("ObserveConfig").properties!["paths"]).kind).toBe("list");
    expect(kindOf(root, def("MigrateConfig").properties!["connections"]).kind).toBe("complex");
    expect(kindOf(root, def("RecoverConfig").properties!["sql"]).kind).toBe("section");
    expect(kindOf(root, root.properties!["llm"]).kind).toBe("section");
  });

  it("every top-level section resolves", () => {
    for (const [name, node] of Object.entries(root.properties!)) {
      const r = resolve(root, node);
      expect(r.$ref, name).toBeUndefined();
    }
  });

  it("turns field text into values and back", () => {
    expect(parseField({ kind: "number", integer: true }, "12")).toBe(12);
    expect(parseField({ kind: "number", integer: true }, "")).toBeNull();
    expect(parseField({ kind: "bool" }, "false")).toBe(false);
    expect(parseField({ kind: "bool" }, "")).toBeNull();
    expect(parseField({ kind: "list" }, "src\n db \n")).toEqual(["src", "db"]);
    expect(showField({ kind: "list" }, ["a", "b"])).toBe("a\nb");
    expect(valueAt({ recover: { sql: { "default-dialect": "postgres" } } }, ["recover", "sql", "default-dialect"])).toBe(
      "postgres",
    );
  });
});
