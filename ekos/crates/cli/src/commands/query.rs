use super::store::open_store;
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_kir::KirId;
use ekos_ledger::KnowledgeStore;
use ekos_runtime::reason::{plan_question, render_plan};
use ekos_runtime::{RetrievalRequest, Runtime};
use std::{path::Path, str::FromStr};

pub fn object(config: &EkosConfig, cwd: &Path, id_str: &str, format: &str) -> Result<()> {
    let id = KirId::from_str(id_str).map_err(|_| anyhow::anyhow!("invalid object id: {id_str}"))?;

    let ledger = open_ledger(config, cwd)?;
    let obj = ledger.get_object(&id)?;

    match obj {
        None => {
            eprintln!("Not found: {id_str}");
            std::process::exit(1);
        }
        Some(obj) => {
            // Attach evidence fragments
            let mut evidence = Vec::new();
            for ev_id in &obj.evidence {
                if let Some(ev) = ledger.get_evidence(ev_id)? {
                    evidence.push(ev);
                }
            }

            if format == "json" {
                let out = serde_json::json!({ "object": obj, "evidence": evidence });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Object: {} ({})", obj.name, obj.kind);
                println!("  ID:   {}", obj.id);
                if !obj.properties.is_empty() {
                    println!("  Properties:");
                    for (k, v) in &obj.properties {
                        println!("    {k}: {v}");
                    }
                }
                if !evidence.is_empty() {
                    println!("  Evidence:");
                    for ev in &evidence {
                        println!(
                            "    [{:.0}%] {} — \"{}\"",
                            ev.confidence * 100.0,
                            ev.location.path,
                            ev.fragment
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

pub fn find(config: &EkosConfig, cwd: &Path, query: &str, explain: bool, mode: &str) -> Result<()> {
    let ledger = open_ledger(config, cwd)?;
    let rt = Runtime::over(&*ledger);

    if explain {
        // RFC 0124: show how the REASON planner classifies and routes this text.
        let plan = plan_question(query, &rt)?;
        println!("{}\n", render_plan(&plan));
    }

    // RFC 0119: route through the retrieval seam. RFC 0125: `--mode vector|hybrid` attaches a
    // pre-computed query embedding; `vector` also drops the BM25 arm.
    let mut req = RetrievalRequest::lexical(query);
    match mode {
        "lexical" => {}
        "vector" | "hybrid" => {
            req.query_embedding = Some(super::commit::embed_query_blocking(config, cwd, query)?);
            if mode == "vector" {
                req.arms.bm25 = false;
            }
        }
        other => anyhow::bail!("unknown --mode {other:?} (want lexical/vector/hybrid)"),
    }
    let results = rt.retrieve(&req)?;

    if explain && !results.arm_timings.is_empty() {
        // RFC 0126: per-arm wall-clock, so a slow hybrid search shows which half is slow.
        let arms: Vec<String> = results
            .arm_timings
            .iter()
            .map(|t| format!("{:?} {:.1}ms ({})", t.source, t.elapsed_ms, t.candidates))
            .collect();
        println!("arms: {}\n", arms.join(" · "));
    }

    if results.hits.is_empty() {
        println!("No objects found matching '{query}'.");
    } else {
        let arms = if results.arms_run.vector {
            if results.arms_run.bm25 {
                " (bm25 + vector)"
            } else {
                " (vector)"
            }
        } else {
            ""
        };
        println!("{} result(s) for '{query}'{arms}:", results.hits.len());
        for hit in &results.hits {
            println!("  {}  {}", hit.id, hit.name);
        }
        if matches!(mode, "vector" | "hybrid") && !results.arms_run.vector {
            eprintln!(
                "note: no vector index on disk (or dim mismatch) — results are lexical only; run \
                 `ekos commit` with [embeddings] enabled"
            );
        }
    }

    Ok(())
}

pub fn neighbourhood(config: &EkosConfig, cwd: &Path, id_str: &str, depth: u32) -> Result<()> {
    let id = KirId::from_str(id_str).map_err(|_| anyhow::anyhow!("invalid object id: {id_str}"))?;

    let ledger = open_ledger(config, cwd)?;
    let rt = Runtime::over(&*ledger);
    let graph = rt.load_neighborhood(&id, depth)?;

    if graph.objects.is_empty() {
        eprintln!("Not found: {id_str}");
        std::process::exit(1);
    }

    println!(
        "Neighbourhood of {} (depth {}): {} objects, {} relationships",
        id_str,
        depth,
        graph.objects.len(),
        graph.relationships.len()
    );
    println!();

    for obj in &graph.objects {
        let marker = if obj.id == id { " [root]" } else { "" };
        println!("  {}  {} ({}){}", obj.id, obj.name, obj.kind, marker);
    }

    if !graph.relationships.is_empty() {
        println!();
        for rel in &graph.relationships {
            println!("  {:?}  {} → {}", rel.kind, rel.from, rel.to);
        }
    }

    Ok(())
}

fn open_ledger(config: &EkosConfig, cwd: &Path) -> Result<Box<dyn KnowledgeStore>> {
    open_store(config, cwd).map_err(|e| anyhow::anyhow!("{e}\nRun `ekos build` first."))
}
