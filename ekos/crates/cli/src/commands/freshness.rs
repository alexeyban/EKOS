//! `ekos freshness` (RFC 0171): which source files changed since the ledger was committed, and
//! which compiled objects cite them.

use crate::freshness::{self, CitingObject, Freshness, Status};
use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Objects listed per file in the text report; `--json` carries them all.
const OBJECTS_PER_FILE: usize = 5;

#[derive(Debug, Serialize)]
struct FreshnessJson {
    schema_version: u32,
    #[serde(flatten)]
    freshness: Freshness,
    /// Changed or removed file → the ledger objects whose evidence cites it.
    may_be_stale: BTreeMap<String, Vec<CitingObject>>,
}

pub fn run(config: &EkosConfig, cwd: &Path, json: bool, limit: usize) -> Result<()> {
    let f = freshness::check(config, cwd, limit);
    let mut paths: Vec<String> = f.changed.iter().chain(&f.removed).cloned().collect();
    paths.sort();
    let may_be_stale = if paths.is_empty() {
        BTreeMap::new()
    } else {
        match super::store::open_store_read_only(config, cwd) {
            Ok(store) => freshness::citing_objects(&*store, &paths)?,
            Err(e) => {
                tracing::debug!("no ledger to map files to objects: {e}");
                BTreeMap::new()
            }
        }
    };

    if json {
        let out = FreshnessJson {
            schema_version: 1,
            freshness: f,
            may_be_stale,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("Source freshness: {}", f.summary_line());
    if f.status == Status::Unknown {
        return Ok(());
    }
    if let (Some(then), Some(now)) = (&f.git_head_then, &f.git_head_now)
        && then != now
    {
        println!(
            "  git: {} at commit, {} now",
            &then[..then.len().min(10)],
            &now[..now.len().min(10)]
        );
    }
    if f.status == Status::Fresh {
        return Ok(());
    }
    section("Changed", f.changed_count, &f.changed, &may_be_stale);
    section("Added", f.added_count, &f.added, &may_be_stale);
    section("Removed", f.removed_count, &f.removed, &may_be_stale);
    let stale: usize = may_be_stale.values().map(Vec::len).sum();
    println!();
    println!(
        "{stale} compiled object(s) cite a changed or removed file. Rebuild with `ekos build` … `ekos commit`."
    );
    println!("Rule: {}.", freshness::RULE);
    Ok(())
}

fn section(
    title: &str,
    count: usize,
    shown: &[String],
    may_be_stale: &BTreeMap<String, Vec<CitingObject>>,
) {
    if count == 0 {
        return;
    }
    println!();
    println!("{title} ({count}):");
    for p in shown {
        println!("  {p}");
        if let Some(objs) = may_be_stale.get(p) {
            let names: Vec<String> = objs
                .iter()
                .take(OBJECTS_PER_FILE)
                .map(|o| format!("{} {}", o.kind, o.name))
                .collect();
            let more = objs.len().saturating_sub(OBJECTS_PER_FILE);
            let tail = if more > 0 {
                format!(" (+{more} more)")
            } else {
                String::new()
            };
            println!("    may be stale: {}{tail}", names.join(", "));
        }
    }
    if count > shown.len() {
        println!("  … {} more (raise --limit)", count - shown.len());
    }
}
