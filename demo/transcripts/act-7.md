# Act 7 transcript

**Prompt:** Design a CDC architecture for ingesting order data into a lakehouse. Base it on my past work — my prior CDC projects, my mistakes, my lessons.

```
## CDC Architecture for Order Ingestion → Lakehouse

Grounded in four of your own prior projects rather than generic best practice:

**Pipeline:** Azure PostgreSQL (orders DB, `wal_level=logical`) → Debezium on ACI → Event Hub (Kafka-compatible) → Event Hub Capture → ADLS → Bronze (Delta, raw envelopes) → metadata-driven Silver (MERGE + schema evolution) → optional Data Vault 2.0 → dbt Gold → Trino/Databricks SQL serving.

**Why these choices (not generic):**
- **Debezium**, not ADF polling — already running in 3 of your projects; ADF in your stack is reserved for full-load bronze copy only, never incremental CDC.
- **Event Hub Capture → ADLS** instead of a standing Spark streaming consumer — an Azure-cost optimization you already designed in `azure-data-project`, avoiding the hand-rolled watermark/checkpoint management that bit you in `ai-data-platform-engineer`.
- **Metadata-driven Silver MERGE** — lifted almost verbatim from `opncodetest/SILVER_METADATA_REFACTOR.md`, which is literally about an `orders`/`products` pipeline.
- **dbt for Gold** — same pattern in both `databricks-lab` and `opncodetest/cdc_gold`, including freshness/uniqueness tests you can reuse directly.

**Your own past mistakes to avoid this time:**
- `wal_level=logical` must be set before Debezium connects (you hit this).
- Non-idempotent connector/bucket bootstrap, and trusting Spark exit codes over `_delta_log` evidence — both flagged as anti-patterns in your `CROSS_MODEL_CDC_RECOVERY_PLAN.md`.
- Event-time-only dedup is too weak — use `(pk, event_time, event_ts_ms, bronze_offset)`.
- Debezium encodes Postgres `NUMERIC` (order prices/amounts) as base64 — needs explicit decoding, documented in `databricks-lab/CLAUDE.md`.
- Pipeline SP needs **User Access Administrator** on the RG before Terraform can create role assignments — this is already in your memory and applies here too.
- Your Azure CDC stack is currently *disabled* because Postgres Flexible Server is offer-restricted on your subscription — resolve that account-level blocker before standing this up for real.

**Gaps with no prior art** (be aware these are new ground, not proven patterns): real production-scale throughput (everything you've built is lab-scale), zero-downtime backfill/re-snapshot runbook for a live orders table, PII/GDPR handling for payment data (only a roadmap exists, not built), and multi-region DR.

Full detail with file citations is above from the estate-architect agent if you want to dig into any specific component.
```
