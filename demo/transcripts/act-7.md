# Act 7 transcript

**Prompt:** Design a CDC architecture for ingesting order data into a lakehouse. Base it on my past work — my prior CDC projects, my mistakes, my lessons.

```
The design is grounded in your own three prior CDC builds. Here's the full architecture:

# CDC Architecture for Order Data into a Lakehouse

Grounded in evidence from three prior projects:
- **`opncodetest`** — metadata-driven Kafka→Bronze→Silver pipeline, Silver notebook literally titled "Silver Layer - Orders Processing," plus a `cdc_gold/` dbt project with order revenue models and tests.
- **`databricks-lab`** — Debezium→Kafka→Bronze→Silver→Data Vault 2.0→dbt Gold, including a generated `HUB_ORDERS`/`sat_orders_core`/`sat_orders_pricing` Data Vault model.
- **`ai-data-platform-engineer`** — an earlier, cruder CDC build plus a hard-won postmortem (`AI_Session_Analysis_Report.md`) documenting real production bugs.

No prior EKOS/Pentaho-based CDC work was found — everything below is sourced from the three repos above, explicitly labeled where it's new ground.

---

## 1. Source Capture
**Reuse: Debezium logical replication** (`databricks-lab/docs/architecture.md`) — PostgreSQL `wal_level=logical`, Kafka Connect, `pgoutput`, one topic per table for `orders`, `order_items`, and dimension tables.
**Lesson baked in:** a missing `topic.prefix` silently prevented topic creation before (`AI_Session_Analysis_Report.md` E11) — it's now mandatory in the connector template.

## 2. Transport / Streaming
**Reuse: Kafka + Spark Structured Streaming** with `subscribePattern` (`cdc.public.*`) so new order-adjacent tables auto-attach without redeploy.
**Known gotcha:** Databricks Serverless can't reach a local ngrok-tunneled Kafka — the proven fallback is the `kafka-to-volume` Docker profile (Auto Loader off a Volume instead of direct Kafka).
**No prior art:** cloud-managed Kafka (MSK/Confluent Cloud) production topology — only ever mentioned, never built. Use general best practice here.

## 3. Landing/Staging (Bronze)
**Reuse:** raw Debezium envelope, append-only, one Delta table per source table, `mergeSchema=true`, schema-drift check before write.
**Explicitly avoid:** the earlier `cdc_to_delta_gold.py` pattern of batch `overwrite` reads instead of streaming `foreachBatch`/MERGE — that repo's own postmortem documents this as buggy and non-idempotent.

## 4. Schema Evolution
**Reuse:** the `NB_schema_drift_helpers.ipynb` module (already validated against `bronze.orders` in its own example) — `additive_only` policy, hard-stops on breaking drift, logs to `monitoring.schema_drift_log`.
**Real precedent to design around:** order data actually had a column rename (`product`→`product_id`) mid-project, handled via `coalesce()` across old/new names. Build that coalesce-across-renames pattern into Silver from day one — renames, not just additions, are the realistic risk.

## 5. Deduplication / Upsert
**Reuse:** window function (`row_number()` over `event_time desc`, partitioned by order key) → keep latest → `MERGE INTO` Delta.
**Mistake to avoid:** the earlier build used `dropDuplicates()`, which can keep an arbitrary version of a row rather than the latest — wrong for order status/price updates. Watermark late arrivals (`withWatermark`, ~10 min, matching prior usage).

## 6. Medallion Layering
Bronze → Silver → (optional Data Vault) → Gold, exactly as built twice already. Gold built with dbt, reusing the `total_products_order.sql` join pattern and its grain-uniqueness/positive-amount test patterns.
**Real design choice, not automatic:** Data Vault adds real cost/complexity (13 hubs/19 links/15 sats in the prior build) for audit/time-travel value — skip it if full historization isn't required, as `opncodetest` itself does.

## 7. Backfill Strategy
**Reuse:** full-reset-with-dry-run notebook pattern (`NB_reset_tables.ipynb`) for nuclear resets; Debezium **signal table** for incremental/partial backfills without connector restart.
**No prior art:** precise point-in-time replay via Delta time-travel + Kafka offset rewind — never done here, design fresh with general best practice.

## 8. Monitoring / Data Quality
**Reuse:** `monitoring` schema — `schema_drift_log`, `dq_results`, `pii_column_registry` (order data has PII — register those columns immediately).
**Lessons baked in:** don't trust subprocess/Spark exit codes as success signals (Spark can exit 1 on successful runs); verify via row counts/`_delta_log`. Make all setup/registration scripts idempotent — non-idempotent Debezium/bucket/BI setup broke on re-run before.

---

### Mistakes explicitly not being repeated
| Past mistake | Fix in this design |
|---|---|
| Missing `topic.prefix` | Hardcoded in connector template |
| `dropDuplicates()` instead of latest-wins merge | Windowed row_number + MERGE |
| Trusting exit codes | Row-count/_delta_log verification |
| Non-idempotent setup scripts | Check-before-create everywhere |
| Serverless/ngrok Kafka incompatibility | `kafka-to-volume` fallback called out upfront |
| Silent break on column rename | Coalesce-across-renames in Silver |

### Marked "no prior art — general best practice"
- Cloud-managed Kafka production topology/IAM
- Point-in-time replay via Delta time-travel + Kafka offset rewind

Full file citations available if you want to go pull the actual notebooks/SQL referenced (e.g. `opncodetest/notebooks/silver/NB_process_to_silver.ipynb`, `databricks-lab/docs/architecture.md`, `ai-data-platform-engineer/AI_Session_Analysis_Report.md`).
```
