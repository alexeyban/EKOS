# Retention Policy Notes

These notes were taken during the data-platform working session and are kept
here as the working record until the formal policy document lands.

## Scope

The policy covers every table in the analytics warehouse, plus the raw landing
zone that the ingestion jobs write to before transformation. Object storage
buckets used purely as build caches are explicitly out of scope, since nothing
in them is derived from customer records and they are rebuilt from source on
every pipeline run.

## Default Retention

Fact tables are kept for thirty-six months. Dimension tables are kept
indefinitely, because they are small and rebuilding historical joins without
them is impractical. Raw landing-zone files are kept for ninety days, which is
long enough to replay any transformation bug we have actually hit in practice.

## Deletion Mechanics

Deletion runs as a scheduled job rather than a trigger, so that a mistake in the
retention configuration produces a reviewable plan before anything is removed.
The job writes a manifest of every partition it intends to drop, and a human
approves the manifest for anything larger than one percent of the table.

## Exceptions

A table may be granted an exception only with a written justification recorded
against the table's entry in the catalogue. Exceptions expire after one year and
must be renewed explicitly; there is no silent rollover.
