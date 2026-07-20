# EKOS Demo — Skills + Custom Subagents

**What this demonstrates:** EKOS compiles an entire multi-project estate
into an evidence-backed knowledge ledger, then serves it read-only through
MCP tools. This demo shows two different ways Claude Code consumes that
ledger — **skills** (methods loaded inline into any session) and **custom
subagents** (dedicated personas with a scoped toolset) — across seven acts
that run against the presenter's real, live estate. Nothing here is staged
data; every query hits the actual compiled ledger.

> **Note on process:** this demo (agents, script, transcripts) is a
> *consumer* of the compiled ledger — it changes no compiler pass, schema,
> or storage format, so it does not require an RFC. The RFC-first rule in
> `CLAUDE.md` governs code under `ekos/crates/`.

## Cast

| | Kind | Capability |
|---|---|---|
| `ekos-knowledge` | skill | query mechanics: tool picker, Locate→Expand→Prove, EKL cheat sheet |
| `memory` | skill | cross-project memory: recall, capture, async refresh |
| `estate-scout` | agent (haiku) | existence — "what's out there?", MCP-only, cannot open files |
| `impact-analyst` | agent (sonnet) | consequence — blast-radius + evidence |
| `memory-keeper` | agent (sonnet) | memory — the only agent that writes |
| `estate-architect` | agent (inherit) | synthesis — designs from the estate's own prior art |

---

## Act 0 — Prep checklist (run before presenting)

1. **Refresh the ledger.** Run the memory skill's refresh pipeline and
   confirm all four stages succeed:
   ```bash
   cd "$WORKSPACE_ROOT" && "$EKOS_BIN" build && "$EKOS_BIN" recover && \
     "$EKOS_BIN" compile && "$EKOS_BIN" commit
   ```
   Then confirm scale: ask `ekos_status` for ~20k+ objects across the
   estate's project count.
2. **Fresh MCP connection.** Start a *new* Claude Code session after any
   EKOS rebuild — a long-running MCP connection can go stale against a
   schema/binary that changed underneath it. Do not rebuild EKOS between
   prep and showtime.
3. **Install the agents:**
   ```bash
   cp demo/agents/*.md ~/.claude/agents/
   ```
   Run `/agents` inside Claude Code and confirm all four appear with the
   tool scopes described above.
4. **Smoke every act headless first** — see `headless.sh` below. Known-good
   targets to check for non-empty results (substitute your own estate's
   equivalents if these don't apply):
   - Act 2: an EKL table search returns >0 rows
   - Act 3: the chosen table has >0 dependents
   - Act 4: a real past lesson note is found by keyword search
   - Act 7: a keyword search for the climax topic hits real prior work
     (verified: searching "cdc" against this estate returns 50 hits across
     `cdc_gold/`, `databricks-lab/ingestion/cdc/`, and
     `data-platform/pipelines/spark_jobs/cdc_stream.py`, among others)
5. **Reset Act 5** before each run-through — delete or vary the lesson note
   it writes, so the capture is genuinely new each time. **Verify the write
   actually happened** — confirmed in rehearsal that a headless run can
   reply "Saved" without having written anything (a confidently-wrong
   result, not a crash); `headless.sh` now checks this automatically for
   Act 5, but when running Act 5 live, check `$MEMORY_DIR` for the new file
   before trusting the transcript.
6. Prompts below reference objects **by name**, never by id — ids can shift
   across ledger rebuilds; agents resolve names to ids live every time.

---

## Act 1 — Cold open (skill, no agent)

**Say:** *"What do you actually know about my projects right now?"*

**Activates:** the `ekos-knowledge` skill, loaded inline (no subagent) —
first proof point: the base session already knows how to query the ledger.

**Expected calls:** `ekos_status` → then a broad `ekos_search` on a topic
the audience will recognize.

**Wow line:** *"That's every object, every relationship, across every
project — and it opened zero files to tell you that."*

---

## Act 2 — Scout (estate-scout, haiku)

**Say:** *"Use the estate-scout agent: find every database table related
to orders across my estate, and show me what one of them is connected to."*

**Activates:** `estate-scout` — note the model badge: this is the cheapest
model available, deliberately, to make the point that the ledger — not the
model — is doing the heavy lifting.

**Expected calls:** `ekos_ekl "FIND Object WHERE kind = 'Table' AND name
CONTAINS 'order'"` → `ekos_neighborhood` on one resulting id.

**Wow line:** *"A cross-repository schema inventory, including foreign
keys — from the cheapest model Claude offers, with zero file access."*

**Verified reality:** the estate's current SQL/table recovery covers the
checked-in fixtures (`ecommerce.sql`, `northwind.sql`) richly (real FK
graphs) — most of the wider estate's 22k objects are file/git-level, not
deep schema-level. The scout correctly says so rather than inventing
tables in other projects; that's the honesty rule working, not a gap to
hide. If a richer real-project schema has been recovered by demo day,
swap the prompt's project name accordingly.

---

## Act 3 — Impact (impact-analyst, sonnet)

**Say:** *"Ask the impact-analyst: what breaks if I rename the customers
table?"*

**Activates:** `impact-analyst`.

**Expected calls:** locate → `ekos_dependents` → `ekos_state` per
dependent for evidence.

**Wow line:** *"A blast-radius report where every dependency comes with
the actual foreign-key clause or config line that proves it — not a
guess."*

**Verified reality:** rehearsal shows the analyst correctly reports that
today's only indexed `customers` tables are the fixture schemas (it
actively checked and ruled out similarly-named real-project entities for
lack of evidence) — a clean demonstration of "empty/negative results are
answers, not failures," one of the ledger's core honesty rules. Present it
that way rather than as a letdown.

---

## Act 4 — Recall (memory-keeper, read path)

**Say:** *"Have I ever hit FTS5 duplicate-row problems before? How did I
fix it?"*

**Activates:** `memory-keeper`, reading.

**Expected calls:** `ekos_search "fts5 duplicates"` → `ekos_state` on the
matching note.

**Wow line:** *"That lesson was written by a past session solving a
completely different problem — and it's readable by any model, not just
the one that wrote it."*

---

## Act 5 — Capture (memory-keeper, write path)

**Say:** *"Remember this for future sessions: `headless.sh`'s act filter
used to compare the target act number against itself instead of against
the requested list, so passing one act number silently ran all of them —
always verify a filter function against a case where it should say no."*

**Activates:** `memory-keeper`, writing.

**Expected calls:** `Write` a new
`EKOS--lesson--headless-filter-logic-inverted.md` note, then `Bash` to
fire the async refresh pipeline.

**Wow line:** *"That's a bug this demo's own rehearsal found ten minutes
ago — captured live, on stage, as it happened."*  Optionally `tail -f
.ekos/refresh.log` in a side terminal to show the pipeline actually run.

**Prep note:** pick a fact for this act that is genuinely new — check it
isn't already covered in `$MEMORY_DIR` *or* in Claude Code's own
per-project memory (`~/.claude/projects/<hash>/memory/`), which captures
facts independently of EKOS. A well-scoped agent will correctly refuse to
write a near-duplicate of something already remembered (confirmed in
rehearsal), which is the right behavior but means a stale example won't
reliably demo a fresh write.

---

## Act 6 — Time machine (skill, no agent)

**Say:** *"What changed across my workspace in the last week?"*

**Activates:** `ekos-knowledge` skill inline.

**Expected calls:** `ekos_diff(from=<7 days ago>)`, then optionally
`ekos_state(id, at=<a past timestamp>)` on one changed object to show its
prior state.

**Wow line:** *"That's not a git log — it's a semantic diff, and it can
reconstruct exactly what any object looked like at any point in history."*

**Verified reality:** right after a heavy rebuild week, `ekos_diff` over the
last 7 days is mostly ingestion noise, and rehearsal shows Claude correctly
falls back to real git logs rather than presenting that noise as signal.
For a clean live demo, pick a `from` timestamp *after* the last full
rebuild (so the diff reflects real incremental changes), or anchor the
"reconstruct the past" half of this act on a specific object with genuine
multi-version history (e.g. one of the fixture tables touched during
EKOS's own RFC work) via `ekos_state(id, at=...)` instead of a
workspace-wide diff.

---

## Act 7 — Climax (estate-architect)

**Say:** *"Design a CDC architecture for ingesting order data into a
lakehouse. Base it on my past work — my prior CDC projects, my mistakes,
my lessons."*

**Activates:** `estate-architect`.

**Expected calls:** `ekos_search` on the topic and on prior related
projects → `ekos_state` on the strongest hits → `ekos_neighborhood` for
wiring → a memory-note search for lessons.

**Wow line:** close on the design citing real prior projects (this estate
has genuine CDC material in `cdc_gold/`, `databricks-lab/ingestion/cdc/`,
and `data-platform/pipelines/spark_jobs/cdc_stream.py`) and named lesson
notes, then say it plainly: *this isn't a generic architecture — it's the
one **you** would actually build, because it's built from what you already
know.*

---

## Fallbacks

- **LLM variability** in a live run: prompts are fixed and rehearsed; the
  expected-call and expected-output-shape notes above let the presenter
  narrate around any detour without losing the audience.
- **Total failure** (network, model outage): fall back to the
  pre-generated transcripts in `demo/transcripts/`, produced by
  `headless.sh`.
