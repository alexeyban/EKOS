# Sample Project

A small internal service used to exercise the file-observation pipeline in
integration tests. This directory intentionally has no vendored content —
`FileObserver` treats any file as an opaque blob, so realism here doesn't
hinge on the prose being "real"; only the SQL schema and git history fixtures
in `tests/fixtures/` need to be sourced from real open data (see
`git_fixture/NOTICE.md`).

## Overview

This service owns customer order data and exposes a small internal API for
other teams to query order status.

## Getting started

```
cargo run
```

## Support

File an issue in the internal tracker if something looks wrong.
