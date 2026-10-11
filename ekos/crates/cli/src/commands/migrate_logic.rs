//! `ekos migrate lower` / `ekos migrate check-candidate` — RFC 0164 Phase 1.
//!
//! Both read the ledger and the workspace's own source files. Neither opens a database connection
//! and neither calls a model. Nothing is written: the lowering is printed for review, and what it
//! becomes on the target is still RFC 0160's guarded executor's business.
//!
//! The definition of a view or routine is read back from its file by the span `ekos recover`
//! recorded. That makes this a raw-content read, so it applies the same RFC 0043 exclusion and
//! redaction as recover does — the spans were measured on the redacted text.

use anyhow::{Context, Result, bail};
use ekos_compiler_core::config::EkosConfig;
use ekos_kir::{KirObject, ObjectKind};
use ekos_migrate_logic::lower::{LowerOptions, Lowering, lower_view};
use ekos_migrate_logic::triggers::TriggerOutcome;
use ekos_migrate_logic::{
    check_candidate, propose_trigger, recorded_gate, routine_atoms, routine_gate, sql_atoms,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

fn is_kind(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn text<'a>(o: &'a KirObject, key: &str) -> &'a str {
    o.properties
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
}

/// The object's definition, read from its file by the recorded span.
///
/// `Err` is a sentence for the report: a missing file, an excluded path, or a span that no longer
/// holds a definition of this object (the file changed since `ekos recover`).
fn definition(config: &EkosConfig, cwd: &Path, o: &KirObject) -> Result<String, String> {
    let path = text(o, "source_path");
    if path.is_empty() {
        return Err("no source path was recorded".into());
    }
    let redaction = config.redaction_config();
    if ekos_common::redaction::is_excluded_path(path, &redaction) {
        return Err(format!("{path} is excluded by the security rules"));
    }
    let raw = std::fs::read_to_string(cwd.join(path))
        .map_err(|e| format!("{path} could not be read: {e}"))?;
    let redacted = ekos_common::redaction::redact(&raw, &redaction);
    let span = |key: &str| {
        o.properties
            .get(key)
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
    };
    let (Some(start), Some(end)) = (span("span_start"), span("span_end")) else {
        return Err("no source span was recorded".into());
    };
    let slice = redacted.get(start..end).ok_or_else(|| {
        format!("{path} is shorter than the recorded span — re-run `ekos recover`")
    })?;
    let head = slice.trim_start().to_ascii_lowercase();
    let bare = o.name.rsplit('.').next().unwrap_or(&o.name).to_lowercase();
    if !head.starts_with("create") || !slice.to_lowercase().contains(&bare) {
        return Err(format!(
            "{path} no longer holds this definition at the recorded span — re-run `ekos recover`"
        ));
    }
    Ok(slice.to_string())
}

fn location(o: &KirObject) -> String {
    match o.properties.get("line").and_then(|v| v.as_u64()) {
        Some(line) => format!("{}:{line}", text(o, "source_path")),
        None => text(o, "source_path").to_string(),
    }
}

fn sorted<'a>(objects: &'a [KirObject], kind: &str) -> Vec<&'a KirObject> {
    let mut out: Vec<&KirObject> = objects.iter().filter(|o| is_kind(o, kind)).collect();
    out.sort_by(|a, b| (location(a), &a.name).cmp(&(location(b), &b.name)));
    out
}

/// One row of the report, as JSON. The text rendering is derived from the same values.
fn view_row(config: &EkosConfig, cwd: &Path, o: &KirObject, options: &LowerOptions) -> Value {
    let mut row = json!({ "name": o.name, "source": location(o) });
    match definition(config, cwd, o) {
        Err(why) => {
            row["outcome"] = json!("unreadable");
            row["why"] = json!(why);
        }
        Ok(def) => match lower_view(&def, options) {
            Lowering::Lowered(l) => {
                // The lowering is checked against its own source before it is shown.
                let invented = sql_atoms(&def)
                    .map(|atoms| check_candidate(&atoms, &l.sql))
                    .ok();
                match invented {
                    Some(report) if report.accepted() => {
                        row["outcome"] = json!("lowered");
                        row["sql"] = json!(l.sql);
                        row["notes"] = json!(l.notes);
                    }
                    other => {
                        row["outcome"] = json!("blocked");
                        row["blockers"] = json!([{
                            "code": "LOGIC.INVENTION",
                            "detail": match other {
                                Some(r) => format!(
                                    "the lowering states what the source does not: {}",
                                    r.unmapped.iter().map(|a| a.text.clone()).collect::<Vec<_>>().join(", ")
                                ),
                                None => "the source could not be reduced to atoms".to_string(),
                            },
                        }]);
                    }
                }
            }
            Lowering::Blocked { blockers } => {
                row["outcome"] = json!("blocked");
                row["blockers"] = json!(blockers);
            }
        },
    }
    row
}

fn trigger_row(o: &KirObject) -> Value {
    let class = text(o, "classification");
    let mut row = json!({ "name": o.name, "source": location(o), "classification": class });
    match propose_trigger(class) {
        TriggerOutcome::Proposed(p) => {
            row["outcome"] = json!("proposed");
            row["shape"] = json!(p.shape);
            row["proposal"] = json!(p.proposal);
            row["behaviour_change"] = json!(p.behaviour_change);
        }
        TriggerOutcome::HumanDecision { why } => {
            row["outcome"] = json!("human_decision");
            row["why"] = json!(why);
        }
    }
    row
}

fn routine_row(o: &KirObject) -> Value {
    let language = text(o, "language").to_lowercase();
    let mut row = json!({ "name": o.name, "source": location(o), "language": language });
    if language != "plpgsql" {
        row["outcome"] = json!("out_of_scope");
        row["why"] = json!(format!(
            "a {} routine: only PL/pgSQL bodies are recovered statement by statement",
            if language.is_empty() {
                "non-PL/pgSQL"
            } else {
                &language
            }
        ));
        return row;
    }
    let props: serde_json::Map<String, Value> = o
        .properties
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    match recorded_gate(&props) {
        Ok(()) => row["outcome"] = json!("eligible"),
        Err(why) => {
            row["outcome"] = json!("ineligible");
            row["why"] = json!(why.describe());
        }
    }
    row
}

fn tally(rows: &[Value]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for r in rows {
        *out.entry(r["outcome"].as_str().unwrap_or_default().to_string())
            .or_default() += 1;
    }
    out
}

fn tally_line(rows: &[Value]) -> String {
    tally(rows)
        .iter()
        .map(|(k, n)| format!("{n} {}", k.replace('_', " ")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `ekos migrate lower`.
pub fn lower(
    config: &EkosConfig,
    cwd: &Path,
    database: Option<String>,
    show_sql: bool,
    as_json: bool,
) -> Result<()> {
    super::migrate::require_enabled(config)?;
    let store = super::store::open_store(config, cwd)?;
    let objects = store.all_objects()?;
    let options = LowerOptions { database };

    let views: Vec<Value> = sorted(&objects, "View")
        .into_iter()
        .map(|o| view_row(config, cwd, o, &options))
        .collect();
    let triggers: Vec<Value> = sorted(&objects, "Trigger")
        .into_iter()
        .map(trigger_row)
        .collect();
    let routines: Vec<Value> = sorted(&objects, "Procedure")
        .into_iter()
        .map(routine_row)
        .collect();

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "views": views, "triggers": triggers, "routines": routines,
                "summary": {
                    "views": tally(&views), "triggers": tally(&triggers), "routines": tally(&routines),
                },
            }))?
        );
        return Ok(());
    }

    println!("Views: {} — {}", views.len(), tally_line(&views));
    let mut by_code: BTreeMap<String, usize> = BTreeMap::new();
    for v in &views {
        let name = v["name"].as_str().unwrap_or_default();
        let source = v["source"].as_str().unwrap_or_default();
        match v["outcome"].as_str() {
            Some("lowered") => {
                println!("  lowered   {name}  ({source})");
                for n in v["notes"].as_array().into_iter().flatten() {
                    println!("              · {}", n.as_str().unwrap_or_default());
                }
                if show_sql {
                    println!("              {}", v["sql"].as_str().unwrap_or_default());
                }
            }
            Some("blocked") => {
                println!("  blocked   {name}  ({source})");
                for b in v["blockers"].as_array().into_iter().flatten() {
                    let code = b["code"].as_str().unwrap_or_default();
                    *by_code.entry(code.to_string()).or_default() += 1;
                    println!(
                        "              {code}: {}",
                        b["detail"].as_str().unwrap_or_default()
                    );
                }
            }
            _ => println!(
                "  unreadable {name}  ({source}): {}",
                v["why"].as_str().unwrap_or_default()
            ),
        }
    }
    if !by_code.is_empty() {
        println!("  Blockers by rule:");
        for (code, n) in &by_code {
            println!("    {n:>4}  {code}");
        }
    }

    println!("\nTriggers: {} — {}", triggers.len(), tally_line(&triggers));
    for t in &triggers {
        let name = t["name"].as_str().unwrap_or_default();
        let class = t["classification"].as_str().unwrap_or_default();
        match t["outcome"].as_str() {
            Some("proposed") => {
                println!(
                    "  {name} [{class}] → {}",
                    t["proposal"].as_str().unwrap_or_default()
                );
                println!(
                    "              changes: {}",
                    t["behaviour_change"].as_str().unwrap_or_default()
                );
            }
            _ => println!(
                "  {name} [{class}] → a person decides: {}",
                t["why"].as_str().unwrap_or_default()
            ),
        }
    }

    println!("\nRoutines: {} — {}", routines.len(), tally_line(&routines));
    for r in routines.iter().filter(|r| r["outcome"] == "ineligible") {
        println!(
            "  ineligible {}  ({}): {}",
            r["name"].as_str().unwrap_or_default(),
            r["source"].as_str().unwrap_or_default(),
            r["why"].as_str().unwrap_or_default()
        );
    }
    println!(
        "\nNothing was written or run. A lowered view is a proposal: it is checked to parse on the \
         target and to state nothing its source does not, not yet executed against data (V5)."
    );
    Ok(())
}

/// `ekos migrate check-candidate`.
pub fn check(
    config: &EkosConfig,
    cwd: &Path,
    source: &str,
    candidate: &Path,
    as_json: bool,
) -> Result<()> {
    super::migrate::require_enabled(config)?;
    let store = super::store::open_store(config, cwd)?;
    let objects = store.all_objects()?;
    let wanted = source.to_lowercase();
    let matches: Vec<&KirObject> = objects
        .iter()
        .filter(|o| is_kind(o, "View") || is_kind(o, "Procedure"))
        .filter(|o| {
            let name = o.name.to_lowercase();
            name == wanted
                || name.rsplit('.').next() == Some(wanted.as_str())
                || format!("{}:{name}", text(o, "source_path").to_lowercase()) == wanted
        })
        .collect();
    let object = match matches.as_slice() {
        [] => bail!("no View or Procedure named {source:?} in the ledger"),
        [one] => *one,
        many => bail!(
            "{source:?} names {} objects; say which as <path>:<name>:\n{}",
            many.len(),
            many.iter()
                .map(|o| format!("  {}:{}", text(o, "source_path"), o.name))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    let def = definition(config, cwd, object).map_err(|why| anyhow::anyhow!(why))?;
    let (atoms, unparsed) = if is_kind(object, "View") {
        (
            sql_atoms(&def).map_err(|e| anyhow::anyhow!("the view does not parse: {e}"))?,
            0,
        )
    } else {
        let ir = ekos_plpgsql::parse_function(&def)
            .map_err(|e| anyhow::anyhow!("the routine does not lex: {e:?}"))?;
        if let Err(why) = routine_gate(&ir) {
            bail!(
                "{} is not eligible for reconstruction: {}. A candidate for it cannot be checked, \
                 because the check has nothing to compare the missing part against.",
                object.name,
                why.describe()
            );
        }
        let found = routine_atoms(&ir);
        (found.atoms, found.unparsed)
    };
    let sql = std::fs::read_to_string(candidate)
        .with_context(|| format!("reading {}", candidate.display()))?;
    let report = check_candidate(&atoms, &sql);

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "source": object.name, "source_atoms": atoms.len(),
                "source_fragments_unparsed": unparsed,
                "accepted": report.accepted(), "report": report,
            }))?
        );
    } else {
        println!(
            "Source: {} ({}) — {} atom(s){}",
            object.name,
            location(object),
            atoms.len(),
            if unparsed > 0 {
                format!(
                    ", {unparsed} fragment(s) of the source did not parse and count for nothing"
                )
            } else {
                String::new()
            }
        );
        if let Some(e) = &report.parse_error {
            println!("REJECTED: the candidate does not parse as ClickHouse SQL: {e}");
        } else if report.unmapped.is_empty() {
            println!(
                "ACCEPTED: all {} atom(s) of the candidate are stated by the source.",
                report.checked
            );
        } else {
            println!(
                "REJECTED: {} of {} atom(s) are not stated by the source:",
                report.unmapped.len(),
                report.checked
            );
            for a in &report.unmapped {
                println!("  {:<9} {}", format!("{:?}", a.kind).to_lowercase(), a.text);
            }
        }
    }
    if !report.accepted() {
        bail!("candidate rejected");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(name: &str, path: &str, start: usize, end: usize) -> KirObject {
        KirObject::new(name, ObjectKind::Custom("View".into()))
            .with_property("source_path", json!(path))
            .with_property("span_start", json!(start))
            .with_property("span_end", json!(end))
            .with_property("line", json!(1))
    }

    #[test]
    fn a_view_is_read_back_by_its_span_and_lowered() {
        let dir = tempfile::tempdir().unwrap();
        let sql =
            "-- header\nCREATE VIEW open_items AS SELECT id FROM items WHERE closed = false;\n";
        std::fs::write(dir.path().join("v.sql"), sql).unwrap();
        let start = sql.find("CREATE").unwrap();
        let o = view("open_items", "v.sql", start, sql.len() - 1);
        let row = view_row(
            &EkosConfig::default(),
            dir.path(),
            &o,
            &LowerOptions::default(),
        );
        assert_eq!(row["outcome"], "lowered", "{row}");
        assert_eq!(
            row["sql"],
            "CREATE VIEW `open_items` AS SELECT `id` FROM `items` WHERE (`closed` = false)"
        );
    }

    #[test]
    fn a_span_that_no_longer_holds_the_view_is_reported_not_lowered() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("v.sql"),
            "CREATE VIEW other AS SELECT 1;\n-- padding padding",
        )
        .unwrap();
        let o = view("open_items", "v.sql", 0, 30);
        let row = view_row(
            &EkosConfig::default(),
            dir.path(),
            &o,
            &LowerOptions::default(),
        );
        assert_eq!(row["outcome"], "unreadable");
        assert!(row["why"].as_str().unwrap().contains("re-run"), "{row}");
        // A missing file and a path the security rules exclude are reported the same way.
        let gone = view("v", "missing.sql", 0, 5);
        assert_eq!(
            view_row(
                &EkosConfig::default(),
                dir.path(),
                &gone,
                &LowerOptions::default()
            )["outcome"],
            "unreadable"
        );
        let secret = view("v", ".env", 0, 5);
        let row = view_row(
            &EkosConfig::default(),
            dir.path(),
            &secret,
            &LowerOptions::default(),
        );
        assert!(row["why"].as_str().unwrap().contains("excluded"), "{row}");
    }

    #[test]
    fn triggers_and_routines_report_their_outcome() {
        let t = KirObject::new("audit_trg", ObjectKind::Custom("Trigger".into()))
            .with_property("classification", json!("Audit"));
        assert_eq!(trigger_row(&t)["outcome"], "proposed");
        let m = KirObject::new("mixed_trg", ObjectKind::Custom("Trigger".into()))
            .with_property("classification", json!("Mixed"));
        assert_eq!(trigger_row(&m)["outcome"], "human_decision");

        let routine = |language: &str, fidelity: &str, dynamic: u64| {
            KirObject::new("f", ObjectKind::Custom("Procedure".into()))
                .with_property("language", json!(language))
                .with_property("fidelity", json!(fidelity))
                .with_property("dynamic_sql_sites", json!(dynamic))
        };
        assert_eq!(
            routine_row(&routine("plpgsql", "statements", 0))["outcome"],
            "eligible"
        );
        assert_eq!(
            routine_row(&routine("plpgsql", "partial", 0))["outcome"],
            "ineligible"
        );
        assert_eq!(
            routine_row(&routine("plpgsql", "statements", 1))["outcome"],
            "ineligible"
        );
        assert_eq!(
            routine_row(&routine("sql", "signature", 0))["outcome"],
            "out_of_scope"
        );
    }
}
