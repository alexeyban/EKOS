# RFC 0177 — Eval follow-up: retrieval gaps, premise rejections, uncited answers

**Status:** Accepted — implemented 2026-10-11 (devlog_254)
**Author:** EKOS team
**Created:** 2026-10-11
**Builds on:** RFC 0138 (eval harness), RFC 0139 (grading), RFC 0140/0141 (what evidence carries)

## Motivation

A full run on `deepseek-v4.1-flash` scored 88 of 101. The report named three problems:
retrieval, refusals (15% fabrication) and 11 uncited answers. Each failing scenario was traced to
its cause from the saved transcript.

| Reported | Cause |
|---|---|
| 11 answers uncited | All 11 are correct refusals. `answered_uncited` counted every answer without a citation. |
| 3 of 20 refusal scenarios fabricated | All 3 rejected the false premise in their own words (`adv-004`, `adv-011`, `adv-015`). The grader knows a fixed phrase list and none matched. |
| 6 retrieval and 3 generation failures | Nine causes, listed under Design §3. |

A failed run earlier the same day (the provider answered `403` to 90 of 91 calls) showed that a
run where every model call errors can still print `PASS`.

## Design

### 1. A refusal is not an uncited answer

`answered_uncited` is true only for an answer that is not a refusal. The report counts refusals
on their own line.

### 2. A premise rejection is a stated outcome

The reason prompt already fixes the opening words of a refusal (`Insufficient evidence.`). It
gains a second fixed opening, `Premise not supported.`, for a question that asserts something the
claims do not support or contradict. A cited correction may follow.

The grader accepts that opening as a refusal. It does not gain loose phrases such as "does not":
those appear in fabricated answers too. A citation must still be valid.

### 3. Retrieval

Each cause below was found in a saved transcript and each change was measured with §5.

| | Cause | Change |
|---|---|---|
| A | The excerpt shown for a search hit was the opening of the object's text. A section was found by a sentence the model never saw. | The claim shows the window of the text holding the most distinct query words (`reason.rs::matched_window`). |
| B | A symbol's doc comment and signature were not searchable. `KirObject::indexed_content` held the excerpt, symbol names, OCR text and LLM overview, and nothing an author wrote about a symbol. | `description`, `signature` and `fields` are indexed. Rust struct and enum symbols record `fields`. |
| C | An existing index does not gain newly indexed text until each object is rewritten. | The search index records a content version; a writable open under a different version wipes and replays it, as a stale schema already does. |
| D | The partial-match band (hits that hold some of the query's words) was ordered by BM25 alone, so a short name holding one word outranked a passage holding most of them. | The band is fetched four times wider and ordered by BM25 rank fused with coverage rank (reciprocal rank, k = 60). Strict hits are untouched. |
| E | The "full source text" of a symbol (RFC 0140 §3) was never attached on a real ledger: 0 of 101 scenarios. The lookup expects the observation artifact's id and a commit records the knowledge artifact's (`ka:…`). | When the artifact holds no file text, the symbol's own evidence fragment is shown. It is the symbol's source, sliced at recover time. |
| F | Imports are recorded per imported path, so `M::self`, `M::redact` and `M::Config` are separate objects. "What depends on M" answered for `M::self` only. | A requested dependents traversal from `M::self` adds the dependents of every `M::…` object of the same kind, each claim naming the member reached. |
| G | A query equal to a file's name without directory or extension ranked the file under a dozen of its own sections. | The exact-name signal matches a file by its name or stem, unless more than three files share it. |
| H | Documents that quote an eval question match it word for word. | `ekos eval run` leaves out objects whose text holds a scenario's question verbatim and prints how many. `--keep-question-echoes` turns it off. |
| I | Recall@10 resolved each expected name to one id. Two objects can bear one name (a file and the document read from it); the map kept whichever came last. | An expected name is found when any object bearing it is in the top ten. A name that resolves to nothing is a miss. |

H applies to `ekos eval run` only. It is the one place the eval treats its corpus differently from
production, so the count is printed with the run. On the 2026-09-23 self-workspace it left out 6
objects and changed neither metric.

#### Measured (2026-09-23 self-workspace ledger, no model)

| Step | Recall@10 | Scenarios with every expected fact in the evidence |
|---|---|---|
| Before | 55.9% | 52 / 63 |
| A, E, F | 55.9% | 56 / 63 |
| G, I | 61.8% | 56 / 63 |
| D | 61.8% | 60 / 63 |
| B, C (index rebuilt) | 70.6% | 60 / 63 |

Ordering the band by coverage alone reached the same 60 of 63 and dropped Recall@10 back to
55.9%: a long file that mentions every word once buried the symbol the question describes. The
fused order kept both.

#### Not fixed

- **A file's text past its first ~900 characters is not in the ledger.** `code-004` asks the
  workspace's Rust edition; `edition = "2024"` is on line 69 of `ekos/Cargo.toml`.
- **A symbol has no link to the observation artifact holding its file**, so text beyond the
  40-line evidence fragment cannot be shown.
- **A documentation section that describes a symbol outranks the symbol.** Five scenarios pass
  on the section while the symbol itself is outside the top ten.
- `fields` exist only after the Rust analyzer runs again (pass version `v5`).

### 4. An errored scenario fails the run

A run with any errored scenario reports `FAIL` and says how many errored. Before, every mean was
`None` when every scenario errored, each gate held vacuously, and the run printed `PASS`.

### 5. Measuring without a model

`ekos eval run --evidence-only` runs retrieval and evidence assembly for every scenario and makes
no model call. It reports Recall@10 and, for each scenario with expected facts, whether every fact
is present in the evidence the model would have been shown. This is the number each change in §3
is measured against.

## Result

Same suite, same ledger, `deepseek-v4.1-flash`:

| | Before | After |
|---|---|---|
| Scenarios passed | 88 / 101 | 95 / 101 |
| Answer correctness | 86.2% | 95.8% |
| Evidence groundedness | 96.7% | 95.6% |
| Recall@10 | 55.9% | 70.6% |
| Fabrication (of 20 refusal scenarios) | 15% | 10% |
| Answered but uncited | 11 | 2 |
| Failures attributed to retrieval | 6 | 0 |

The fixed opening in §2 moved three false-premise scenarios to a pass. It was also used on one
real question (`arch-015`), and two correct denials used neither opening (`adv-001`, `sec-003`).

One of the two uncited answers quoted code with an unclosed brace, which hid its citation block
from the brace scanner. The block is now also found by its key (`ai.rs::citation_block_spans`);
that fix came after the run and is covered by a unit test on the measured shape.

## Non-goals

- Changing gates or scenario thresholds.
- A model-based judge.
- Fields for Python, Elixir or JavaScript types.
- Re-recovering the self-workspace: `recover` and `commit` call the configured model, which is
  metered here.

## Verification

- Unit tests for each item, including a fabricated answer containing "does not" that still fails.
- The RFC 0126 retrieval harness does not regress.
- Evidence-only measurement before and after each change in §3, on the existing self-workspace (search index rebuilt for B).
- One full run on the local model and one on `deepseek-v4.1-flash`.
