# Vendored Apache Ossie files

`ossie-schema.json` is copied unchanged from
[apache/ossie](https://github.com/apache/ossie) `core-spec/ossie-schema.json` at commit
`8437d59e07c7fa56fcabc4a16d46864fd88d7313` (2026-11-06), spec version `0.2.0.dev0`.
The test fixtures in `../tests/fixtures/` come from the same repository's `examples/`, at the
same commit. `tpcds_semantic_model.v0_1_1.yaml` comes from the `osi-0.1.1-rc1` tag.

These files are licensed under the Apache License, Version 2.0 (`LICENSE-APACHE-2.0`).

    Apache Ossie (incubating)
    Copyright 2026 The Apache Software Foundation

    This product includes software developed at
    The Apache Software Foundation (http://www.apache.org/).

To update them, copy the new files and bump `SPEC_VERSION` in `src/model.rs` if the schema's
`version` constant changed. The tests validate every export against the copied schema.
