//! Looks for row values in a ledger: every current object and relationship, serialised, searched
//! for each line of a values file. It backs the Oracle/iDempiere plan's release gate "row values in
//! the ledger: 0" (RFC 0043, RFC 0211 profiles): sample text values from the source database, then
//! show that none of them reached a compiled object — or exactly where one did.
//!
//! ```text
//! cargo run -p ekos --release --example value_scan -- \
//!   --home ~/.ekos --project idemdocs --values values.txt [--min-len 5] [--all]
//! ```
//!
//! A hit names the object's kind, its name, its `origin` property (`live` for the database
//! catalog and its profiles) and the JSON path of the property holding the value. A hit in an
//! object read from a source file (documentation, code, migration scripts) is that file's own text,
//! not a database row; the report keeps the two apart. Only current versions are read: the ledger
//! is append-only, and an earlier version is out of this scan's reach.

use ekos_compiler_core::EkosConfig;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Where a value was found: the record's kind, name and origin, and the JSON paths holding it.
type Hit = (String, String, String, Vec<String>);

/// Every JSON path under `v` whose string contains `needle`, with that string (cut to 60 chars).
fn paths_with(v: &Value, needle: &str, at: &mut String, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.contains(needle) => out.push(format!(
            "{at} = {:?}",
            s.chars().take(60).collect::<String>()
        )),
        Value::Array(items) => {
            for (i, x) in items.iter().enumerate() {
                let len = at.len();
                at.push_str(&format!("[{i}]"));
                paths_with(x, needle, at, out);
                at.truncate(len);
            }
        }
        Value::Object(map) => {
            for (k, x) in map {
                let len = at.len();
                at.push('.');
                at.push_str(k);
                paths_with(x, needle, at, out);
                at.truncate(len);
            }
        }
        _ => {}
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let home = PathBuf::from(arg("--home").expect("--home <dir>"));
    let project = arg("--project").expect("--project <name>");
    let tenant = arg("--tenant").unwrap_or_else(|| "default".into());
    let all = args.iter().any(|a| a == "--all");
    let min_len: usize = arg("--min-len").map_or(5, |s| s.parse().expect("--min-len <n>"));
    let values: Vec<String> = std::fs::read_to_string(arg("--values").expect("--values <file>"))?
        .lines()
        .map(str::trim)
        .filter(|v| v.chars().count() >= min_len)
        .map(String::from)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let location = ekos::home::location(&home, &tenant, &project)?;
    let (config, root): (EkosConfig, PathBuf) = ekos::home::load_project(&location)?;
    let store = ekos::commands::store::open_store_read_only(&config, &root)?;

    let mut records: Vec<(String, String, String, Value)> = Vec::new();
    for o in store.all_objects()? {
        let v = serde_json::to_value(&o)?;
        let origin = v["properties"]["origin"]
            .as_str()
            .unwrap_or("file")
            .to_string();
        records.push((format!("{:?}", o.kind), o.name.clone(), origin, v));
    }
    let objects = records.len();
    for r in store.all_relationships()? {
        let v = serde_json::to_value(&r)?;
        records.push((
            format!("{:?}", r.kind),
            String::new(),
            "relationship".into(),
            v,
        ));
    }

    let mut hits: BTreeMap<&str, Vec<Hit>> = BTreeMap::new();
    for (kind, name, origin, v) in &records {
        let text = v.to_string();
        for value in &values {
            if text.contains(value.as_str()) {
                let mut paths = Vec::new();
                paths_with(v, value, &mut String::new(), &mut paths);
                // A value in a column's profile: say which column and its type, since the
                // profile keeps bounds of numbers and dates by design (RFC 0211).
                for p in &mut paths {
                    if let Some(i) = p
                        .strip_prefix(".properties.columns[")
                        .and_then(|r| r.split(']').next())
                        .and_then(|i| i.parse::<usize>().ok())
                    {
                        let c = &v["properties"]["columns"][i];
                        p.push_str(&format!(
                            " (column {} {})",
                            c["name"].as_str().unwrap_or("?"),
                            c["data_type"]
                                .as_str()
                                .or(c["type"].as_str())
                                .unwrap_or("?")
                        ));
                    }
                }
                hits.entry(value).or_default().push((
                    kind.clone(),
                    name.clone(),
                    origin.clone(),
                    paths,
                ));
            }
        }
    }

    let live = hits
        .values()
        .flatten()
        .filter(|(_, _, origin, _)| origin == "live")
        .count();
    println!(
        "values: {} (≥ {min_len} chars) · objects: {objects} · relationships: {} · values found: {} · hits in live objects: {live}",
        values.len(),
        records.len() - objects,
        hits.len(),
    );
    for (value, found) in &hits {
        println!("{value:?}: {} record(s)", found.len());
        for (kind, name, origin, paths) in found.iter().take(if all { usize::MAX } else { 5 }) {
            println!(
                "  {kind} {name:?} origin={origin} at {}",
                paths.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
            );
        }
    }
    Ok(())
}
