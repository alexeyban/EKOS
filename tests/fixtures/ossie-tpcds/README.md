# Apache Ossie end-to-end fixture (RFC 0245)

A small project for EKOS's Ossie tests (`ekos/crates/cli/tests/ossie_e2e.rs`) and the public
validation script (`scripts/ossie-validate.sh`).

| File | What it is |
|---|---|
| `db/tpcds.sql` | five TPC-DS tables (`date_dim`, `customer`, `item`, `store`, `store_sales`) with keys, foreign keys, a coded column with a `CHECK` and a comment, and one view. Written for this test from the TPC-DS column list; no TPC tooling or data. |
| `semantic/tpcds.ossie.yaml` | Apache Ossie's official example `examples/tpcds_semantic_model.yaml`, unchanged, from apache/ossie@8437d59 (Apache License 2.0; see `ekos/crates/ossie/schema/`). It describes the same five tables. |

The tests copy this directory to a temporary workspace; nothing is ever written here.
