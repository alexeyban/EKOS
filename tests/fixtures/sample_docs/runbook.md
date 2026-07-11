# Runbook: Order Service

## Common incidents

### Orders stuck in "pending"

Usually caused by the payment webhook not firing. Check the payment
provider's dashboard for failed webhook deliveries, then manually replay
the event from the admin console.

### High latency on order lookup

Check whether the read replica is lagging. If lag exceeds 30s, fail over
to the primary until the replica catches up.

## Escalation

Page the on-call engineer if an incident is customer-facing and unresolved
after 15 minutes.
