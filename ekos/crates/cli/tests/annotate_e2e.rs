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
