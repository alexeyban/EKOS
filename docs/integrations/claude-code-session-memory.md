# Claude Code + EKOS session memory (RFC 0151)

> **Status: `SessionStart` injection verified live (2026-09-21); timeout/failure semantics and
> `SessionEnd` not verified.** A scratch workspace with the hook below and a pending note containing a
> canary token: `claude -p` with the hook answered with the token; the same prompt in a directory
> without the hook answered `NONE`. So `ekos session brief --format claude-hook` output does reach the
> model as context. The docs (code.claude.com/docs/en/hooks) also confirm `SessionStart`/`SessionEnd`,
> `timeout` in seconds, and a `hooks.<Event>[].hooks[].{type,command}` structure. **No `PreCompact`
> event is documented**, so this integration does not use one. Not exercised: what a hook that times
> out or exits non-zero does to session start, and the `SessionEnd` commit hook.

## 1. Enable and register the MCP server

```toml
# ekos.toml
[session-memory]
enabled = true
```

`ekos mcp serve --workspace <dir>` then lists `ekos_session_note`, `ekos_session_recall` and
`ekos_session_brief`. There is no MCP tool that confirms or rejects a note.

## 2. Hooks — `.claude/settings.json` example

```json
{
  "hooks": {
    "SessionStart": [
      { "hooks": [ { "type": "command", "timeout": 10,
        "command": "ekos session brief --scope-from-git --format claude-hook || true" } ] }
    ],
    "SessionEnd": [
      { "hooks": [ { "type": "command", "timeout": 20,
        "command": "(ekos session commit >/dev/null 2>&1 &) ; true" } ] }
    ]
  }
}
```

Design rules baked into the commands, so a hook can never hurt a session:

- **Fail open for memory.** `ekos session brief` prints an empty-memory brief if the ledger is missing
  or busy; `|| true` keeps a failing command from blocking. `ekos session commit` retries a busy ledger
  with backoff and otherwise leaves notes *pending* (nothing lost).
- **Fail closed for redaction.** A note whose redaction fails, or that is nothing but a secret, is
  dropped — never written unredacted.

## 3. Optional transcript capture (Phase 7)

Not wired into a hook here because the transcript payload shape is unverified. Manually:

```bash
ekos session capture --session my-session --file transcript.txt   # redacted slices, kept outside the ledger
ekos session extract --session my-session                         # opt-in; needs [session-memory] extraction = true
ekos session commit
```

Extraction uses the `[llm]` provider — a cloud provider is a metered call. Output is only ever an
unconfirmed proposal whose quoted span must exist in the slice.

## 4. Review

```bash
ekos session recall "orders total"          # see tier + staleness
ekos session review <claim-id> confirm      # human-only; T0 -> T1
ekos session review <claim-id> supersede --by <newer-claim-id>
```
