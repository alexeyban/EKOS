# Claude Code + EKOS session memory (RFC 0151)

> **Status: verified against the published hooks reference (code.claude.com/docs/en/hooks, checked
> 2026-09-21); NOT verified in a live session.** Confirmed by the docs: events `SessionStart` and
> `SessionEnd`; the JSON output shape `{"hookSpecificOutput":{"hookEventName":"SessionStart",
> "additionalContext":"..."}}` that `ekos session brief --format claude-hook` emits; `timeout` in
> seconds; `transcript_path`, `session_id` and `cwd` in the stdin payload; the
> `hooks.<Event>[].hooks[].{type,command}` settings structure. **Not documented:** a `PreCompact`
> event (this integration deliberately does not use one), and which events inject plain stdout into
> model context — hence the brief uses the explicit `additionalContext` JSON form. Whether the
> injected text actually reaches the model still needs one live check.

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
