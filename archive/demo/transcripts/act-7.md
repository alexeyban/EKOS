# Act 7 transcript

**Prompt:** Design a CDC architecture for ingesting order data into a lakehouse. Base it on my past work — my prior CDC projects, my mistakes, my lessons.

```
The estate-architect agent found **three prior CDC implementations** in your ledger and built the design directly on top of them, rather than from generic best practice. Here's the design:

## Prior art it found
1. **`databricks-lab` (gitlab)** — full production-grade lab: Debezium/Kafka/Postgres → Bronze → Silver → Data Vault 2.0 → Gold, with DQ monitoring, schema-drift policy engine, and GDPR/PII erasure pipeline.
2. **`opncodetest`** — orders-specific: Debezium → Kafka → Bronze → Silver (`silver_orders`) → dbt Gold, with a metadata-driven refactor history.
3. **`azure-data-project`** — same Debezium pattern, but transported via Azure Event Hub (Kafka protocol, SASL_SSL) instead of local Kafka.

## Proposed architecture (grounded)

**1. Capture** — Debezium `pgoutput` logical replication on Postgres (exact connector config reused from `opncodetest`/`postgres-connector.json`), with `heartbeat.interval.ms=30000` from the Azure variant to detect stalled slots.

**2. Transport** — Kafka Connect. Scar to avoid: `databricks-lab/ROADMAP.md` documents that direct `spark.readStream.format("kafka")` couldn't reach Docker-hosted Kafka from Databricks Serverless (ngrok tunnel limitation) — they had to build a Kafka→Volume producer + Auto Loader workaround. Plan for that decoupling upfront if targeting Serverless, or move straight to cloud Kafka (Confluent/MSK) as the roadmap recommends.

**3. Bronze** — Envelope validation with quarantine (not silent drops), `mergeSchema=true` paired with a separate drift-validation gate, 30-day TTL, and volume-anomaly detection (WARN <50% of 7-day avg, FAIL at zero rows) — all lifted directly from `NB_ingest_to_bronze.ipynb`.

**4. Silver / dedup** — **Key lesson**: `SILVER_METADATA_REFACTOR.md` and `DQ_INCIDENT_RUNBOOK.md` document a real production incident — deduping on `event_time` alone caused `pk_uniqueness FAIL` from duplicate PKs in MERGE, because event timestamps aren't guaranteed unique/monotonic. The fix already adopted: order by `(event_time, event_ts_ms, bronze_offset)` — Kafka offset as tie-breaker. Use the three-column version, not `opncodetest`'s original one-column version.

**5. Schema evolution** — Reuse the `additive_only` policy engine (`NB_schema_drift_helpers.ipynb`): new columns pass through, removals/type changes hard-block the pipeline. Also reuse the already-solved Debezium decimal decoding (base64 + scale) for `price` fields.

**6. Gold** — Start with `opncodetest`'s simpler Silver→Gold dbt shape (with freshness SLAs) rather than `databricks-lab`'s full Data Vault 2.0 layer — that layer was built for a 15-table normalized schema and is overkill unless you're conforming multiple order-adjacent sources.

**7. Monitoring** — Reuse `DQ_INCIDENT_RUNBOOK.md` wholesale: quarantine categories, escalation SLAs (Silver PK-uniqueness failure → 1hr response), and a Gold reconciliation test modeled on the existing payment-totals check.

**Explicitly flagged as new ground, not history**: Postgres replication-slot/WAL disk-growth monitoring (no prior evidence of this being solved), and whether to add the DV2.0 conformance layer for a single-source orders pipeline (a scope call, not a pattern from your past work).

Want me to write this up as an RFC under `docs/rfcs/` per this repo's workflow, or is it meant for one of those other projects (`opncodetest`/`databricks-lab`)?
```
