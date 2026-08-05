# Act 1 transcript

**Prompt:** What do you actually know about my projects right now?

```
Here's what I actually know right now, from memory plus the current repo state:

**From persistent memory (cross-session):**
- One memory on file: EKOS storage state as of 2026-07-17 — RFC 0015 (compact storage: v2 ledger, packed artifact segments) is implemented and was *applied to the live estate workspace* (not just merged). RFC 0016 (fact-segment engine: EAV facts, tantivy, mmap) is fully implemented too, and was promoted to serve the live estate after the size gate was amended (structurally unreachable at the original 2x threshold, amended to "≤2x at equal/better latency," passing at 1.66x). Fresh workspaces still default to SQLite; only the existing estate workspace runs on the fact engine.
- No feedback or user-profile memories saved yet — I don't have anything on file about your role or working preferences beyond what CLAUDE.md states.

**From the current repo state (not memory — this is live):**
- You're on `main`, up to date with origin, with uncommitted changes touching the marketing feature (RFC 0030): `crates/cli/src/commands/marketing.rs`, `crates/marketing/src/prompt.rs`, `crates/marketing/src/tweet.rs`, plus `demo/` docs and `ekos.toml`.
- An untracked file `ekos-marketing-agent-plan.md` — a design doc for an "EKOS Marketing Agent v1": auto-generate an X/Twitter post from new `devlog_XXX.md` files, require human approval, prevent duplicate posts. Explicitly MVP-scoped (no LinkedIn/blog/images/threads/analytics/scheduling for v1).
- Recent commit history shows unrelated fixes (benchmark DDL signature update, Pentaho/SQL script fix) and docs work on the Pioneer Program.

Given CLAUDE.md's workflow, this plan doc would need a proper RFC in `docs/rfcs/` or `ekos/docs/rfcs/` (next available number) before implementation — worth checking whether that's already been done or still pending. Want me to check the current in-progress marketing diff against that plan doc to see how far along it is?
```
