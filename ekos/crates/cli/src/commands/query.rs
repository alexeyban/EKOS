use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use ekos_kir::KirId;
use ekos_ledger::Ledger;
use ekos_runtime::Runtime;
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

pub fn find(config: &EkosConfig, cwd: &Path, query: &str) -> Result<()> {
    let ledger = open_ledger(config, cwd)?;
    let rt = Runtime::new(&ledger);
    let results = rt.find_objects(query)?;

    if results.is_empty() {
        println!("No objects found matching '{query}'.");
    } else {
        println!("{} result(s) for '{query}':", results.len());
        for (id, name) in &results {
            println!("  {id}  {name}");
        }
    }

    Ok(())
}

pub fn neighbourhood(config: &EkosConfig, cwd: &Path, id_str: &str, depth: u32) -> Result<()> {
    let id = KirId::from_str(id_str).map_err(|_| anyhow::anyhow!("invalid object id: {id_str}"))?;

    let ledger = open_ledger(config, cwd)?;
    let rt = Runtime::new(&ledger);
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

fn open_ledger(config: &EkosConfig, cwd: &Path) -> Result<Ledger> {
    let path = config.ledger_path(cwd);
    Ledger::open(&path).map_err(|e| {
        anyhow::anyhow!(
            "cannot open ledger at {}: {e}\nRun `ekos build` first.",
            path.display()
        )
    })
}
