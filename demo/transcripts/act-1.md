# Act 1 transcript

**Prompt:** What do you actually know about my projects right now?

```
Here's what I actually know, combining memory and current repo state:

**From persistent memory (may be stale — worth re-verifying if load-bearing):**
- EKOS storage stack: RFC 0015 (compact v2 storage) shipped and migrated to the live estate on 2026-07-17. RFC 0016 (fact-segment engine — tantivy, mmap, EAV facts) is fully implemented and was *promoted to serve the live estate*, though it never hit its original ≥2x compression gate (gate was amended to ≤2x, which it passes at 1.66x). Default backend for fresh workspaces is still SQLite (soaking).

**From current repo state (git log + working tree, more trustworthy than memory for "now"):**
- Latest commit is RFC 0043 — global secrets/PII redaction, on by default, non-optional.
- Recent work arc: RFC 0042 (curated docs), RFC 0041 (Rust source analyzer), RFC 0038/0039/0040 (Python/PySpark analyzer, SQL dialects, code knowledge expansion), Pentaho→dbt tooling and a presentation deck.
- You have **uncommitted changes right now**: `ekos.toml`, `demo/transcripts/act-1.md`, `crates/cli/src/commands/marketing.rs`, `crates/marketing/src/prompt.rs`, `crates/marketing/src/tweet.rs`, plus an untracked `ekos-marketing-agent-plan.md` — looks like in-progress work on the marketing/tweet-publishing feature (RFC 0030 area).

Want me to look at the diff/plan file to see what that in-progress marketing work is actually doing?
```
