//! Structured run-log events (RFC 0220).
//!
//! A producer states a step's numbers once, here; the CLI's run-log layer writes each event as one
//! JSON line (`stage`, `msg`, the fields of `data`, `cmd`, `t_ms`). Without that layer an event is
//! an ordinary `tracing` event and costs nothing beyond it.

use serde::Serialize;

/// The `tracing` target of a run-log event.
pub const TARGET: &str = "ekos_run_log";
/// The `tracing` target of one model call (or cache hit), counted on the run's `llm` line.
pub const MODEL_CALL_TARGET: &str = "ekos_model_call";

/// Model-call kinds a [`model_call`] event names.
pub const COMPLETION: &str = "completion";
pub const CACHE_HIT: &str = "cache_hit";
pub const EMBEDDING: &str = "embedding";

/// One run-log line: `stage`, `msg`, and the fields of `data` (an object; any other value is kept
/// under `data`).
pub fn emit(stage: &str, msg: &str, data: &impl Serialize) {
    let data = serde_json::to_string(data).unwrap_or_else(|_| "null".into());
    tracing::info!(target: "ekos_run_log", stage, data = data.as_str(), "{msg}");
}

/// One model call: `kind` is [`COMPLETION`], [`CACHE_HIT`] or [`EMBEDDING`].
pub fn model_call(kind: &str, provider: &str, model: &str, ok: bool) {
    tracing::info!(target: "ekos_model_call", kind, provider, model, ok, "model call");
}
