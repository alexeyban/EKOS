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
    println!("Rule: {}.", f.rule);
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn committed_workspace(dir: &Path) -> EkosConfig {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(dir.join("src/b.rs"), "pub fn b() {}\n").unwrap();
        std::fs::write(
            dir.join("ekos.toml"),
            "[workspace]\nroot = \".\"\n\n[observe]\npaths = [\".\"]\nignore-patterns = [\".ekos\"]\n\n\
             [llm]\napi-key-env = \"EKOS_TEST_KEY_THAT_DOES_NOT_EXIST\"\n",
        )
        .unwrap();
        let config = EkosConfig::from_file(&dir.join("ekos.toml")).unwrap();
        crate::commands::init::run(&config, dir).unwrap();
        crate::commands::build::run(&config, dir).await.unwrap();
        crate::commands::recover::run(&config, dir, false)
            .await
            .unwrap();
        crate::commands::compile::run(&config, dir).await.unwrap();
        crate::commands::commit::run(&config, dir, true)
            .await
            .unwrap();
        config
    }

    #[tokio::test]
    async fn reports_a_fresh_then_a_changed_workspace_as_text_and_json() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let config = committed_workspace(dir).await;
        // Fresh: nothing changed since the commit.
        assert_eq!(freshness::check(&config, dir, 50).status, Status::Fresh);
        run(&config, dir, false, 50).unwrap();

        // A change (different size, so it is a change without hashing), an addition, a removal.
        std::fs::write(dir.join("src/a.rs"), "pub fn a() { let _ = 1; }\n").unwrap();
        std::fs::write(dir.join("src/c.rs"), "pub fn c() {}\n").unwrap();
        std::fs::remove_file(dir.join("src/b.rs")).unwrap();
        let f = freshness::check(&config, dir, 50);
        assert_ne!(f.status, Status::Fresh);
        assert_eq!((f.changed_count, f.added_count, f.removed_count), (1, 1, 1));
        run(&config, dir, false, 50).unwrap();
        run(&config, dir, true, 50).unwrap();
        // A limit smaller than the lists takes the "… more" path.
        run(&config, dir, false, 0).unwrap();
    }

    #[test]
    fn a_section_lists_objects_and_elides_the_rest() {
        let objs: Vec<CitingObject> = (0..7)
            .map(|i| CitingObject {
                id: format!("id{i}"),
                kind: "RustSymbol".into(),
                name: format!("f{i}"),
            })
            .collect();
        let mut stale = BTreeMap::new();
        stale.insert("src/a.rs".to_string(), objs);
        section("Changed", 3, &["src/a.rs".to_string()], &stale);
        section("Added", 0, &[], &stale);
    }
}
