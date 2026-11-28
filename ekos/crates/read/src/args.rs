//! Reading a tool's JSON arguments.

use crate::{ReadError, Result};
use ekos_kir::KirId;
use serde_json::Value;
use std::str::FromStr;

/// A string argument that must be present.
pub fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ReadError::Argument(format!("missing required string argument `{key}`")))
}

/// The `id` argument, as an object id.
pub fn required_id(args: &Value) -> Result<KirId> {
    let raw = required_str(args, "id")?;
    KirId::from_str(raw).map_err(|_| ReadError::Argument(format!("invalid object id: {raw}")))
}

/// An optional integer argument, `default` when absent, an error when present but not an integer
/// in `min..=max`.
///
/// Refused rather than clamped: an agent that asked for depth 10 and silently got 3 reasons from a
/// graph it believes is complete. And never `as u32` on the raw `u64` — that wrapped, so
/// `4294967296` hops ran as 0.
pub fn bounded_arg(args: &Value, key: &str, default: u64, min: u64, max: u64) -> Result<u64> {
    let Some(raw) = args.get(key) else {
        return Ok(default);
    };
    let n = raw.as_u64().ok_or_else(|| {
        ReadError::Argument(format!("`{key}` must be a non-negative integer, got {raw}"))
    })?;
    if !(min..=max).contains(&n) {
        return Err(ReadError::Argument(format!(
            "`{key}` is {n}; it must be between {min} and {max}"
        )));
    }
    Ok(n)
}

/// `ekos_neighborhood` bounds. Measured on EKOS's own ledger around one hub object, release build
/// (2026-09-28): depth 2 → 3.6 s, depth 3 → 28 s and 14 MB. Over `--http` a single worker thread
/// serves every client, so one unbounded call stalled all of them.
pub const NEIGHBORHOOD_MAX_DEPTH: u64 = 3;
/// How many objects a neighbourhood returns when the caller does not say.
pub const NEIGHBORHOOD_DEFAULT_OBJECTS: u64 = 500;
/// The most objects a neighbourhood may be asked for.
pub const NEIGHBORHOOD_MAX_OBJECTS: u64 = 5_000;
/// `ekos_impact`'s hop bound. Its default is 5; 20 is well past any real dependency chain.
pub const IMPACT_MAX_HOPS: u64 = 20;
