# RFC 0191 — Extension seam v2: commands, their classes, scoped MCP tools

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-16
**Builds on:** RFC 0149 (extension seam), RFC 0181 (read scope), RFC 0186 (command catalogue)
**Needed by:** RFC 0190 (EKOS Lakehouse)

## Motivation

RFC 0149 lets an out-of-tree build add observers, recover passes, a post-commit step and MCP
tools. EKOS Lakehouse (RFC 0190) is the first extension that is a *product* rather than a
connector, and it needs three things the seam does not offer:

1. **Its own commands** (`ekos lakehouse init|plan|generate|check|run-report|status|doctor`).
   Today `Cli`/`Commands` are private and `main_with` parses the arguments itself, so an
   extension cannot add a subcommand at all.
2. **A class for each command** (RFC 0186). `ekos commands --json` drops a command without a
   row in `catalogue::CLASSES`, and the console never runs a command that is not listed.
3. **A read-scope decision for its MCP tools** (RFC 0181). Under a restricted scope the MCP
   server serves only `SCOPED_TOOLS`, a constant, so every extension tool is refused to a
   restricted reader, including tools that read only through the scoped store.

## Design

Four additions to `EkosExtension`, and a settings table in `ekos.toml`, each with a default that keeps today's behaviour, so the
private binary extension (RFC 0149) compiles and behaves unchanged.

```rust
/// Top-level subcommands this extension adds, e.g. `lakehouse` with its own subcommands.
fn commands(&self) -> Vec<clap::Command> { Vec::new() }

/// The catalogue class of every leaf command it adds, by path ("lakehouse status").
fn command_classes(&self) -> Vec<(&'static str, Class, bool)> { Vec::new() }

/// Runs one of `commands()`. `name` is the top-level name, `matches` its own matches.
async fn run_command(&self, name: &str, matches: &clap::ArgMatches,
                     ctx: &CommandContext<'_>) -> anyhow::Result<()> { … bail … }

/// True when MCP tool `name` reads knowledge only through the store it is handed,
/// so a read scope filters it and a restricted reader may call it.
fn mcp_tool_is_scoped(&self, _name: &str) -> bool { false }
```

`CommandContext` carries what every built-in command gets after `main_with`'s setup: the
loaded `EkosConfig` (with `--project`/`--tenant` already applied, RFC 0178), the working
directory, the config path and the directory the command was typed in. Commands open the
ledger through `commands::store`, as built-ins do.

### Parsing

`main_with` builds the command tree as `Cli::command()` plus every extension's commands, and
parses once. A built-in command goes through `Cli::from_arg_matches` as today. An extension
command becomes a non-derived `Commands::Extension { name, matches }` variant (`#[command(skip)]`),
with the global flags read from the same matches, and then follows the normal path:
project selection, config load, logging, dispatch to the owning extension.

`main_with` refuses to start when:

- an extension command's name is a built-in command or another extension's command;
- a leaf of an extension command has no class in `command_classes()`. This is the runtime
  form of the `every_command_has_a_class_and_every_class_a_command` guard, which cannot see
  out-of-tree commands.

`--read-scope` stays refused for extension commands, as for every command other than
`mcp serve`, `ekl` and `query`: a scope cannot be enforced on a command that reads more than
the ledger.

`main_with_args(extensions, args)` is the same function with explicit arguments, for tests.
`main_with` passes the process arguments.

### Catalogue

`ekos commands --json` lists extension commands with their classes, through the same
`catalogue` function, so the console can run them with the same rules as built-ins.

### MCP

The restricted-reader check becomes "in `SCOPED_TOOLS`, or an extension says the tool is
scoped". An extension tool is still refused by default. The web console's MCP gateway keeps
its own copy of `SCOPED_TOOLS` and does not see extension decisions: a restricted reader
reaching an extension tool through the console gateway is still refused. That is the safe
direction, and it is recorded here as a known limit.

### Settings

`EkosConfig` refuses unknown sections (`deny_unknown_fields`), and a config that fails to parse
falls back to the defaults with only a warning, so an extension had nowhere to keep its
settings. `ekos.toml` gains one table per extension:

```toml
[extensions.lakehouse]
trino-url = "http://127.0.0.1:58080"
```

Core keeps `[extensions.*]` as given (`EkosConfig::extensions`, a map of JSON values) and never
reads it. `EkosConfig::extension_settings::<T>(name)` parses one table as the extension's own
type, with an error that names the table. Any other unknown section is still an error.

## Not in this RFC

- Console tabs contributed by an extension. RFC 0190 M5 decides between a generic mechanism
  and a separate UI.
- Custom-kind registration at runtime. Extension kinds keep getting rows in
  `ekos_kir::custom_kinds::REGISTRY`, as `Binary*` did in RFC 0149.

## Tests

- A test extension adds a command group: it parses, dispatches with the loaded config, and
  appears in the catalogue with its class.
- A collision with a built-in name, and a leaf without a class, each stop `main_with` with a
  message naming the command.
- An extension tool marked scoped is served to a restricted reader; one not marked is refused.
- `Extensions::none()`: the command tree and catalogue are byte-identical to before.
- `[extensions.<name>]` parses into the extension's type; a mismatch names the table; another
  unknown section is still refused; a config without extensions serializes as before.
