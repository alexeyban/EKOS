//! Metering, budget and fallback for LLM calls (RFC 0197 §1, §4, §5).
//!
//! Wrapping order, outermost first: [`FallbackLlmProvider`] over one
//! `CachedLlmProvider(MeteredLlmProvider(provider))` per model — so a disk-cache hit is never
//! metered, budgeted or logged, and each model in a route keeps its own cache entries.

use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmResponse, Task};
use crate::pricing;
use async_trait::async_trait;
use ekos_compiler_core::config::LlmPrice;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Spend shared by every provider one command builds (RFC 0197 §4: the window is the process).
#[derive(Debug, Default)]
pub struct Budget {
    limit_usd: Option<f64>,
    spent_usd: Mutex<f64>,
}

impl Budget {
    pub fn new(limit_usd: Option<f64>) -> Arc<Self> {
        Arc::new(Self {
            limit_usd,
            spent_usd: Mutex::new(0.0),
        })
    }

    pub fn limit_usd(&self) -> Option<f64> {
        self.limit_usd
    }

    /// Settled spend plus whatever calls in flight have reserved.
    pub fn spent_usd(&self) -> f64 {
        *self.spent_usd.lock().expect("budget lock")
    }

    /// Reserve `worst` before a call; refused when it could cross the limit.
    fn reserve(&self, worst: f64) -> Result<(), String> {
        let mut spent = self.spent_usd.lock().expect("budget lock");
        if let Some(limit) = self.limit_usd
            && *spent + worst > limit
        {
            return Err(format!(
                "this call could cost up to ${worst:.4}, and ${:.4} of the ${limit:.2} budget is spent",
                *spent
            ));
        }
        *spent += worst;
        Ok(())
    }

    /// Replace a reservation with what the call actually cost.
    fn settle(&self, reserved: f64, actual: f64) {
        let mut spent = self.spent_usd.lock().expect("budget lock");
        *spent = (*spent - reserved + actual).max(0.0);
    }
}

/// One line of `llm-usage.jsonl`: metadata only, never the prompt or the answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageRecord {
    pub at: String,
    pub task: String,
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_read_tokens: u32,
    #[serde(default)]
    pub cache_write_tokens: u32,
    /// `None` when the price is unknown.
    pub cost_usd: Option<f64>,
    pub latency_ms: u64,
}

/// Read a usage log; unparseable lines are skipped.
pub fn read_usage(path: &std::path::Path) -> std::io::Result<Vec<UsageRecord>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    Ok(text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

/// Meters one model: data policy, budget, usage log.
pub struct MeteredLlmProvider<T> {
    inner: T,
    task: Task,
    provider: String,
    /// `false` for a local model (Ollama): free, so outside the budget.
    metered: bool,
    price: Option<LlmPrice>,
    allow_data_collection: bool,
    budget: Arc<Budget>,
    log: Option<PathBuf>,
}

/// What a [`MeteredLlmProvider`] needs besides the provider it wraps.
pub struct Meter {
    pub task: Task,
    pub provider: String,
    pub metered: bool,
    pub prices: BTreeMap<String, LlmPrice>,
    pub allow_data_collection: bool,
    pub budget: Arc<Budget>,
    pub log: Option<PathBuf>,
}

impl<T: LlmProvider> MeteredLlmProvider<T> {
    pub fn new(inner: T, meter: Meter) -> Self {
        let price = pricing::price(inner.model_name(), &meter.prices);
        Self {
            inner,
            task: meter.task,
            provider: meter.provider,
            metered: meter.metered,
            price,
            allow_data_collection: meter.allow_data_collection,
            budget: meter.budget,
            log: meter.log,
        }
    }

    fn admit(&self, req: &LlmRequest<'_>) -> Result<f64, LlmError> {
        let model = self.inner.model_name();
        if pricing::collects_data(model) && !self.allow_data_collection {
            return Err(LlmError::Policy(format!(
                "{model}'s provider may keep or train on requests; set [llm] allow-data-collection = true to use it"
            )));
        }
        if !self.metered {
            return Ok(0.0);
        }
        let Some(price) = &self.price else {
            if self.budget.limit_usd().is_some() {
                return Err(LlmError::Budget(format!(
                    "no price known for {model}: add [llm.prices.\"{model}\"] or remove budget-usd"
                )));
            }
            return Ok(0.0);
        };
        let chars = req.system.len()
            + req.user.len()
            + req.history.iter().map(|m| m.content.len()).sum::<usize>();
        let worst = pricing::cost(price, pricing::estimate_tokens(chars), req.max_tokens, 0, 0);
        self.budget.reserve(worst).map_err(LlmError::Budget)?;
        Ok(worst)
    }

    fn finish(
        &self,
        req: &LlmRequest<'_>,
        reserved: f64,
        started: Instant,
        result: &Result<LlmResponse, LlmError>,
    ) {
        let cost = match result {
            Ok(r) if self.metered => self.price.map(|p| {
                pricing::cost(
                    &p,
                    r.input_tokens,
                    r.output_tokens,
                    r.cache_read_tokens,
                    r.cache_write_tokens,
                )
            }),
            Ok(_) => Some(0.0),
            Err(_) => Some(0.0),
        };
        self.budget.settle(reserved, cost.unwrap_or(0.0));
        let Some(path) = &self.log else { return };
        let rec = UsageRecord {
            at: chrono::Utc::now().to_rfc3339(),
            task: self.task.as_str().into(),
            provider: self.provider.clone(),
            model: self.inner.model_name().into(),
            prompt_version: req.prompt_version.into(),
            ok: result.is_ok(),
            error: result.as_ref().err().map(|e| {
                let s = e.to_string();
                s.chars().take(300).collect()
            }),
            input_tokens: result.as_ref().map(|r| r.input_tokens).unwrap_or(0),
            output_tokens: result.as_ref().map(|r| r.output_tokens).unwrap_or(0),
            cache_read_tokens: result.as_ref().map(|r| r.cache_read_tokens).unwrap_or(0),
            cache_write_tokens: result.as_ref().map(|r| r.cache_write_tokens).unwrap_or(0),
            cost_usd: cost,
            latency_ms: started.elapsed().as_millis() as u64,
        };
        let line = serde_json::to_string(&rec).expect("a usage record serializes") + "\n";
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?
                .write_all(line.as_bytes())
        };
        if let Err(e) = write() {
            tracing::warn!("cannot append to {}: {e}", path.display());
        }
    }
}

#[async_trait]
impl<T: LlmProvider> LlmProvider for MeteredLlmProvider<T> {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let reserved = self.admit(req)?;
        let started = Instant::now();
        let result = self.inner.complete(req).await;
        self.finish(req, reserved, started, &result);
        result
    }

    async fn complete_stream(
        &self,
        req: &LlmRequest<'_>,
        on_chunk: &mut (dyn FnMut(String) + Send),
    ) -> Result<LlmResponse, LlmError> {
        let reserved = self.admit(req)?;
        let started = Instant::now();
        let result = self.inner.complete_stream(req, on_chunk).await;
        self.finish(req, reserved, started, &result);
        result
    }

    fn cache_namespace(&self) -> Option<String> {
        self.inner.cache_namespace()
    }
}

/// A route's models in order: the next is tried when one fails transiently
/// ([`LlmError::is_transient`]). A streamed answer that already sent text is not retried.
pub struct FallbackLlmProvider {
    chain: Vec<Arc<dyn LlmProvider>>,
}

impl FallbackLlmProvider {
    /// `chain` must not be empty.
    pub fn new(chain: Vec<Arc<dyn LlmProvider>>) -> Self {
        assert!(!chain.is_empty(), "a route has at least one model");
        Self { chain }
    }
}

#[async_trait]
impl LlmProvider for FallbackLlmProvider {
    fn model_name(&self) -> &str {
        self.chain[0].model_name()
    }

    async fn complete(&self, req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let mut last = None;
        for (i, p) in self.chain.iter().enumerate() {
            match p.complete(req).await {
                Ok(r) => return Ok(r),
                Err(e) if e.is_transient() && i + 1 < self.chain.len() => {
                    tracing::warn!(
                        "{} failed ({e}); trying {}",
                        p.model_name(),
                        self.chain[i + 1].model_name()
                    );
                    last = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.expect("the loop returns unless a model failed"))
    }

    async fn complete_stream(
        &self,
        req: &LlmRequest<'_>,
        on_chunk: &mut (dyn FnMut(String) + Send),
    ) -> Result<LlmResponse, LlmError> {
        for (i, p) in self.chain.iter().enumerate() {
            let mut sent = false;
            let mut forward = |c: String| {
                sent = true;
                on_chunk(c)
            };
            match p.complete_stream(req, &mut forward).await {
                Ok(r) => return Ok(r),
                Err(e) if e.is_transient() && !sent && i + 1 < self.chain.len() => {
                    tracing::warn!(
                        "{} failed ({e}); trying {}",
                        p.model_name(),
                        self.chain[i + 1].model_name()
                    );
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!("the last model's result is always returned")
    }

    fn cache_stats(&self) -> Option<(u64, u64)> {
        let mut any = false;
        let (mut h, mut m) = (0, 0);
        for p in &self.chain {
            if let Some((a, b)) = p.cache_stats() {
                any = true;
                h += a;
                m += b;
            }
        }
        any.then_some((h, m))
    }

    fn cache_namespace(&self) -> Option<String> {
        self.chain[0].cache_namespace()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Scripted {
        model: String,
        fail_with: Option<u16>,
        calls: Arc<AtomicU32>,
        usage: (u32, u32),
    }

    #[async_trait]
    impl LlmProvider for Scripted {
        fn model_name(&self) -> &str {
            &self.model
        }
        async fn complete(&self, _req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.fail_with {
                Some(status) => Err(LlmError::Api {
                    status,
                    body: "no".into(),
                }),
                None => Ok(LlmResponse {
                    content: format!("from {}", self.model),
                    model: self.model.clone(),
                    input_tokens: self.usage.0,
                    output_tokens: self.usage.1,
                    ..Default::default()
                }),
            }
        }
    }

    fn scripted(model: &str, fail_with: Option<u16>) -> (Scripted, Arc<AtomicU32>) {
        let calls = Arc::new(AtomicU32::new(0));
        (
            Scripted {
                model: model.into(),
                fail_with,
                calls: calls.clone(),
                usage: (1_000, 1_000),
            },
            calls,
        )
    }

    fn req(max_tokens: u32) -> LlmRequest<'static> {
        LlmRequest {
            system: "s",
            user: "u",
            prompt_version: "test-v1",
            max_tokens,
            history: &[],
        }
    }

    fn meter(budget: Arc<Budget>, log: Option<PathBuf>, metered: bool) -> Meter {
        Meter {
            task: Task::DocSemantics,
            provider: "zen".into(),
            metered,
            prices: BTreeMap::new(),
            allow_data_collection: false,
            budget,
            log,
        }
    }

    #[tokio::test]
    async fn logs_usage_and_cost_but_never_the_text() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("llm-usage.jsonl");
        let (p, _) = scripted("claude-haiku-5-5", None);
        let m = MeteredLlmProvider::new(p, meter(Budget::new(None), Some(log.clone()), true));
        m.complete(&req(100)).await.unwrap();
        let recs = read_usage(&log).unwrap();
        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(
            (r.task.as_str(), r.model.as_str(), r.ok),
            ("doc-semantics", "claude-haiku-5-5", true)
        );
        // 1000 in at $0.10/M + 1000 out at $0.50/M.
        assert!((r.cost_usd.unwrap() - 0.0006).abs() < 1e-12);
        let raw = std::fs::read_to_string(&log).unwrap();
        assert!(
            !raw.contains("from claude"),
            "the answer must not be logged"
        );
    }

    #[tokio::test]
    async fn budget_refuses_a_call_whose_worst_case_could_cross_it() {
        // gpt-6-luna: $0.50/M output; 1M max_tokens worst case = $0.50 > $0.10.
        let (p, calls) = scripted("gpt-6-luna", None);
        let m = MeteredLlmProvider::new(p, meter(Budget::new(Some(0.10)), None, true));
        let err = m.complete(&req(1_000_000)).await.unwrap_err();
        assert!(matches!(err, LlmError::Budget(_)), "{err}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a refused call never reaches the host"
        );
        // A small call fits and settles at its real cost.
        let budget = Budget::new(Some(0.10));
        let (p, _) = scripted("gpt-6-luna", None);
        let m = MeteredLlmProvider::new(p, meter(budget.clone(), None, true));
        m.complete(&req(1_000)).await.unwrap();
        assert!((budget.spent_usd() - 0.0006).abs() < 1e-12);
    }

    #[tokio::test]
    async fn unknown_price_is_refused_only_under_a_budget_and_local_models_are_free() {
        let (p, _) = scripted("mystery-model", None);
        let m = MeteredLlmProvider::new(p, meter(Budget::new(Some(1.0)), None, true));
        assert!(matches!(
            m.complete(&req(10)).await.unwrap_err(),
            LlmError::Budget(_)
        ));
        let (p, _) = scripted("mystery-model", None);
        let m = MeteredLlmProvider::new(p, meter(Budget::new(None), None, true));
        assert!(m.complete(&req(10)).await.is_ok());
        let (p, _) = scripted("llama3", None);
        let budget = Budget::new(Some(0.0));
        let m = MeteredLlmProvider::new(p, meter(budget.clone(), None, false));
        assert!(m.complete(&req(10)).await.is_ok());
        assert_eq!(budget.spent_usd(), 0.0);
    }

    #[tokio::test]
    async fn data_collecting_models_are_refused_by_default() {
        let (p, calls) = scripted("big-pickle", None);
        let m = MeteredLlmProvider::new(p, meter(Budget::new(None), None, true));
        assert!(matches!(
            m.complete(&req(10)).await.unwrap_err(),
            LlmError::Policy(_)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let (p, _) = scripted("big-pickle", None);
        let mut mt = meter(Budget::new(None), None, true);
        mt.allow_data_collection = true;
        assert!(
            MeteredLlmProvider::new(p, mt)
                .complete(&req(10))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn fallback_moves_on_only_for_transient_failures() {
        let (a, a_calls) = scripted("a", Some(429));
        let (b, b_calls) = scripted("b", None);
        let f = FallbackLlmProvider::new(vec![Arc::new(a), Arc::new(b)]);
        assert_eq!(f.complete(&req(10)).await.unwrap().content, "from b");
        assert_eq!(
            (
                a_calls.load(Ordering::SeqCst),
                b_calls.load(Ordering::SeqCst)
            ),
            (1, 1)
        );

        let (a, _) = scripted("a", Some(400));
        let (b, b_calls) = scripted("b", None);
        let f = FallbackLlmProvider::new(vec![Arc::new(a), Arc::new(b)]);
        assert!(
            f.complete(&req(10)).await.is_err(),
            "a 400 recurs on any model"
        );
        assert_eq!(b_calls.load(Ordering::SeqCst), 0);
    }
}
