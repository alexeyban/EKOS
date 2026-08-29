//! Write leases + fencing tokens (RFC 0113 B3, RFC 0111 §9).
//!
//! Exactly one worker may write a partition at a time. A lease is short-TTL and renewed by
//! heartbeat; on expiry the next worker takes over. Every grant carries a **monotonically
//! increasing fencing token** — the coordinator rejects a `manifest_commit` (or renew/release)
//! bearing a stale token, so a slow ex-lease-holder can never overwrite the new one's work.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::protocol::PartitionId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub partition: PartitionId,
    pub holder: String,
    /// Monotonic per partition — increments on every grant.
    pub token: u64,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "detail")]
pub enum LeaseError {
    #[error("partition {0} is already leased")]
    AlreadyLeased(PartitionId),
    #[error("lease on {0} has expired or is not held by you")]
    Expired(PartitionId),
    #[error("fencing token is stale for {0} — a newer lease has been granted")]
    Fenced(PartitionId),
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LeaseTable {
    active: HashMap<PartitionId, Lease>,
    /// The highest token ever handed out per partition (survives lease expiry, so tokens are
    /// strictly monotone forever).
    next_token: HashMap<PartitionId, u64>,
}

impl LeaseTable {
    fn is_live(lease: &Lease, now: DateTime<Utc>) -> bool {
        lease.expires_at > now
    }

    /// Acquire (or take over an expired) lease. `AlreadyLeased` if a *live* lease is held by
    /// someone else.
    pub fn acquire(
        &mut self,
        partition: &str,
        holder: &str,
        now: DateTime<Utc>,
        ttl: chrono::Duration,
    ) -> Result<Lease, LeaseError> {
        if let Some(existing) = self.active.get(partition)
            && Self::is_live(existing, now)
            && existing.holder != holder
        {
            return Err(LeaseError::AlreadyLeased(partition.to_string()));
        }
        let token = {
            let n = self.next_token.entry(partition.to_string()).or_insert(0);
            *n += 1;
            *n
        };
        let lease = Lease {
            partition: partition.to_string(),
            holder: holder.to_string(),
            token,
            expires_at: now + ttl,
        };
        self.active.insert(partition.to_string(), lease.clone());
        Ok(lease)
    }

    /// Verify a lease is still yours and current — the fencing check every mutating call runs.
    pub fn check(
        &self,
        partition: &str,
        holder: &str,
        token: u64,
        now: DateTime<Utc>,
    ) -> Result<(), LeaseError> {
        match self.active.get(partition) {
            Some(l) if l.token > token => Err(LeaseError::Fenced(partition.to_string())),
            Some(l) if l.holder == holder && l.token == token && Self::is_live(l, now) => Ok(()),
            _ => Err(LeaseError::Expired(partition.to_string())),
        }
    }

    pub fn renew(
        &mut self,
        partition: &str,
        holder: &str,
        token: u64,
        now: DateTime<Utc>,
        ttl: chrono::Duration,
    ) -> Result<Lease, LeaseError> {
        self.check(partition, holder, token, now)?;
        let lease = self.active.get_mut(partition).expect("check passed");
        lease.expires_at = now + ttl;
        Ok(lease.clone())
    }

    pub fn release(
        &mut self,
        partition: &str,
        holder: &str,
        token: u64,
        now: DateTime<Utc>,
    ) -> Result<(), LeaseError> {
        // A stale token can't release a newer holder's lease.
        if let Some(l) = self.active.get(partition)
            && l.token > token
        {
            return Err(LeaseError::Fenced(partition.to_string()));
        }
        if let Some(l) = self.active.get(partition)
            && l.holder == holder
            && l.token == token
        {
            let _ = now;
            self.active.remove(partition);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + s, 0).unwrap()
    }
    fn ttl() -> chrono::Duration {
        chrono::Duration::seconds(30)
    }

    #[test]
    fn one_live_lease_at_a_time_but_takeover_after_expiry() {
        let mut lt = LeaseTable::default();
        let a = lt.acquire("p", "worker-a", t(0), ttl()).unwrap();
        assert_eq!(a.token, 1);
        assert_eq!(
            lt.acquire("p", "worker-b", t(10), ttl()),
            Err(LeaseError::AlreadyLeased("p".into()))
        );
        // after the TTL, b takes over with a higher token
        let b = lt.acquire("p", "worker-b", t(31), ttl()).unwrap();
        assert_eq!(b.token, 2);
    }

    #[test]
    fn stale_token_is_fenced() {
        let mut lt = LeaseTable::default();
        let _a = lt.acquire("p", "a", t(0), ttl()).unwrap();
        let _b = lt.acquire("p", "b", t(31), ttl()).unwrap(); // a's lease expired, b takes over
        // a comes back with token 1 → fenced
        assert_eq!(
            lt.check("p", "a", 1, t(32)),
            Err(LeaseError::Fenced("p".into()))
        );
        assert!(lt.check("p", "b", 2, t(32)).is_ok());
    }

    #[test]
    fn renew_extends_only_a_current_lease() {
        let mut lt = LeaseTable::default();
        let a = lt.acquire("p", "a", t(0), ttl()).unwrap();
        let renewed = lt.renew("p", "a", a.token, t(20), ttl()).unwrap();
        assert_eq!(renewed.expires_at, t(50));
        // once expired, renew fails
        assert!(lt.renew("p", "a", a.token, t(100), ttl()).is_err());
    }
}
