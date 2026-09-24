# RFC 0162 — Evidence-backed migration report and the groundedness gate

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation), RFC 0156 (validation results and controls), RFC 0158
(findings and the completeness check), RFC 0161 (approvals), RFC 0139 (answer quality),
RFC 0035/0037 (`docs-gen` rendering precedent), RFC 0138 (eval harness)

---

## Summary

The migration report is compiled from ledger queries, not written by a model. An LLM may write
connective prose; every factual sentence must carry a citation to a fact id, and the report compiler
verifies each citation resolves and supports the claim. A report below the groundedness threshold
cannot be signed off.

## Motivation

The report is the deliverable. It is what a data owner reads before agreeing that a decade-old
system can be switched off, and what an auditor reads two years later when a number looks wrong.

A generated narrative that is 95% accurate is worthless for that purpose, because the reader cannot
tell which 5%. The only useful property is that every claim is traceable, and the only way to have
that property reliably is to compile the report from facts and refuse to ship one that is not.

EKOS already has both halves: `docs-gen` renders deterministically from the ledger with zero LLM
calls, and `crates/evals/src/evaluators/groundedness.rs` scores citation coverage and validity. This
RFC composes them for the migration case.

## Design

### Compilation, not generation

The report is a sequence of sections, each a **query plus a template**. The query runs against the
ledger; the template renders its rows. No section's factual content originates in a model.

| Section | Source |
|---|---|
| Executive summary | Counts and states by unit; open risks; total unexplained divergences |
| Scope and inventory | Catalog objects by kind, with the completeness check's numerator and denominator |
| Data-quality findings | `MigrationDqFinding` joined to its `MigrationDisposition` and approver |
| Compatibility and lossiness | `MigrationCompatFinding` with affected rows and the decision taken |
| Target design | `MigrationTargetDesign` with the evidence that motivated each choice |
| Logic migration | Per object: IR diff, fidelity label, V5 result (RFC 0164) |
| Validation matrix | unit × tier × result × **controls fired** |
| Divergences | expected / explained / unexplained, with masked examples |
| Approvals log | Every `MigrationApprovalRecord` with actor, risk class and evidence hash |
| Open risks | Blocking findings without a disposition; units below their required tier |
| Sign-off | The `MigrationReport` snapshot hash and its signatories |

The validation matrix prints the control column next to the result column deliberately. "V3 passed"
and "V3 passed and caught all eleven planted defects" are different claims, and only the second is
worth anything — putting them side by side makes a report that omits controls look as thin as it is.

### Prose, and the line it may not cross

An LLM may write: section introductions, transitions, and a plain-language restatement of a finding
*that is already rendered from facts beside it*. It runs through the existing `ekos ask` grounding
and citation pipeline (`runtime/src/ai.rs`), the same path `docs-gen --prose` uses.

It may not: introduce a number, a name, a count, a date or a causal claim that is not in the
compiled content. The verifier enforces this rather than trusting the prompt.

### Citation verification

Every factual sentence carries one or more fact ids. The compiler then:

1. **Resolves** each id in the ledger snapshot the report was compiled from. An unresolvable id is a
   hard error.
2. **Checks support** — the cited fact's type must match the claim's shape. A row count cites a
   `MigrationValidationResult`, not a `MigrationTargetDesign`. This is a structural check, not a
   semantic one, and it is deliberately conservative: it catches the common failure where a model
   attaches a nearby plausible id.
3. **Scores** using the three-component metric already implemented: citation coverage × citation
   validity × grounded-answer rate.

Below the policy threshold, the report is produced and marked **not signable**, with the failing
sentences listed. It is not silently downgraded, and it is not withheld — an ungrounded report is
still useful to the person fixing it.

### Snapshot and immutability

A report is compiled against a specific ledger state. The `MigrationReport` fact records:

```
version            monotonic per project
snapshot           the ledger state (as-of timestamp + content hash)
groundedness       the three components and the product
controls_summary   tiers run, controls fired, controls missed
signed_off_by      OIDC subjects, once signed
report_hash        hash of the rendered output
```

Recompiling from the same snapshot must produce byte-identical output — the same determinism
requirement `docs-gen` already meets, and the reason a signed report can be re-derived rather than
merely archived. A test asserts it.

Sign-off is R4 (RFC 0161): two approvers, typed confirmation, and it is refused outright when any of
these is true — unexplained divergences exist, any required tier has not run, any control was
missed, the completeness check (RFC 0158) fails, or groundedness is below threshold. Five mechanical
preconditions, none of them a matter of judgement at the moment of signing.

### Outputs

Markdown (default), HTML (console view with clickable citations into the evidence panel, reusing the
RFC 0127 console's existing evidence panel), and PDF. The same compiled content renders all three;
only the renderer differs.

## Testing

- Determinism: two compilations from the same snapshot are byte-identical.
- Citation: a report with a deliberately unresolvable id fails hard; one with a type-mismatched
  citation fails the support check.
- Gate: a report below the groundedness threshold is marked not signable and cannot be signed.
- Preconditions: each of the five sign-off blockers is tested independently.
- Controls column: a run where a control was missed renders as failed in the matrix and blocks
  sign-off.
- Prose: an LLM-introduced number with no citation is caught by the verifier, using a fixture prose
  block crafted to contain one.

## Alternatives considered

- **LLM writes the report from the ledger.** Rejected — this is exactly the artifact where a
  plausible-sounding wrong number does the most damage, and it is the failure mode the whole system
  is built to avoid.
- **Citations as footnote links only, without verification.** Rejected: unverified citations are
  worse than none, because they manufacture trust.
- **Semantic entailment checking of each claim against its citation.** Attractive and deferred: it
  needs a model in the verification loop, which reintroduces the problem one level up. The
  conservative structural check is the v1 answer.
- **Blocking report generation below threshold.** Rejected — the person who has to fix the gaps
  needs to see them.

## Open questions

- [ ] Groundedness threshold value. RFC 0139's existing thresholds are a starting point but the
      migration corpus has a different claim density.
- [ ] Cryptographic signing of a signed-off report, versus hash-in-ledger only. Ties to the
      RFC 0153 release-signing work if that grows a general signing facility.
- [ ] Should the report embed the masked divergence examples, or link to them in the console only?

## Acceptance criteria

- [ ] Report for a LedgerSMB migration compiles, scores above threshold, and every claim resolves to
      a fact.
- [ ] Byte-identical recompilation from the same snapshot.
- [ ] All five sign-off preconditions are independently enforced and tested.
- [ ] The validation matrix shows controls alongside results.
- [ ] An ungrounded report is produced, marked not signable, and lists its failing sentences.
