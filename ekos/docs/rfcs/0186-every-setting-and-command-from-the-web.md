# RFC 0186 — Every setting and every command from the web

**Status:** Accepted — implemented 2026-10-14 (devlog_270)
**Author:** EKOS team
**Created:** 2026-10-14
**Builds on:** RFC 0130 (config UX), RFC 0131 (commands and runs), RFC 0180 (roles), RFC 0184 (console settings), RFC 0185 (installer)
**Part of:** installer and web configuration (0184, 0185, this RFC, 0187)

## Motivation

The console runs 17 commands from a hand-kept list (`web/api/app/commands.py`) out of more than a
hundred, and edits `ekos.toml` only as raw text. Someone who installed EKOS with the setup wizard
should not need a terminal to run `semantics confirm`, `coverage`, `ledger timeline` or
`migrate lower`, or to know the name and type of every setting.

Two things stop the console from simply growing its list: every command added by hand is a
place to forget an authorization decision, and a hand-written form drifts from the Rust types.
So the binary describes itself and the console builds from that description.

## 1. A command catalogue from the binary

`ekos commands --json` walks clap's command tree and prints, for every command: its path, its
help, its arguments (long name or position, `flag` / `value` / `path` — a path is detected by
its value parser's type, `PathBuf` — multiple, required, possible values, defaults, help), its
**class**, and whether it may call the configured model.

The class is a decision, kept in one table (`commands/catalogue.rs::CLASSES`):

| Class | Meaning | From the web |
|---|---|---|
| `read` | reads the project | `read` |
| `write` | changes the project's state | `operate` |
| `review` | a human decision (confirm, reject, resolve, approve, import of reviewed LinkML) | `review` — a person, never a token; the reviewer is the signed-in person |
| `admin` | destructive or structural (`clean`, `ledger migrate`, `branch merge`, `session purge`) | `admin` + the command typed as a confirmation |
| `external` | reaches a live system outside EKOS (`migrate discover/load/v5`, `clickhouse ask`, `treasury scan`) | `admin` + typed confirmation |
| `local` | servers, the installer, the console itself, the home as a whole | never |

A test fails when a command has no row or a row names no command, and pins the review commands
(the same ones `commands/mcp.rs` must never reach).

**Arguments the console sets** are not in the catalogue: `--config`, `--home`, `--project`,
`--tenant`, `--read-scope`, `--workspace`.

### The console side

`GET /api/workspaces/{id}/cli` lists the catalogue without `local` commands, with `allowed` for
the caller. `POST /api/workspaces/{id}/cli` takes `{command, args, confirm}`:

1. the command must be in the catalogue and not `local`;
2. the caller needs the class's action, and must see the whole workspace (a command prints what
   it reads, and none takes the caller's read scope);
3. `admin`/`external` need `confirm` equal to the command's name;
4. `catalogue.build_argv` builds the argv: only listed arguments; options as `--long=value`,
   positionals after `--`, so no value can pose as an option; possible values enforced; a
   `path` argument must resolve inside the project's directory or the tenant's source roots;
   for a review, `by` is the signed-in person whatever the form said;
5. the run goes through the existing job runner as a `cli` command whose argv was validated —
   never a shell. `cli` is not in the named-command list, so the old route cannot reach it.

The UI's **Commands** tab lists the commands by group with a filter, builds a form from the
arguments, shows the command line before running, labels commands that may call a paid model,
and opens the run's log.

## 2. `ekos.toml` as a form

`EkosConfig` and every section type derive `schemars::JsonSchema`; `ekos config schema --json`
prints the schema (names as in the file, doc comments as descriptions, defaults). The console
serves it at `GET /api/config-schema`, and `GET …/config` now also returns the file's values.

The **Settings** card on the project's config page renders every section: text, numbers,
true/false/unset, choices, lists (one per line), nested sections. A setting whose type is a
table of tables or a list of tables is left to the text editor. Changes go to
`POST …/config/edit`, which applies them to the current text with `tomlkit` — comments, order and
unknown keys kept — and returns the new text without writing; it lands in the editor, where the
existing validate → save path (RFC 0130) applies. A change cannot replace a section with a value
or reach through a value.

Sources (`source add|remove`), the project's MCP and session-memory options and every other
section are reachable through these two; console settings are RFC 0184's page.

## Tests

- Rust: catalogue completeness both ways; review commands are `review`; argument shapes for
  `query find`, `import linkml` (a path), `commit` (`--yes`, may call a model).
- Console: argv shape; a positional that looks like an option stays a value; unknown,
  console-managed, missing, impossible and mistyped arguments refused; `local` and unknown
  commands refused; rights by class; a review names the person; a token cannot review; typed
  confirmation; paths outside the project refused and relative ones resolved; the named route
  cannot reach `cli`; the real binary's catalogue and a real run. Config: edits keep comments
  and unknown keys, refuse structure changes, write nothing; the real schema.
- UI: grouping, form → args, the reviewer never sent, the preview; field kinds from the real
  schema.
- `deploy/e2e/package.sh`: a command outside the old list run from the web and in a browser; a
  setting changed through the form, validated and saved.

## Not in this RFC

Commands of extension builds (RFC 0149) are not in the catalogue; a private build would add rows.
Settings of a list of tables (`[[sources]]`, `[migrate.connections.*]`) stay in the text editor.
