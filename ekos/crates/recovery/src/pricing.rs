//! Model prices and data policy (RFC 0197 §4–5).
//!
//! The built-in table is OpenCode Zen's published per-million-token prices as the maintainer read
//! them on [`PRICES_AS_OF`]; `[llm.prices."<model>"]` overrides an entry or adds one. A metered
//! model missing from both has an **unknown** price — never treated as free.

use ekos_compiler_core::config::LlmPrice;
use std::collections::BTreeMap;

/// When the built-in table was read.
pub const PRICES_AS_OF: &str = "2026-10-17";

/// `(model, input, output, cache read, cache write)` in US dollars per million tokens.
type PriceRow = (&'static str, f64, f64, Option<f64>, Option<f64>);

const ZEN: &[PriceRow] = &[
    ("claude-fable-5", 10.0, 50.0, Some(1.0), Some(12.5)),
    ("claude-fable-5-1", 10.0, 50.0, Some(0.25), Some(12.5)),
    ("claude-haiku-4-5", 1.0, 5.0, Some(0.10), Some(1.25)),
    ("claude-haiku-5-5", 0.10, 0.50, Some(0.01), Some(0.13)),
    ("claude-opus-4-5", 5.0, 25.0, Some(0.50), Some(6.25)),
    ("claude-opus-4-6", 5.0, 25.0, Some(0.50), Some(6.25)),
    ("claude-opus-4-7", 5.0, 25.0, Some(0.50), Some(6.25)),
    ("claude-opus-4-8", 5.0, 25.0, Some(0.50), Some(6.25)),
    ("claude-opus-5", 5.0, 25.0, Some(0.50), Some(6.25)),
    ("claude-opus-5-5", 4.0, 20.0, Some(0.20), Some(5.0)),
    ("claude-sonnet-4", 3.0, 15.0, Some(0.30), Some(3.75)),
    ("claude-sonnet-4-5", 3.0, 15.0, Some(0.30), Some(3.75)),
    ("claude-sonnet-4-6", 3.0, 15.0, Some(0.30), Some(3.75)),
    ("claude-sonnet-5", 2.0, 10.0, Some(0.20), Some(2.50)),
    ("claude-sonnet-5-5", 2.0, 10.0, Some(0.20), Some(2.50)),
    ("deepseek-v4-flash", 0.14, 0.28, Some(0.03), None),
    ("deepseek-v4-pro", 1.74, 3.84, Some(0.15), None),
    ("deepseek-v4.1-flash", 0.30, 1.20, Some(0.01), None),
    ("gemini-3-flash", 0.50, 3.0, Some(0.05), None),
    ("gemini-3.1-pro", 2.0, 12.0, Some(0.20), None),
    ("gemini-3.5-flash", 1.50, 9.0, Some(0.15), None),
    ("gemini-3.5-flash-lite", 0.30, 2.50, Some(0.03), None),
    ("glm-5", 1.0, 3.20, Some(0.20), None),
    ("glm-5.1", 1.40, 4.40, Some(0.26), None),
    ("glm-5.2", 1.40, 4.40, Some(0.26), None),
    ("glm-5.3", 1.40, 4.40, Some(0.26), None),
    ("glm-5.3-flash", 0.15, 0.50, Some(0.03), None),
    ("gpt-5", 1.07, 8.50, Some(0.11), None),
    ("gpt-5-codex", 1.07, 8.50, Some(0.11), None),
    ("gpt-5-nano", 0.05, 0.40, Some(0.01), None),
    ("gpt-5.1", 1.07, 8.50, Some(0.11), None),
    ("gpt-5.1-codex", 1.07, 8.50, Some(0.11), None),
    ("gpt-5.1-codex-max", 1.25, 10.0, Some(0.13), None),
    ("gpt-5.1-codex-mini", 0.25, 2.0, Some(0.03), None),
    ("gpt-5.2", 1.75, 14.0, Some(0.18), None),
    ("gpt-5.2-codex", 1.75, 14.0, Some(0.18), None),
    ("gpt-5.3-codex", 1.75, 14.0, Some(0.18), None),
    ("gpt-5.4", 2.50, 15.0, Some(0.25), None),
    ("gpt-5.4-mini", 0.75, 4.50, Some(0.08), None),
    ("gpt-5.4-nano", 0.20, 1.25, Some(0.02), None),
    ("gpt-5.5", 5.0, 30.0, Some(0.50), None),
    ("gpt-5.6-luna", 0.20, 1.20, Some(0.02), Some(0.25)),
    ("gpt-5.6-sol", 4.0, 20.0, Some(0.40), Some(5.0)),
    ("gpt-5.6-terra", 2.50, 15.0, Some(0.25), Some(3.13)),
    ("gpt-6-astra", 10.0, 50.0, Some(1.0), Some(12.5)),
    ("gpt-6-luna", 0.10, 0.50, Some(0.01), Some(0.13)),
    ("gpt-6-sol", 2.0, 10.0, Some(0.20), Some(2.50)),
    ("gpt-6.1-sol", 2.0, 10.0, Some(0.10), Some(2.50)),
    ("grok-4.5", 2.0, 6.0, Some(0.30), None),
    ("grok-4.6", 2.0, 6.0, Some(0.50), None),
    ("grok-4.7", 2.0, 6.0, Some(0.50), None),
    ("grok-build-0.1", 1.0, 2.0, Some(0.20), None),
    ("kimi-k2.5", 0.60, 3.0, Some(0.08), None),
    ("kimi-k2.6", 0.95, 4.0, Some(0.16), None),
    ("kimi-k2.7-code", 0.95, 4.0, Some(0.19), None),
    ("kimi-k3", 3.0, 15.0, Some(0.30), None),
    ("minimax-m2.5", 0.30, 1.20, Some(0.06), None),
    ("minimax-m2.7", 0.30, 1.20, Some(0.06), None),
    ("minimax-m3", 0.30, 1.20, Some(0.06), None),
    ("mistral-large-4", 0.68, 2.09, Some(0.07), None),
    ("muse-spark-1.2", 1.25, 4.25, Some(0.15), None),
    ("muse-spark-1.3", 1.25, 4.25, Some(0.15), None),
    ("qwen3.5-plus", 0.20, 1.20, Some(0.02), Some(0.25)),
    ("qwen3.6-plus", 0.50, 3.0, Some(0.05), Some(0.63)),
    ("qwen3.8-flash", 0.15, 0.47, Some(0.02), Some(0.20)),
    ("qwen3.8-max", 2.0, 6.0, Some(0.25), Some(2.50)),
];

/// Models whose provider may keep or train on requests (Zen's docs, read [`PRICES_AS_OF`]).
/// Matched by prefix. Refused unless `[llm] allow-data-collection = true`.
const COLLECTS: &[&str] = &["big-pickle", "mimo-", "nemotron-", "ll-alpha", "exo-"];

/// Whether `model`'s provider may keep or train on what it receives.
pub fn collects_data(model: &str) -> bool {
    COLLECTS.iter().any(|p| model.starts_with(p))
}

/// The price of `model`: `[llm.prices]` first, then the built-in table.
pub fn price(model: &str, overrides: &BTreeMap<String, LlmPrice>) -> Option<LlmPrice> {
    if let Some(p) = overrides.get(model) {
        return Some(*p);
    }
    ZEN.iter()
        .find(|(m, ..)| *m == model)
        .map(|&(_, input, output, cache_read, cache_write)| LlmPrice {
            input,
            output,
            cache_read,
            cache_write,
        })
}

/// US dollars for one call. Cached tokens fall back to the input price when the table has no
/// cache price for them.
pub fn cost(p: &LlmPrice, input: u32, output: u32, cache_read: u32, cache_write: u32) -> f64 {
    let m = 1_000_000.0;
    (input as f64 * p.input
        + output as f64 * p.output
        + cache_read as f64 * p.cache_read.unwrap_or(p.input)
        + cache_write as f64 * p.cache_write.unwrap_or(p.input))
        / m
}

/// A rough input-token count for the budget's worst case: four characters a token, rounded up.
pub fn estimate_tokens(chars: usize) -> u32 {
    chars.div_ceil(4) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_prices_and_overrides() {
        let none = BTreeMap::new();
        let haiku = price("claude-haiku-5-5", &none).unwrap();
        assert_eq!((haiku.input, haiku.output), (0.10, 0.50));
        assert!(price("no-such-model", &none).is_none());
        let mut o = BTreeMap::new();
        o.insert(
            "claude-haiku-5-5".to_string(),
            LlmPrice {
                input: 1.0,
                output: 2.0,
                cache_read: None,
                cache_write: None,
            },
        );
        assert_eq!(price("claude-haiku-5-5", &o).unwrap().input, 1.0);
    }

    #[test]
    fn cost_counts_cached_tokens_at_their_own_price() {
        let p = price("claude-haiku-5-5", &BTreeMap::new()).unwrap();
        // 1M input + 1M output + 1M cache read + 1M cache write.
        let c = cost(&p, 1_000_000, 1_000_000, 1_000_000, 1_000_000);
        assert!((c - (0.10 + 0.50 + 0.01 + 0.13)).abs() < 1e-9);
        let no_cache_price = LlmPrice {
            input: 1.0,
            output: 0.0,
            cache_read: None,
            cache_write: None,
        };
        assert!((cost(&no_cache_price, 0, 0, 1_000_000, 0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn data_collecting_models_are_named() {
        assert!(collects_data("big-pickle"));
        assert!(collects_data("nemotron-3-ultra-free"));
        assert!(!collects_data("glm-5.3-flash"));
        assert!(!collects_data("claude-haiku-5-5"));
    }
}
