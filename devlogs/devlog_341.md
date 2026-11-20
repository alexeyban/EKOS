# Devlog 341 — Plain ASCII terminal output on Windows

**Date:** 2026-11-20
**PRs:** none (pushed to `main`)
**Branch:** main

---

## Summary

The owner ran `ekos doctor` on Windows with the MSI build and saved the output to a file. The file
showed `â”€â”€…` where the rule should be and `â€”` for each dash. On Windows the CLI now writes those
symbols as ASCII, so any capture method gives readable text.

---

## Plain ASCII on Windows

### Problem / motivation
`ekos` writes UTF-8. PowerShell decodes a native program's output with its own code page, here
Windows-1252, before `>` saves it again as UTF-8. So `─` (three bytes) became `â”€` (three
characters). Reversing that step gives back the original text exactly. The program cannot set how
the shell decodes: PowerShell has chosen before `ekos` starts, so `SetConsoleOutputCP` would not help.

The CLI prints about 6,000 such symbols, from 900 printing calls in 56 files. Editing every call was
not practical, so one filter does it.

### What was built
| Component | Change |
|---|---|
| `cli/src/ascii.rs` | `active()` (Windows, or `EKOS_ASCII=1`; `EKOS_ASCII=0` turns it off), `plain` (a fixed table: `─`→`-`, `—`→`--`, `→`→`->`, `§`→`sec. `, `…`→`...`, box drawing, quotes, sparkline blocks), `console` (`plain` unless the text is a JSON document), `Writer` (an `io::Write` doing the same) |
| `cli/src/lib.rs` | `println!`/`print!`/`eprintln!`/`eprint!` defined before the modules. A `macro_rules!` in textual scope shadows the prelude's, so all 900 calls go through `ascii::console` unchanged. They call the std macros, so test output capture still works |
| `commands/mod.rs` | the tracing terminal layers write through `ascii::stdout`/`ascii::stderr` |
| `commands/questions.rs`, `commands/commit.rs` | the direct writes (questions output, commit progress lines) |
| `app.rs` | `try_get_matches_from`. When ASCII is active, `--help`, `--version` and usage errors are printed through the filter (`exit_for_clap`); otherwise `clap`'s own `exit` prints them, with colours |
| `bin/ekos.rs` | prints the final `Error: …` itself, through the filter, and exits with 1 as before |
| `tests/ascii_output.rs` | the binary with `EKOS_ASCII=1`: `doctor`, `--help` and a usage error are all ASCII; with `0` the rule stays; `console status --json` is still JSON |

### Decisions
- **Only known symbols change.** Every other non-ASCII character is kept. A Windows user name in
  Cyrillic, or a file name with an accent, is data, and `?` in its place would break a path someone
  copies.
- **JSON is never changed.** The console and EKOS.DEV read `--json` output. A JSON document's strings
  are values, and Windows reads UTF-8 bytes from a pipe correctly. The test is the text parsing as
  JSON, and it is only run when the text starts with `{` or `[`.
- **On in every Windows console, not only when redirected.** A capture is only one case. Fonts and
  code pages in old consoles show the same symbols wrongly, and one rule is easier to reason about.
- **Files EKOS writes keep UTF-8.** Generated docs, reports and exports are files, not terminal output.

---

## Knowledge Captured
- **`macro_rules! println` before the `mod` lines of `lib.rs` overrides `std::println!` for the whole
  crate.** It needs no import and does not trigger an ambiguity error. Implicit format captures
  (`{name}`) still work through `format!($($arg)*)`. Integration tests and other crates are not
  covered.
- **The mojibake comes from the capturing shell.** Recognise it by its sequences: `â”€` is `─`
  (UTF-8 read as cp1252), and `ΓöÇ` is the same character read as cp437. On a machine already
  affected: in PowerShell, `[Console]::OutputEncoding = [Text.UTF8Encoding]::new()`, or run
  `cmd /c "ekos doctor > log.txt"`.
- **Not seen on Windows yet.** CI's Windows job builds and tests it, and the test runs the binary
  with `EKOS_ASCII=1` on Linux. A capture on a real Windows PC is still to be checked.

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/cli/src/{ascii,lib,app}.rs`, `bin/ekos.rs` | the filter, the macros, clap and the error line |
| `ekos/crates/cli/src/commands/{mod,questions,commit}.rs` | logs and the direct writes |
| `ekos/crates/cli/tests/ascii_output.rs` | the binary test |
| `CHANGELOG.md`, `README.md`, `docs/generated/ekos-self-documentation.html` | docs |
