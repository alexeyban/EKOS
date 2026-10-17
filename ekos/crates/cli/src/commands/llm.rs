//! `ekos llm routes|usage|probe` (RFC 0197): which model each task uses, what the calls cost,
//! and which models a key can reach.

use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_recovery::usage::{UsageRecord, read_usage};
use ekos_recovery::{LlmRequest, Task};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

fn router(config: &EkosConfig, cwd: &Path) -> ekos_recovery::LlmRouter {
    super::recover::llm_router(config, &config.artifact_dir(cwd))
}

/// `ekos llm routes`: each task's provider, models and wire format.
pub fn routes(config: &EkosConfig, cwd: &Path, json: bool) -> Result<()> {
    let r = router(config, cwd);
    let rows: Vec<_> = Task::ALL
        .iter()
        .map(|t| {
            let i = r.route(*t);
            let routed = config.llm.tasks.contains_key(t.as_str());
            serde_json::json!({
                "task": t.as_str(), "routed": routed, "provider": i.provider,
                "models": i.models, "wire": i.wire.as_str(), "key_env": i.key_env,
                "prices": i.models.iter().map(|m| ekos_recovery::pricing::price(m, &config.llm.prices)
                    .map(|p| serde_json::json!({"input": p.input, "output": p.output}))).collect::<Vec<_>>(),
            })
        })
        .collect();
    let unknown: Vec<&String> = config
        .llm
        .tasks
        .keys()
        .filter(|k| Task::parse(k).is_none())
        .collect();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1, "routes": rows, "unknown_tasks": unknown,
                "budget_usd": config.llm.budget_usd, "prices_as_of": ekos_recovery::pricing::PRICES_AS_OF,
            }))?
        );
        return Ok(());
    }
    println!("{:<16} {:<8} {:<17} models", "task", "provider", "wire");
    for (t, row) in Task::ALL.iter().zip(&rows) {
        let i = r.route(*t);
        let mark = if row["routed"] == true {
            ""
        } else {
            " (from [llm])"
        };
        let models: Vec<String> = i
            .models
            .iter()
            .map(
                |m| match ekos_recovery::pricing::price(m, &config.llm.prices) {
                    Some(p) => format!("{m} (${}/${} per M)", p.input, p.output),
                    None if i.provider == "ollama" => format!("{m} (local)"),
                    None => format!("{m} (price unknown)"),
                },
            )
            .collect();
        println!(
            "{:<16} {:<8} {:<17} {}{mark}",
            t.as_str(),
            i.provider,
            i.wire.as_str(),
            models.join(" → ")
        );
    }
    if let Some(b) = config.llm.budget_usd {
        println!("\nbudget: ${b:.2} per command");
    }
    for u in unknown {
        println!("warning: [llm.tasks.{u}] names no task — it is never used");
    }
    Ok(())
}

#[derive(Default, Serialize)]
struct Totals {
    calls: u64,
    failed: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    cost_usd: f64,
    /// Calls whose price was unknown, so not in `cost_usd`.
    unpriced: u64,
    latency_ms: u64,
}

impl Totals {
    fn add(&mut self, r: &UsageRecord) {
        self.calls += 1;
        self.failed += u64::from(!r.ok);
        self.input_tokens += u64::from(r.input_tokens);
        self.output_tokens += u64::from(r.output_tokens);
        self.cache_read_tokens += u64::from(r.cache_read_tokens);
        self.cache_write_tokens += u64::from(r.cache_write_tokens);
        match r.cost_usd {
            Some(c) => self.cost_usd += c,
            None => self.unpriced += 1,
        }
        self.latency_ms += r.latency_ms;
    }
}

/// `ekos llm usage`: calls that reached a model, by task and model.
pub fn usage(config: &EkosConfig, cwd: &Path, since: Option<&str>, json: bool) -> Result<()> {
    let r = router(config, cwd);
    let path = r.usage_log().expect("a router logs usage").to_path_buf();
    let since = since
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|d| d.with_timezone(&chrono::Utc))
                .or_else(|_| {
                    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                        .map(|d| d.and_hms_opt(0, 0, 0).expect("midnight").and_utc())
                })
                .map_err(|_| anyhow::anyhow!("--since: an RFC 3339 time or YYYY-MM-DD, not {s}"))
        })
        .transpose()?;
    let records: Vec<UsageRecord> = read_usage(&path)?
        .into_iter()
        .filter(
            |u| match (&since, chrono::DateTime::parse_from_rfc3339(&u.at)) {
                (Some(s), Ok(at)) => at.with_timezone(&chrono::Utc) >= *s,
                _ => true,
            },
        )
        .collect();
    let mut by: BTreeMap<(String, String), Totals> = BTreeMap::new();
    let mut all = Totals::default();
    for u in &records {
        by.entry((u.task.clone(), u.model.clone()))
            .or_default()
            .add(u);
        all.add(u);
    }
    if json {
        let rows: Vec<_> = by
            .iter()
            .map(|((t, m), x)| serde_json::json!({"task": t, "model": m, "totals": x}))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1, "log": path, "by_task_and_model": rows, "total": all,
            }))?
        );
        return Ok(());
    }
    if records.is_empty() {
        println!("no LLM calls recorded in {}", path.display());
        return Ok(());
    }
    println!(
        "{:<16} {:<22} {:>6} {:>10} {:>9} {:>9} {:>10} {:>8}",
        "task", "model", "calls", "input", "output", "cached", "cost", "avg ms"
    );
    for ((t, m), x) in &by {
        println!(
            "{t:<16} {m:<22} {:>6} {:>10} {:>9} {:>9} {:>10} {:>8}",
            x.calls,
            x.input_tokens,
            x.output_tokens,
            x.cache_read_tokens,
            format!("${:.4}", x.cost_usd),
            x.latency_ms / x.calls.max(1)
        );
    }
    println!(
        "\n{} calls ({} failed), ${:.4}{}",
        all.calls,
        all.failed,
        all.cost_usd,
        if all.unpriced > 0 {
            format!(" + {} calls at an unknown price", all.unpriced)
        } else {
            String::new()
        }
    );
    Ok(())
}

#[derive(Serialize)]
struct ProbeRow {
    model: String,
    wire: &'static str,
    ok: bool,
    detail: String,
    latency_ms: u64,
    cost_usd: Option<f64>,
}

/// `ekos llm probe`: one tiny request per model (a few tokens each), uncached, so a key's
/// reachable models are known before a route names them. Models default to every route's.
pub async fn probe(
    config: &EkosConfig,
    cwd: &Path,
    task: Option<&str>,
    models: &[String],
    json: bool,
) -> Result<()> {
    let task = match task {
        Some(t) => Task::parse(t).ok_or_else(|| anyhow::anyhow!("no task named {t}"))?,
        None => Task::Default,
    };
    let r = router(config, cwd).uncached();
    let mut list: Vec<String> = models.to_vec();
    if list.is_empty() {
        for t in Task::ALL {
            for m in r.route(t).models {
                if !list.contains(&m) {
                    list.push(m);
                }
            }
        }
    }
    let route = r.route(task);
    let mut rows = vec![];
    for m in &list {
        let wire = if route.provider == "zen" {
            ekos_recovery::route::zen_wire(m)
        } else {
            route.wire
        };
        let started = std::time::Instant::now();
        let spent_before = r.budget().spent_usd();
        let res = match r.provider_for_models(task, Some(std::slice::from_ref(m))) {
            Ok(p) => p
                .complete(&LlmRequest {
                    system: "Reply with the single word OK.",
                    user: "OK?",
                    prompt_version: "llm-probe-v1",
                    max_tokens: 64,
                    history: &[],
                })
                .await
                .map(|resp| {
                    format!(
                        "{:?} ({} in, {} out)",
                        resp.content.trim(),
                        resp.input_tokens,
                        resp.output_tokens
                    )
                }),
            Err(e) => Err(e),
        };
        let (ok, detail) = match res {
            Ok(d) => (true, d),
            Err(e) => (false, short_error(&e.to_string())),
        };
        rows.push(ProbeRow {
            model: m.clone(),
            wire: wire.as_str(),
            ok,
            detail,
            latency_ms: started.elapsed().as_millis() as u64,
            cost_usd: ekos_recovery::pricing::price(m, &config.llm.prices)
                .map(|_| r.budget().spent_usd() - spent_before),
        });
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"schema_version": 1, "provider": route.provider, "models": rows})
            )?
        );
    } else {
        for x in &rows {
            println!(
                "{:<24} {:<17} {:<4} {:>6} ms  {}",
                x.model,
                x.wire,
                if x.ok { "ok" } else { "FAIL" },
                x.latency_ms,
                x.detail
            );
        }
        println!(
            "\n{} of {} reachable, ${:.5} spent",
            rows.iter().filter(|x| x.ok).count(),
            rows.len(),
            r.budget().spent_usd()
        );
    }
    Ok(())
}

/// The host's message, not the whole JSON error body.
fn short_error(e: &str) -> String {
    let msg = e
        .find("\"message\":\"")
        .map(|i| {
            let rest = &e[i + 11..];
            rest[..rest.find('"').unwrap_or(rest.len())].to_string()
        })
        .unwrap_or_else(|| e.to_string());
    msg.chars().take(120).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_error_keeps_the_hosts_message() {
        let e = r#"api error 500: {"error":{"type":"server_error","message":"Upstream request failed: Model access is disabled"}}"#;
        assert_eq!(
            short_error(e),
            "Upstream request failed: Model access is disabled"
        );
        assert_eq!(short_error("plain"), "plain");
    }
}
