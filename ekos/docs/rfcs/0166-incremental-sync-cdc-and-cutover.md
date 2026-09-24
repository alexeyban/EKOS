# RFC 0166 — Incremental sync, CDC, parallel run and cutover

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation, state machine), RFC 0156 (rolling validation), RFC 0157 (source
safety, watermark candidates), RFC 0160 (chunked execution), RFC 0161 (R4 approvals),
RFC 0162 (the report that gates sign-off), RFC 0165 (Delta merge path)

---

## Summary

The last mile: keeping the target current while the source keeps changing, running both systems in
parallel under continuous validation, and a cutover whose checklist is compiled from ledger state
rather than typed into a wiki page.

## Motivation

Everything before this RFC validates a **snapshot**. Real migrations do not get a snapshot — the
source keeps taking writes for weeks while the target is built, tested and trusted.

That gap is where confidence is actually won or lost. A single V4 pass proves the target matched the
source once. A month of rolling validation with zero unexplained divergences on live traffic is what
lets somebody sign their name against switching the old system off.

It is also where the two hardest problems live, and neither has a clever solution — only a careful
one: **deletes**, which a watermark cannot see, and **the moment of cutover**, where in-flight writes
can be lost between the two systems.

## Design

### Incremental by watermark

The default. A monotonic column identified by RFC 0157's monotonicity profiling — `updated_at`, a
sequence-backed id, a version counter — bounds each sync batch.

```
low  = last committed watermark for the unit
high = a value read from the source now, minus the late-arrival window
copy rows where watermark ∈ (low, high], chunked as in RFC 0160
commit the new watermark only after the batch's own V1 count check passes
```

Two details that are the whole difficulty:

- **The late-arrival window.** A transaction that began before `high` and commits after it writes a
  row whose `updated_at` is below a watermark already passed. The window (default: the source's
  longest observed transaction duration, rounded up, with a policy floor) holds `high` back so those
  rows are still caught. The observed duration is measured, not guessed, and recorded.
- **Clock skew.** A wall-clock watermark on a source with multiple writers is not reliably monotonic.
  Where available, `pg_current_snapshot()`/`xmin` bounds are preferred over timestamps precisely
  because they are transactional rather than temporal; a timestamp watermark is used only when
  nothing better exists, and the fact says which was used.

### Deletes

A watermark cannot see a deleted row: there is nothing left to carry a watermark. Three strategies,
per unit, chosen by evidence and recorded as a design decision:

| Strategy | When | Cost |
|---|---|---|
| **Soft-delete column** | Source marks rather than removes (profile finds `deleted_at`/`is_deleted`) | free — deletes are ordinary updates |
| **Periodic key-set diff** | Small to medium tables, no soft delete | RFC 0156's V4 machinery on keys only: bucket the PK column on both sides, bisect to find target-only keys |
| **CDC** | Large tables where a key-set diff is too expensive | see below |

The key-set diff is the pragmatic default because it reuses machinery that already exists and is
already control-tested. It is deliberately the same code path as V4 rather than a parallel
implementation, so a bug in bucketing shows up in both places and gets fixed once.

### CDC via logical replication

For units where watermarks and key-set diffs are insufficient — high churn, hard deletes, or a
requirement for low replication lag.

- A logical replication slot with the `pgoutput` plugin, consumed by EKOS and batched into target
  writes (`ReplacingMergeTree` inserts, or `MERGE INTO` for Delta).
- **`REPLICA IDENTITY` is a compatibility finding, checked up front.** A table at the default
  `REPLICA IDENTITY` emits only the primary key for `UPDATE`/`DELETE`; without `FULL` (or a suitable
  index identity), before-images are unavailable and some reconciliation is impossible. This must be
  known before the slot is created, not discovered mid-run.
- **Slot lag is monitored and alarmed.** An unconsumed replication slot accumulates WAL and can fill
  the source's disk — this is a genuine way to take down a production database, so the consumer
  reports lag as a first-class metric and the cutover checklist refuses to proceed with an unhealthy
  slot. A slot EKOS created is dropped on teardown, and orphaned slots are reported by `ekos doctor`.
- ClickHouse's experimental `MaterializedPostgreSQL` engine is **not** used: it moves the slot
  lifecycle inside the target engine where EKOS cannot monitor or bound it.

### Parallel run

Both systems live for a window, with the source authoritative.

- **Rolling validation**: V1–V3 on a rotating subset of partitions or key ranges, sized so the whole
  unit is covered within the policy window. V5 runs against live data on migrated logic objects.
- **A trend, not a snapshot.** Divergences per day, by class, per unit — the thing a data owner
  actually reads before agreeing to cut over. A clean single run means much less than a flat line
  across three weeks.
- **Divergences during parallel run are expected and informative.** Replication lag produces
  transient divergences that resolve; a persistent one does not. The classifier distinguishes them by
  re-checking after the lag window, and only a divergence that survives re-check counts against the
  window.

### Cutover

The checklist is **compiled from ledger state**, not maintained by hand:

```
every unit in scope is `validated` or `signed_off`
zero unexplained divergences across the full parallel-run window
every R3 finding has an approved disposition
every required validation tier has run, and every planted control fired
the completeness check (RFC 0158) passes: no unclassified source object
report groundedness ≥ threshold (RFC 0162)
CDC slot healthy, lag within policy   [if CDC in use]
```

Each item renders with the query behind it, so a failing item is immediately specific — "3 units in
`diverged`, listed" rather than "validation incomplete".

**The cutover itself is a human action.** EKOS does not stop writes, flip a DNS record or change an
application's connection string. What it does is sequence and record: a final sync to a quiesced
source, a final V1–V3 on the affected units, the recorded LSN or watermark at which the source was
quiesced, and the R4 sign-off fact with two approvers and typed confirmation.

The final-sync LSN is recorded because it is the answer to the only question that matters if
something goes wrong afterwards: exactly which source state the target is known to match.

**Rollback** is a plan, not a feature. EKOS records what was executed with enough precision to
reverse it where reversal is possible, and states plainly where it is not — data written to the
target after cutover has no source equivalent, and no tool can invent one. The rollback section of
the report says what returning to PostgreSQL would cost at each point after cutover, which is the
honest form of this promise.

## Testing

- Late arrivals: a fixture with a transaction committing after its watermark is still captured.
- Clock skew: a multi-writer fixture with skewed clocks does not lose rows under the xmin-based
  watermark, and the timestamp-based one is shown to lose them — the test documents *why* xmin is
  preferred.
- Deletes: each of the three strategies detects a deleted row; the key-set diff names the exact key.
- CDC: a `REPLICA IDENTITY DEFAULT` table produces the compatibility finding before slot creation;
  slot lag is reported; the slot is dropped on teardown; an orphaned slot is reported by `doctor`.
- Parallel run: a seven-day simulated run against a write-active corpus, with injected lag, ends with
  zero unexplained divergences and a trend series.
- Transient vs persistent: a lag-induced divergence resolves on re-check; an injected persistent one
  does not, and blocks.
- Cutover: each checklist item fails independently, and sign-off is refused while any fails.

## Alternatives considered

- **CDC as the default for everything.** Rejected: replication slots are operationally risky on a
  production source, and most units are served perfectly well by a watermark plus a key-set diff.
  CDC is a considered escalation with a named reason.
- **ClickHouse `MaterializedPostgreSQL`.** Rejected as above — experimental, and it hides slot
  lifecycle from the component responsible for the source's safety.
- **Dual writes from the application during parallel run.** Rejected: it is an application change
  EKOS cannot verify, and it introduces a new consistency problem to solve on top of the one being
  solved.
- **Automated cutover once the checklist is green.** Rejected by RFC 0154's non-goal. The checklist
  being green is a necessary condition for a human decision, not a substitute for one.
- **Timestamp watermarks everywhere for simplicity.** Rejected: silently lossy under clock skew,
  which is the worst possible property for the component that decides which rows get copied.

## Open questions

- [ ] Late-arrival window default when no transaction-duration history exists.
- [ ] Key-set diff cadence during parallel run — per cycle, or nightly?
- [ ] Should EKOS support a read-only "shadow" mode where the target serves queries during parallel
      run and results are compared to the source's, as a V6-adjacent tier?

## Acceptance criteria

- [ ] Late-arriving and skewed-clock writes are captured; the failure mode of the naive approach is
      demonstrated by test.
- [ ] Each delete strategy detects a deleted row and names it.
- [ ] `REPLICA IDENTITY` is checked before slot creation; slot lag is monitored; slots are cleaned up.
- [ ] A seven-day simulated parallel run on a write-active corpus ends with zero unexplained
      divergences and a reported trend.
- [ ] Every cutover checklist item is compiled from a ledger query and independently blocks sign-off.
- [ ] The final-sync LSN or watermark is recorded on the sign-off fact.
