//! RFC 0181 leak suite, through the real binary: a project of two sources compiled by the real
//! pipeline, with marker words in a secret directory and in a second source. Under a scope that
//! hides them, every scoped MCP tool is called with every argument shape that could reach them,
//! and no marker may appear anywhere in any response.
//!
//! The same calls without a scope must show the markers. Otherwise a fixture that never
//! produced them would pass every assertion below.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

/// Words that exist only in what the scopes below hide.
const SECRET_DIR_MARKERS: &[&str] = &[
    "payroll_zebra",
    "vault_quokka",
    "payroll.sql",
    "vault.py",
    "wombat",
    "salaries",
    // Never asked for by name below: these can only come back as content.
    "confidential",
    "salary",
];
/// Words that exist only in the `handbook` source.
const HANDBOOK_MARKERS: &[&str] = &[
    "refunds",
    "narwhal",
    "wombat",
    "salaries",
    "handbook",
    "confidential",
    "thirty days",
];
/// Words that must stay visible, so a scope that hid everything does not pass.
const PUBLIC_MARKERS: &[&str] = &["customers", "charge_card"];

fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ekos"));
    for var in [
        "EKOS_PROJECT",
        "EKOS_TENANT",
        "EKOS_CONFIG",
        "EKOS_WORKSPACE",
        "EKOS_READ_SCOPE",
    ] {
        c.env_remove(var);
    }
    c
}

fn run(home: &Path, args: &[&str]) -> std::process::Output {
    bin()
        .args(args)
        .env("EKOS_HOME", home)
        .current_dir(home)
        .output()
        .expect("running the ekos binary")
}

fn ok(home: &Path, args: &[&str]) -> String {
    let out = run(home, args);
    assert!(
        out.status.success(),
        "ekos {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(path: PathBuf, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Two sources, compiled. Returns the EKOS home.
fn project(root: &Path) -> PathBuf {
    let api = root.join("api");
    write(
        api.join("db/schema.sql"),
        "CREATE TABLE customers (id SERIAL PRIMARY KEY, email TEXT NOT NULL);\n\
         CREATE TABLE orders (id SERIAL PRIMARY KEY, customer_id INT REFERENCES customers(id));\n",
    );
    write(
        api.join("secrets/payroll.sql"),
        "CREATE TABLE payroll_zebra (id SERIAL PRIMARY KEY, \
         customer_id INT REFERENCES customers(id), salary NUMERIC);\n",
    );
    write(
        api.join("secrets/vault.py"),
        "def vault_quokka():\n    return 1\n",
    );
    write(
        api.join("src/lib.rs"),
        "/// Charges a card.\npub fn charge_card(amount: u64) -> u64 { amount }\n",
    );
    write(
        api.join("Cargo.toml"),
        "[package]\nname = \"api\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    let handbook = root.join("handbook");
    write(
        handbook.join("public/refunds.md"),
        // The link gives the fixture an edge from a document that stays visible to one that
        // is hidden; without such an edge, relationship filtering would go untested. A visible
        // document's own text is not rewritten, so the link names no marker word.
        "# Refunds\n\nThe narwhal refunds policy is thirty days. \
         See also [the other page](../hr/pay.md).\n",
    );
    write(
        handbook.join("hr/pay.md"),
        "# Salaries\n\nThe wombat bonus plan is confidential.\n",
    );
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&api)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.com",
        "commit",
        "-qm",
        "init",
    ]);

    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let api_spec = format!("api={}", api.display());
    let handbook_spec = format!("handbook={}", handbook.display());
    ok(
        &home,
        &[
            "project",
            "add",
            "shop",
            "--source",
            &api_spec,
            "--source",
            &handbook_spec,
            "--detect",
        ],
    );
    for stage in [
        vec!["build"],
        vec!["recover"],
        vec!["resolve"],
        vec!["compile"],
        vec!["commit", "--yes"],
    ] {
        let mut args = vec!["--project", "shop"];
        args.extend(stage);
        ok(&home, &args);
    }
    home
}

/// Hides two directories; everything else in both sources stays.
fn deny_directories() -> Value {
    json!({"default": "allow", "rules": [
        {"effect": "deny", "source": "api", "path": "secrets/**"},
        {"effect": "deny", "source": "handbook", "path": "hr/**"},
    ]})
}

/// Shows the `api` source without its secrets; the `handbook` source not at all.
fn api_only() -> Value {
    json!({"default": "deny", "rules": [
        {"effect": "allow", "source": "api"},
        {"effect": "deny", "source": "api", "path": "secrets/**"},
    ]})
}

/// One `ekos mcp serve` over stdio, kept for many calls so the server's result cache is in play.
struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Server {
    fn start(home: &Path, extra: &[&str]) -> Self {
        let mut args: Vec<&str> = extra.to_vec();
        args.extend(["--project", "shop", "mcp", "serve"]);
        let mut child = bin()
            .args(&args)
            .env("EKOS_HOME", home)
            .current_dir(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("starting ekos mcp serve");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next: 1,
        }
    }

    /// The whole response line, and whether the tool reported an error.
    fn call(&mut self, tool: &str, arguments: Value, scope: Option<&Value>) -> (String, bool) {
        let mut params = json!({"name": tool, "arguments": arguments});
        if let Some(scope) = scope {
            params["_meta"] = json!({"ekos/readScope": scope});
        }
        let request =
            json!({"jsonrpc": "2.0", "id": self.next, "method": "tools/call", "params": params});
        self.next += 1;
        writeln!(self.stdin, "{request}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "the server closed its output on {tool}");
        let response: Value = serde_json::from_str(&line).expect("a JSON-RPC response");
        let is_error = response.get("error").is_some()
            || response["result"]["isError"].as_bool().unwrap_or(false);
        (line, is_error)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Ids of the named objects, read without a scope.
fn ids(server: &mut Server, names: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for name in names {
        let query = format!("FIND Object WHERE name = '{name}'");
        let (line, err) = server.call("ekos_ekl", json!({"query": query}), None);
        assert!(!err, "{line}");
        let response: Value = serde_json::from_str(&line).unwrap();
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        let rows = payload["rows"]
            .as_array()
            .or_else(|| payload.as_array())
            .cloned()
            .unwrap();
        assert!(!rows.is_empty(), "the fixture has no object named {name}");
        for row in rows {
            out.push(row["id"].as_str().unwrap().to_string());
        }
    }
    out
}

/// Every call a reader could make with the scoped tools, with the ids of hidden and visible
/// objects filled in.
fn calls(hidden: &[String], visible: &[String]) -> Vec<(&'static str, Value)> {
    let mut out: Vec<(&'static str, Value)> = Vec::new();
    for word in SECRET_DIR_MARKERS
        .iter()
        .chain(HANDBOOK_MARKERS)
        .chain(PUBLIC_MARKERS)
    {
        out.push(("ekos_search", json!({"query": word, "limit": 50})));
        out.push(("ekos_query", json!({"question": format!("what is {word}")})));
        out.push((
            "ekos_retrieve",
            json!({"question": format!("what depends on {word}")}),
        ));
        out.push((
            "ekos_ekl",
            json!({"query": format!("FIND Object WHERE name = '{word}'")}),
        ));
    }
    for query in [
        "FIND Object",
        "FIND Relationship",
        "FIND Object COUNT GROUP BY kind",
        "FIND Object COUNT GROUP BY source",
        "FIND Object WHERE source = 'handbook'",
        "FIND Object WHERE kind = 'File'",
        "FIND Object WHERE kind = 'Table'",
        "FIND Object WHERE kind = 'Document'",
        "FIND Object WHERE kind = 'Section'",
        "FIND Object ORDER BY name",
    ] {
        out.push(("ekos_ekl", json!({"query": query})));
    }
    for id in hidden.iter().chain(visible) {
        out.push(("ekos_state", json!({"id": id})));
        out.push((
            "ekos_state",
            json!({"id": id, "at": "2099-01-01T00:00:00Z"}),
        ));
        out.push(("ekos_neighborhood", json!({"id": id, "depth": 3})));
        out.push(("ekos_dependents", json!({"id": id})));
        out.push((
            "ekos_impact",
            json!({"id": id, "direction": "dependents", "max_hops": 4}),
        ));
        out.push((
            "ekos_impact",
            json!({"id": id, "direction": "dependencies", "max_hops": 4}),
        ));
    }
    out.push(("ekos_diff", json!({"from": "2020-01-01T00:00:00Z"})));
    out.push(("ekos_graph_export", json!({})));
    out.push(("ekos_graph_export", json!({"level": "aggregate"})));
    out.push((
        "ekos_graph_export",
        json!({"level": "aggregate", "group_by": "path_prefix"}),
    ));
    out.push((
        "ekos_graph_export",
        json!({"level": "aggregate", "group_by": "path_prefix", "path_prefix_depth": 3}),
    ));
    out.push(("ekos_graph_export", json!({"include_first_seen": true})));
    out
}

fn leaked<'a>(text: &str, markers: &[&'a str]) -> Vec<&'a str> {
    let lower = text.to_lowercase();
    markers
        .iter()
        .copied()
        .filter(|m| lower.contains(&m.to_lowercase()))
        .collect()
}

/// A tool may echo its request (the question, its keywords, the query plan), so a marker the
/// request itself names proves nothing either way. What is checked for each call is every
/// marker the request did **not** name, and every hidden object id it did not name.
fn leaked_beyond_request(
    response: &str,
    arguments: &Value,
    markers: &[&str],
    hidden_ids: &[String],
) -> Vec<String> {
    let asked = arguments.to_string().to_lowercase();
    let text = response.to_lowercase();
    markers
        .iter()
        .map(|m| m.to_lowercase())
        .chain(hidden_ids.iter().map(|i| i.to_lowercase()))
        .filter(|m| !asked.contains(m) && text.contains(m))
        .collect()
}

/// Both ends of every relationship, read without a scope.
fn relationship_ends(server: &mut Server) -> Vec<(String, String)> {
    let (line, err) = server.call("ekos_ekl", json!({"query": "FIND Relationship"}), None);
    assert!(!err, "{line}");
    let response: Value = serde_json::from_str(&line).unwrap();
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    let rows = payload["rows"]
        .as_array()
        .or_else(|| payload.as_array())
        .cloned()
        .unwrap();
    rows.iter()
        .map(|r| {
            (
                r["from"].as_str().unwrap_or_default().to_string(),
                r["to"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// Ids of every object, as one reader sees them.
fn object_ids(server: &mut Server, scope: Option<&Value>) -> Vec<String> {
    let (line, err) = server.call("ekos_ekl", json!({"query": "FIND Object"}), scope);
    assert!(!err, "{line}");
    let response: Value = serde_json::from_str(&line).unwrap();
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();
    let rows = payload["rows"]
        .as_array()
        .or_else(|| payload.as_array())
        .cloned()
        .unwrap();
    rows.iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn nothing_hidden_by_a_scope_comes_back_from_any_tool() {
    let tmp = tempfile::tempdir().unwrap();
    let home = project(tmp.path());
    let mut server = Server::start(&home, &[]);

    let hidden_by_directory = ids(&mut server, &["payroll_zebra", "vault_quokka", "hr/pay.md"]);
    let everything = object_ids(&mut server, None);
    // Every object is asked about by id, hidden or not: a visible object's neighbourhood is
    // where an edge to a hidden one would show.
    let all_calls = calls(&everything, &[]);
    let relationships = relationship_ends(&mut server);

    // ── control: without a scope each marker comes back from a call that did not name it ──
    let mut returned: std::collections::BTreeSet<String> = Default::default();
    for (tool, arguments) in &all_calls {
        let (line, _) = server.call(tool, arguments.clone(), None);
        for markers in [SECRET_DIR_MARKERS, HANDBOOK_MARKERS] {
            returned.extend(leaked_beyond_request(&line, arguments, markers, &[]));
        }
    }
    for marker in SECRET_DIR_MARKERS.iter().chain(HANDBOOK_MARKERS) {
        assert!(
            returned.contains(&marker.to_lowercase()),
            "without a scope no call returned {marker:?} unasked: the checks below would pass \
             for a fixture that never produces it"
        );
    }

    // ── under each scope: no marker, from any call ───────────────────────────
    // Same server, so every answer above is in its result cache.
    let orders = ids(&mut server, &["orders"]);
    let one_object =
        json!({"default": "allow", "rules": [{"effect": "deny", "object": orders[0]}]});
    let none: &[&str] = &[];
    for (scope, markers, label) in [
        (
            deny_directories(),
            SECRET_DIR_MARKERS,
            "two directories denied",
        ),
        (api_only(), HANDBOOK_MARKERS, "one source allowed"),
        (
            api_only(),
            SECRET_DIR_MARKERS,
            "one source allowed, its secrets denied",
        ),
        // No marker word: `orders` is an ordinary name. The hidden object's id is the signal.
        (one_object, none, "one object denied"),
    ] {
        let shown = object_ids(&mut server, Some(&scope));
        let hidden_ids: Vec<String> = everything
            .iter()
            .filter(|id| !shown.contains(id))
            .cloned()
            .collect();
        assert!(
            !hidden_ids.is_empty() && !shown.is_empty(),
            "[{label}] the scope hides nothing"
        );
        // The fixture must hold an edge that crosses the line a scope draws, or relationship
        // filtering goes untested. Everything the pipeline links here sits in one file, so no
        // path or source rule separates two linked objects; the object rule does (`orders`
        // references `customers`).
        if label == "one object denied" {
            assert!(
                relationships.iter().any(|(a, b)| {
                    everything.contains(a)
                        && everything.contains(b)
                        && shown.contains(a) != shown.contains(b)
                }),
                "[{label}] no relationship joins a visible object to a hidden one"
            );
        }
        let mut public_seen = String::new();
        for (tool, arguments) in &all_calls {
            let (line, _) = server.call(tool, arguments.clone(), Some(&scope));
            let found = leaked_beyond_request(&line, arguments, markers, &hidden_ids);
            assert!(
                found.is_empty(),
                "[{label}] {tool} {arguments} returned {found:?}:\n{line}"
            );
            public_seen.push_str(&line);
        }
        for marker in PUBLIC_MARKERS {
            assert!(
                public_seen.contains(marker),
                "[{label}] {marker:?} is visible under this scope and no call returned it"
            );
        }
        // And asking again without a scope still shows everything: the scoped answers did not
        // replace the unscoped ones in the cache either.
        let (line, _) = server.call("ekos_ekl", json!({"query": "FIND Object"}), None);
        assert_eq!(
            leaked(&line, &["payroll_zebra", "§ Salaries"]).len(),
            2,
            "{line}"
        );
    }

    // A hidden object asked for by id is not found, the same as an id that does not exist.
    let scope = deny_directories();
    let missing = "00000000-0000-4000-8000-000000000000";
    let (unknown, unknown_err) = server.call("ekos_state", json!({"id": missing}), Some(&scope));
    for id in &hidden_by_directory {
        let (line, err) = server.call("ekos_state", json!({"id": id}), Some(&scope));
        assert_eq!(err, unknown_err);
        assert_eq!(
            line.replace(id.as_str(), "ID").len(),
            unknown.replace(missing, "ID").len()
        );
    }
}

#[test]
fn counts_cover_only_what_the_reader_sees() {
    let tmp = tempfile::tempdir().unwrap();
    let home = project(tmp.path());
    let mut server = Server::start(&home, &[]);
    let count = |server: &mut Server, scope: Option<&Value>| -> usize {
        let (line, err) = server.call("ekos_ekl", json!({"query": "FIND Object"}), scope);
        assert!(!err, "{line}");
        let response: Value = serde_json::from_str(&line).unwrap();
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        payload["rows"]
            .as_array()
            .or_else(|| payload.as_array())
            .unwrap()
            .len()
    };
    let all = count(&mut server, None);
    let without_directories = count(&mut server, Some(&deny_directories()));
    let api = count(&mut server, Some(&api_only()));
    assert!(
        api > 0 && api < without_directories && without_directories < all,
        "{api} {without_directories} {all}"
    );

    let (line, _) = server.call(
        "ekos_ekl",
        json!({"query": "FIND Object COUNT GROUP BY source"}),
        Some(&api_only()),
    );
    assert!(leaked(&line, &["handbook"]).is_empty(), "{line}");
}

#[test]
fn every_other_tool_refuses_a_restricted_reader() {
    let tmp = tempfile::tempdir().unwrap();
    let home = project(tmp.path());
    let mut server = Server::start(&home, &[]);
    let scope = deny_directories();
    for tool in [
        "ekos_status",
        "ekos_audit",
        "ekos_conflicts",
        "ekos_semantics_lookup",
        "ekos_semantics_gaps",
        "ekos_session_note",
        "ekos_session_recall",
        "ekos_session_brief",
        "ekos_identity_review",
        "ekos_architecture_review",
        "ekos_architecture_evaluate",
        "ekos_architecture_drift",
        "ekos_architecture_diff",
        "ekos_transformation_explain",
        "ekos_transformation_diff",
        "ekos_clickhouse_query",
        "a_tool_that_does_not_exist",
    ] {
        let (line, err) = server.call(tool, json!({}), Some(&scope));
        assert!(err, "{tool} answered a restricted reader:\n{line}");
        assert!(line.contains("restricted access"), "{tool}: {line}");
        assert!(
            leaked(&line, SECRET_DIR_MARKERS).is_empty(),
            "{tool}: {line}"
        );
    }
    // The same tools answer a reader with no scope (or fail for their own reasons, not this one).
    let (line, err) = server.call("ekos_status", json!({}), None);
    assert!(!err, "{line}");
    // An explicit scope that hides nothing is not a restriction.
    let open = json!({"default": "allow"});
    let (line, err) = server.call("ekos_status", json!({}), Some(&open));
    assert!(!err, "{line}");
}

#[test]
fn a_scope_that_does_not_parse_fails_the_call() {
    let tmp = tempfile::tempdir().unwrap();
    let home = project(tmp.path());
    let mut server = Server::start(&home, &[]);
    for bad in [
        json!({}),
        json!({"rules": []}),
        json!("allow"),
        json!(null),
        json!({"default": "deny", "rules": [{"effect": "allow", "sources": ["api"]}]}),
        json!({"default": "allow", "rules": [{"effect": "deny", "path": "[bad"}]}),
    ] {
        let (line, err) = server.call("ekos_ekl", json!({"query": "FIND Object"}), Some(&bad));
        assert!(err, "a malformed scope {bad} was accepted:\n{line}");
        assert!(leaked(&line, SECRET_DIR_MARKERS).is_empty(), "{line}");
        assert!(leaked(&line, PUBLIC_MARKERS).is_empty(), "{line}");
    }
}

#[test]
fn the_process_scope_and_the_call_scope_both_apply() {
    let tmp = tempfile::tempdir().unwrap();
    let home = project(tmp.path());
    let scope_file = tmp.path().join("scope.json");
    std::fs::write(&scope_file, deny_directories().to_string()).unwrap();
    let file = scope_file.to_str().unwrap();

    // The server was started with the directory scope; a call cannot widen it.
    let mut server = Server::start(&home, &["--read-scope", file]);
    let find_all = json!({"query": "FIND Object"});
    let (line, err) = server.call("ekos_ekl", find_all.clone(), None);
    assert!(!err, "{line}");
    assert!(leaked(&line, SECRET_DIR_MARKERS).is_empty(), "{line}");
    assert!(
        line.contains("customers") && line.contains("refunds"),
        "{line}"
    );

    let widen = json!({"default": "allow"});
    let (line, _) = server.call("ekos_ekl", find_all.clone(), Some(&widen));
    assert!(leaked(&line, SECRET_DIR_MARKERS).is_empty(), "{line}");

    // A call can narrow it further: both scopes apply.
    let (line, _) = server.call("ekos_ekl", find_all, Some(&api_only()));
    assert!(leaked(&line, SECRET_DIR_MARKERS).is_empty(), "{line}");
    assert!(leaked(&line, &["refunds", "narwhal"]).is_empty(), "{line}");
    assert!(line.contains("customers"), "{line}");

    // A restricted process refuses the unscoped tools even for a call that carries no scope.
    let (line, err) = server.call("ekos_status", json!({}), None);
    assert!(err && line.contains("restricted access"), "{line}");
}

#[test]
fn the_cli_flag_filters_reads_and_is_refused_elsewhere() {
    let tmp = tempfile::tempdir().unwrap();
    let home = project(tmp.path());
    let scope_file = tmp.path().join("scope.json");
    std::fs::write(&scope_file, api_only().to_string()).unwrap();
    let file = scope_file.to_str().unwrap();

    let all = ok(
        &home,
        &["--project", "shop", "ekl", "FIND Object", "--json"],
    );
    assert!(all.contains("payroll_zebra") && all.contains("refunds"));

    let scoped = ok(
        &home,
        &[
            "--read-scope",
            file,
            "--project",
            "shop",
            "ekl",
            "FIND Object",
            "--json",
        ],
    );
    assert!(leaked(&scoped, SECRET_DIR_MARKERS).is_empty(), "{scoped}");
    assert!(
        leaked(&scoped, &["refunds", "handbook"]).is_empty(),
        "{scoped}"
    );
    assert!(scoped.contains("customers"));

    let unscoped = ok(
        &home,
        &["--project", "shop", "query", "find", "payroll_zebra"],
    );
    assert!(unscoped.contains("result(s) for"), "{unscoped}");
    let found = ok(
        &home,
        &[
            "--read-scope",
            file,
            "--project",
            "shop",
            "query",
            "find",
            "payroll_zebra",
        ],
    );
    assert!(found.contains("No objects found"), "{found}");

    // Commands that read more than the ledger do not take a scope.
    for command in [
        vec!["status"],
        vec!["doctor"],
        vec!["ask", "what is payroll_zebra"],
        vec!["build"],
    ] {
        let mut args = vec!["--read-scope", file, "--project", "shop"];
        args.extend(command.iter());
        let out = run(&home, &args);
        assert!(!out.status.success(), "{command:?} accepted --read-scope");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("--read-scope applies to"), "{err}");
        assert!(leaked(&String::from_utf8_lossy(&out.stdout), SECRET_DIR_MARKERS).is_empty());
    }

    // A scope file that does not parse stops the command.
    std::fs::write(&scope_file, "{\"rules\": []}").unwrap();
    let out = run(
        &home,
        &[
            "--read-scope",
            file,
            "--project",
            "shop",
            "ekl",
            "FIND Object",
            "--json",
        ],
    );
    assert!(!out.status.success());
    assert!(leaked(&String::from_utf8_lossy(&out.stdout), PUBLIC_MARKERS).is_empty());
}
