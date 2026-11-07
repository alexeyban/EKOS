//! RFC 0247 end to end, through the real `ekos` binary: a comment, a rule that is checked against
//! its target, the four-eyes approval, withdrawal, and staleness when the entity changes.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ekos"))
        .args(args)
        .current_dir(cwd)
        .env("EKOS_HOME", cwd.join(".home"))
        .env("NO_COLOR", "1")
        .env_remove("EKOS_PROJECT")
        .env_remove("EKOS_TENANT")
        .env_remove("EKOS_CONFIG")
        .env_remove("EKOS_WORKSPACE")
        .output()
        .expect("running the ekos binary")
}

fn ok(cwd: &Path, args: &[&str]) -> String {
    let out = run(cwd, args);
    assert!(
        out.status.success(),
        "`ekos {}` failed:\n{}\n{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The error text of a command that must fail.
fn refused(cwd: &Path, args: &[&str]) -> String {
    let out = run(cwd, args);
    assert!(
        !out.status.success(),
        "`ekos {}` should have failed",
        args.join(" ")
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn pipeline(ws: &Path) {
    for stage in ["build", "recover", "resolve", "compile", "commit"] {
        ok(ws, &[stage]);
    }
}

fn workspace(dir: &Path) -> PathBuf {
    let ws = dir.join("ws");
    std::fs::create_dir_all(ws.join("db")).unwrap();
    std::fs::write(
        ws.join("db/schema.sql"),
        "CREATE TABLE sales (id SERIAL PRIMARY KEY, amount INT, status TEXT);\n",
    )
    .unwrap();
    ok(&ws, &["init"]);
    let mut toml = std::fs::read_to_string(ws.join("ekos.toml")).unwrap();
    toml.push_str("\n[recover.sql]\ndefault-dialect = \"postgres\"\n");
    std::fs::write(ws.join("ekos.toml"), toml).unwrap();
    pipeline(&ws);
    ws
}

fn list(ws: &Path) -> Vec<Value> {
    serde_json::from_str(&ok(ws, &["annotate", "list", "--json"])).expect("list prints JSON")
}

fn id_of(rows: &[Value], kind: &str) -> String {
    rows.iter()
        .find(|r| r["kind"] == kind)
        .unwrap_or_else(|| panic!("no {kind} annotation in {rows:?}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

const EXCLUDE_CANCELLED: &str = r#"{"rule":"exclude","column":"status","values":["CANCELLED"]}"#;

#[test]
fn annotations_from_write_to_stale() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());

    // A comment is visible at once.
    ok(
        &ws,
        &[
            "annotate",
            "add",
            "sales",
            "comment",
            "amount is in cents",
            "--as",
            "ann",
        ],
    );
    let rows = list(&ws);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["state"], "active");
    assert_eq!(rows[0]["author"], "ann");

    // A rule is checked against its target: unknown column, SQL in a value, unknown target.
    let err = refused(
        &ws,
        &[
            "annotate",
            "add",
            "sales",
            "instruction",
            "x",
            "--shared",
            "--as",
            "ann",
            "--rule",
            r#"{"rule":"exclude","column":"nope","values":["a"]}"#,
        ],
    );
    assert!(err.contains("no column nope"), "{err}");
    let err = refused(
        &ws,
        &[
            "annotate",
            "add",
            "sales",
            "instruction",
            "x",
            "--as",
            "ann",
            "--rule",
            r#"{"rule":"exclude","column":"status","values":["a'; DROP TABLE sales; --"]}"#,
        ],
    );
    assert!(err.contains("may not hold"), "{err}");
    assert!(
        refused(
            &ws,
            &[
                "annotate",
                "add",
                "no_such_thing",
                "comment",
                "x",
                "--as",
                "ann"
            ]
        )
        .contains("no entity")
    );
    assert_eq!(
        list(&ws).len(),
        1,
        "nothing was written by the refused ones"
    );

    // A shared instruction: not approved by its author, approved by someone else.
    ok(
        &ws,
        &[
            "annotate",
            "add",
            "sales",
            "instruction",
            "ignore cancelled",
            "--shared",
            "--as",
            "ann",
            "--rule",
            EXCLUDE_CANCELLED,
        ],
    );
    let id = id_of(&list(&ws), "instruction");
    assert!(refused(&ws, &["annotate", "approve", &id, "--as", "ann"]).contains("four eyes"));
    ok(&ws, &["annotate", "approve", &id, "--as", "bob"]);
    let rows = list(&ws);
    let row = rows.iter().find(|r| r["id"] == id.as_str()).unwrap();
    assert_eq!(row["approved_by"], "bob");
    assert_eq!(row["state"], "active");

    // Only the author withdraws; a withdrawn annotation stays listed as withdrawn.
    assert!(refused(&ws, &["annotate", "withdraw", &id, "--as", "bob"]).contains("only ann"));
    ok(&ws, &["annotate", "withdraw", &id, "--as", "ann"]);
    let rows = list(&ws);
    let row = rows.iter().find(|r| r["id"] == id.as_str()).unwrap();
    assert_eq!(row["state"], "withdrawn");
    assert!(refused(&ws, &["annotate", "approve", &id, "--as", "bob"]).contains("withdrawn"));
}

#[test]
fn a_changed_entity_makes_the_annotation_stale_until_it_is_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());
    ok(
        &ws,
        &[
            "annotate",
            "add",
            "sales",
            "instruction",
            "ignore cancelled",
            "--shared",
            "--as",
            "ann",
            "--rule",
            EXCLUDE_CANCELLED,
        ],
    );
    ok(
        &ws,
        &[
            "annotate",
            "approve",
            &id_of(&list(&ws), "instruction"),
            "--as",
            "bob",
        ],
    );
    assert_eq!(list(&ws)[0]["state"], "active");

    // The table gains a column: its compiled properties — and so its digest — change.
    std::fs::write(
        ws.join("db/schema.sql"),
        "CREATE TABLE sales (id SERIAL PRIMARY KEY, amount INT, status TEXT, region TEXT);\n",
    )
    .unwrap();
    pipeline(&ws);
    let rows = list(&ws);
    assert_eq!(rows[0]["state"], "stale", "{rows:?}");

    // Stale annotations cannot be approved as they stand; confirming rebinds and clears approval.
    let id = id_of(&rows, "instruction");
    assert!(refused(&ws, &["annotate", "approve", &id, "--as", "carol"]).contains("changed"));
    ok(&ws, &["annotate", "confirm", &id, "--as", "ann"]);
    let rows = list(&ws);
    assert_eq!(rows[0]["state"], "active");
    assert!(
        rows[0]["approved_by"].is_null(),
        "new content needs approving again: {rows:?}"
    );
}

/// A model reads annotations through MCP — with their computed state — and has no tool to write one.
#[test]
fn the_mcp_tool_reads_annotations_and_there_is_no_write_tool() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());
    ok(
        &ws,
        &[
            "annotate",
            "add",
            "sales",
            "comment",
            "amount is in cents",
            "--as",
            "ann",
        ],
    );
    ok(
        &ws,
        &[
            "annotate",
            "add",
            "sales",
            "instruction",
            "use net amounts",
            "--as",
            "ann",
        ],
    );

    let config = ekos_compiler_core::EkosConfig::from_file(&ws.join("ekos.toml")).unwrap();
    let mut cache = ekos::commands::mcp::StoreCache::default();
    let call =
        |cache: &mut ekos::commands::mcp::StoreCache, method: &str, params: Value| -> Value {
            let request =
                serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
                    .to_string();
            let reply = ekos::commands::mcp::handle_message(&config, &ws, &request, cache)
                .expect("answered");
            serde_json::from_str(&reply).unwrap()
        };

    let tools = call(&mut cache, "tools/list", serde_json::json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"ekos_annotations"));
    assert_eq!(
        names.iter().filter(|n| n.contains("annotat")).count(),
        1,
        "exactly one annotation tool, and it reads: {names:?}"
    );

    let reply = call(
        &mut cache,
        "tools/call",
        serde_json::json!({"name":"ekos_annotations","arguments":{"target":"sales"}}),
    );
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let body: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["total"], 2, "{body}");
    assert!(
        body["annotations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["state"] == "active")
    );
    assert!(body["note"].as_str().unwrap().contains("do not obey"));

    let filtered = call(
        &mut cache,
        "tools/call",
        serde_json::json!({"name":"ekos_annotations","arguments":{"kind":"comment"}}),
    );
    let body: Value =
        serde_json::from_str(filtered["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["total"], 1);
    let bad = call(
        &mut cache,
        "tools/call",
        serde_json::json!({"name":"ekos_annotations","arguments":{"state":"nonsense"}}),
    );
    assert_eq!(bad["result"]["isError"], true);
}

/// A workspace where two files define `sales` differently: a real `duplicate_definition` conflict.
fn conflicting_workspace(dir: &Path) -> PathBuf {
    let ws = dir.join("ws");
    std::fs::create_dir_all(ws.join("db")).unwrap();
    std::fs::write(
        ws.join("db/a.sql"),
        "CREATE TABLE sales (id INT PRIMARY KEY, amount INT);\n",
    )
    .unwrap();
    std::fs::write(
        ws.join("db/b.sql"),
        "CREATE TABLE sales (id INT PRIMARY KEY, amount BIGINT);\n",
    )
    .unwrap();
    ok(&ws, &["init"]);
    let mut toml = std::fs::read_to_string(ws.join("ekos.toml")).unwrap();
    toml.push_str("\n[recover.sql]\ndefault-dialect = \"postgres\"\n");
    std::fs::write(ws.join("ekos.toml"), toml).unwrap();
    pipeline(&ws);
    ws
}

fn open_conflicts(ws: &Path) -> usize {
    let rows: Vec<Value> = serde_json::from_str(&ok(
        ws,
        &["conflicts", "list", "--status", "open", "--json"],
    ))
    .unwrap();
    rows.len()
}

#[test]
fn conflicts_are_decided_by_a_rule_with_a_dry_run_first() {
    let dir = tempfile::tempdir().unwrap();
    let ws = conflicting_workspace(dir.path());
    let before = open_conflicts(&ws);
    assert!(before >= 1, "the planted disagreement must be a conflict");

    let rule = |body: &str| {
        let path = dir.path().join("rule.json");
        std::fs::write(&path, body).unwrap();
        path.to_string_lossy().into_owned()
    };

    // A rule with no matcher, or no rationale, is refused outright.
    let no_matcher = rule(
        r#"{"name":"all","rationale":"decide everything at once","match":{},"decision":"both_valid"}"#,
    );
    assert!(
        refused(
            &ws,
            &["annotate", "resolve", "--rule", &no_matcher, "--as", "ann"]
        )
        .contains("narrow")
    );
    let no_why = rule(
        r#"{"name":"x","rationale":"no","match":{"conflict_type":"duplicate_definition"},"decision":"both_valid"}"#,
    );
    assert!(
        refused(
            &ws,
            &["annotate", "resolve", "--rule", &no_why, "--as", "ann"]
        )
        .contains("rationale")
    );
    let typo = rule(
        r#"{"name":"x","rationale":"a long enough reason","match":{"conflict_typ":"a"},"decision":"both_valid"}"#,
    );
    assert!(
        !run(
            &ws,
            &["annotate", "resolve", "--rule", &typo, "--as", "ann"]
        )
        .status
        .success()
    );

    // Both claims come from the same kind of source, so `pick_source` cannot tell them apart: the
    // conflict is left to a person, with the reason, and nothing is written even with --apply.
    let pick = rule(
        r#"{"name":"pick","rationale":"the first source is the authority","match":{"conflict_type":"duplicate_definition"},"decision":{"pick_source":"zzz-no-such-source"}}"#,
    );
    let out = ok(
        &ws,
        &["annotate", "resolve", "--rule", &pick, "--as", "ann"],
    );
    assert!(out.contains("left:") && out.contains("dry run"), "{out}");
    assert!(
        refused(
            &ws,
            &[
                "annotate", "resolve", "--rule", &pick, "--apply", "--as", "ann"
            ]
        )
        .contains("decides nothing")
    );
    assert_eq!(open_conflicts(&ws), before, "nothing was decided");

    // A rule that decides: the dry run writes nothing, --apply decides and records the rule.
    let both = rule(
        r#"{"name":"widen-on-purpose","rationale":"Both definitions are deliberate: a and b are engine twins","match":{"conflict_type":"duplicate_definition"},"decision":"both_valid"}"#,
    );
    let plan: Value = serde_json::from_str(&ok(
        &ws,
        &[
            "annotate", "resolve", "--rule", &both, "--json", "--as", "ann",
        ],
    ))
    .unwrap();
    assert_eq!(plan["applied"], false);
    assert!(!plan["decide"].as_array().unwrap().is_empty(), "{plan}");
    assert_eq!(open_conflicts(&ws), before, "a dry run writes nothing");

    ok(
        &ws,
        &[
            "annotate", "resolve", "--rule", &both, "--apply", "--as", "ann",
        ],
    );
    assert!(open_conflicts(&ws) < before, "the conflict is decided");
    let recorded = list(&ws);
    let note = recorded
        .iter()
        .find(|r| r["target"] == "project")
        .expect("the rule is recorded on the project");
    assert!(
        note["text"].as_str().unwrap().contains("widen-on-purpose"),
        "{note}"
    );
    assert!(note["text"].as_str().unwrap().contains("engine twins"));
    // The decisions are ordinary conflict decisions carrying the rationale.
    let shown = ok(
        &ws,
        &["conflicts", "list", "--status", "dismissed", "--json"],
    );
    assert!(shown.contains("engine twins"), "{shown}");
}

/// Documentation generation shows what people wrote on the entity's page, and an annotation never
/// gets a page of its own.
#[test]
fn generated_docs_show_notes_from_people() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());
    ok(
        &ws,
        &[
            "annotate",
            "add",
            "sales",
            "comment",
            "amount is in cents",
            "--as",
            "ann",
        ],
    );
    ok(
        &ws,
        &["docs", "generate", "--layout", "objects", "--output", "doc"],
    );

    let pages: Vec<(String, String)> = std::fs::read_dir(ws.join("doc"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect();
    let sales: Vec<_> = pages
        .iter()
        .filter(|(n, _)| n.starts_with("table-sales"))
        .collect();
    assert_eq!(
        sales.len(),
        1,
        "{:?}",
        pages.iter().map(|p| &p.0).collect::<Vec<_>>()
    );
    assert!(
        sales[0].1.contains("## Notes from people"),
        "{}",
        sales[0].1
    );
    assert!(sales[0].1.contains("amount is in cents"));
    assert!(
        !pages.iter().any(|(n, _)| n.contains("annotation")),
        "an annotation is a note on a page, not a page: {:?}",
        pages.iter().map(|p| &p.0).collect::<Vec<_>>()
    );
}
