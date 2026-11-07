//! RFC 0245 end to end, through the real `ekos` binary, on `tests/fixtures/ossie-tpcds` (five
//! TPC-DS tables + Apache Ossie's official TPC-DS example model):
//!
//! 1. compile the schema alone, **export**, and validate;
//! 2. add the Ossie model to the sources (**import** by ingest), re-compile, **export again**, and
//!    validate — the official model's synonyms and its dataset metric now ride along;
//! 3. review in the exported file, **`ekos import ossie`**, export the confirmed model, validate;
//! 4. plant two disagreements in the Ossie file and check both are reported, and nothing else.
//!
//! Every export is checked against the vendored official JSON Schema and the official validator's
//! structural checks (`ekos_ossie::check`). `scripts/ossie-validate.sh` runs the same flow through
//! Ossie's own `validation/validate.py`.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures/ossie-tpcds")
}

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

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// A workspace with the fixture's schema; the Ossie model is held back until step 2.
fn workspace(dir: &Path) -> PathBuf {
    let ws = dir.join("ws");
    copy_dir(&fixture().join("db"), &ws.join("db"));
    ok(&ws, &["init"]);
    let mut toml = std::fs::read_to_string(ws.join("ekos.toml")).unwrap();
    toml.push_str("\n[semantics]\nenabled = true\n");
    if toml.contains("[recover.sql]") {
        toml = toml.replace(
            "[recover.sql]",
            "[recover.sql]\ndefault-dialect = \"postgres\"",
        );
    } else {
        toml.push_str("\n[recover.sql]\ndefault-dialect = \"postgres\"\n");
    }
    std::fs::write(ws.join("ekos.toml"), toml).unwrap();
    ws
}

fn pipeline(ws: &Path) -> String {
    let mut log = String::new();
    for stage in ["build", "recover", "resolve", "compile", "commit"] {
        log.push_str(&ok(ws, &[stage]));
    }
    log
}

/// `ekos export ossie --json` with `extra` flags, validated, as JSON.
fn export(ws: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["export", "ossie", "--json"];
    args.extend_from_slice(extra);
    let model: Value = serde_json::from_str(&ok(ws, &args)).expect("export prints JSON");
    validate(&model);
    model
}

/// The vendored official JSON Schema, then the official validator's structural checks.
fn validate(model: &Value) {
    let schema: Value = serde_json::from_str(ekos_ossie::SCHEMA_JSON).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<String> = validator
        .iter_errors(model)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "official JSON Schema: {errors:#?}");
    let parsed: ekos_ossie::SemanticModel = serde_json::from_value(model.clone()).unwrap();
    let problems = ekos_ossie::check(&parsed);
    assert!(problems.is_empty(), "structural checks: {problems:#?}");
}

fn dataset<'a>(m: &'a Value, name: &str) -> &'a Value {
    m["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == name)
        .unwrap_or_else(|| panic!("no dataset {name}"))
}

fn names(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|x| x["name"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The parsed `EKOS` extension of an element.
fn ekos(element: &Value) -> Value {
    let data = element["custom_extensions"]
        .as_array()
        .and_then(|a| a.iter().find(|e| e["vendor_name"] == "EKOS"))
        .and_then(|e| e["data"].as_str())
        .expect("an EKOS extension");
    serde_json::from_str(data).unwrap()
}

fn set_ekos(element: &mut Value, data: &Value) {
    for e in element["custom_extensions"].as_array_mut().unwrap() {
        if e["vendor_name"] == "EKOS" {
            e["data"] = Value::String(data.to_string());
        }
    }
}

fn field<'a>(d: &'a mut Value, name: &str) -> &'a mut Value {
    d["fields"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["name"] == name)
        .unwrap()
}

#[test]
fn export_ingest_reexport_and_review_all_validate() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());

    // ── 1. The schema alone → export → valid ─────────────────────────────────────────────
    pipeline(&ws);
    let first = export(&ws, &["--status", "all"]);
    assert_eq!(first["version"], ekos_ossie::SPEC_VERSION);
    let mut tables = names(&first["datasets"]);
    tables.sort();
    assert_eq!(
        tables,
        [
            "customer",
            "date_dim",
            "item",
            "preferred_customers",
            "store",
            "store_sales"
        ]
    );
    let sales = dataset(&first, "store_sales");
    assert_eq!(
        sales["primary_key"],
        serde_json::json!(["ss_item_sk", "ss_ticket_number"])
    );
    assert!(sales.get("metrics").is_none(), "no metric is ever invented");
    assert_eq!(first["relationships"].as_array().unwrap().len(), 4);
    assert_eq!(
        dataset(&first, "customer")["unique_keys"],
        serde_json::json!([["c_customer_id"]])
    );

    // ── 2. Import the official Ossie model (ingest) → re-compile → export again → valid ──
    std::fs::create_dir_all(ws.join("semantic")).unwrap();
    std::fs::copy(
        fixture().join("semantic/tpcds.ossie.yaml"),
        ws.join("semantic/tpcds.ossie.yaml"),
    )
    .unwrap();
    let log = pipeline(&ws);
    assert!(
        log.contains("Ossie datasets       : 5 linked to a table, 0 unresolved, 0 ambiguous"),
        "{log}"
    );
    let conflicts = ok(
        &ws,
        &["conflicts", "list", "--type", "ossie_vs_schema", "--json"],
    );
    assert_eq!(
        serde_json::from_str::<Value>(&conflicts).unwrap(),
        serde_json::json!([])
    );

    let second = export(&ws, &["--status", "all"]);
    let sales = dataset(&second, "store_sales");
    assert_eq!(
        names(&sales["metrics"]),
        ["total_quantity"],
        "the model's own metric"
    );
    let metric_ext = ekos(&sales["metrics"][0]);
    assert_eq!(metric_ext["origin"], "ossie:semantic/tpcds.ossie.yaml");
    assert!(
        sales["ai_context"]["synonyms"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("POS data"))
    );
    // `customer` has a computed field in the Ossie file, so nothing that could reference it is
    // carried; its synonyms still are.
    let customer = dataset(&second, "customer");
    assert!(customer.get("metrics").is_none());
    assert!(
        customer["ai_context"]["synonyms"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("shoppers"))
    );
    // Model-scoped metrics name the file's datasets, not EKOS's: never exported (RFC 0245 §3).
    assert!(second.get("metrics").is_none());
    // Synonyms are searchable.
    assert!(ok(&ws, &["query", "find", "shoppers"]).contains("customer"));

    // An unchanged re-compile changes nothing in the export.
    pipeline(&ws);
    assert_eq!(export(&ws, &["--status", "all"]), second);

    // ── 3. Review in the file → import ossie → export the confirmed model → valid ─────────
    let mut edited = second.clone();
    let customer = edited["datasets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|d| d["name"] == "customer")
        .unwrap();
    let flag = field(customer, "c_preferred_cust_flag");
    let mut ext = ekos(flag);
    for m in ext["enum_meanings"].as_array_mut().unwrap() {
        match m["value"].as_str().unwrap() {
            "'Y'" => m["status"] = "confirmed".into(),
            "'N'" => m["label"] = "regular customer".into(),
            _ => {}
        }
    }
    set_ekos(flag, &ext);
    let concepts_at = edited["datasets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|d| d["name"] == "customer")
        .unwrap();
    let mut dext = ekos(concepts_at);
    dext["concepts"][0]["status"] = "rejected".into();
    dext["concepts"][0]["note"] = "a filter, not a business concept".into();
    set_ekos(concepts_at, &dext);
    let file = dir.path().join("review.ossie.json");
    std::fs::write(&file, serde_json::to_string_pretty(&edited).unwrap()).unwrap();
    let file = file.to_str().unwrap();

    let plan: Value =
        serde_json::from_str(&ok(&ws, &["import", "ossie", file, "--dry-run", "--json"])).unwrap();
    let mut actions: Vec<&str> = plan["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["action"].as_str().unwrap())
        .collect();
    actions.sort();
    assert_eq!(actions, ["confirm", "edit", "reject"], "{plan:#}");
    assert_eq!(plan["applied"], 0);
    let applied: Value = serde_json::from_str(&ok(
        &ws,
        &["import", "ossie", file, "--as", "e2e-reviewer", "--json"],
    ))
    .unwrap();
    assert_eq!(applied["applied"], 3);

    let confirmed = export(&ws, &[]);
    let flag = dataset(&confirmed, "customer")["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "c_preferred_cust_flag")
        .unwrap()
        .clone();
    assert_eq!(
        flag["ai_context"]["instructions"],
        "Codes: 'N' = regular customer; 'Y' = preferred customer."
    );
    assert!(
        dataset(&confirmed, "customer")["ai_context"]["instructions"].is_null(),
        "the rejected concept is gone from the confirmed model"
    );
    // The same file again plans nothing new: the decisions are in the ledger.
    let again: Value =
        serde_json::from_str(&ok(&ws, &["import", "ossie", file, "--dry-run", "--json"])).unwrap();
    assert!(again["errors"].as_array().unwrap().is_empty(), "{again:#}");
    assert!(
        again["decisions"].as_array().unwrap().is_empty(),
        "{again:#}"
    );

    // ── 4. Plant two disagreements in the Ossie file → exactly those are reported ─────────
    let text = std::fs::read_to_string(ws.join("semantic/tpcds.ossie.yaml")).unwrap();
    let mut model = ekos_ossie::parse(&text).unwrap().remove(0);
    let store = model
        .datasets
        .iter_mut()
        .find(|d| d.name == "store")
        .unwrap();
    store.fields.push(ekos_ossie::Field {
        name: "s_manager".into(),
        expression: ekos_ossie::Expression::sql("s_manager"),
        datatype: Some(ekos_ossie::DataType::String),
        ..Default::default()
    });
    let city = store
        .fields
        .iter_mut()
        .find(|f| f.name == "s_city")
        .unwrap();
    city.datatype = Some(ekos_ossie::DataType::Integer);
    std::fs::write(ws.join("semantic/tpcds.ossie.yaml"), model.to_yaml()).unwrap();
    pipeline(&ws);
    let conflicts: Value = serde_json::from_str(&ok(
        &ws,
        &["conflicts", "list", "--type", "ossie_vs_schema", "--json"],
    ))
    .unwrap();
    let mut attrs: Vec<String> = conflicts
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["attribute"].as_str().unwrap_or_default().to_string())
        .collect();
    attrs.sort();
    assert_eq!(
        attrs,
        ["columns.s_city.datatype", "columns.s_manager"],
        "{conflicts:#}"
    );
    // The planted model still exports a valid document.
    export(&ws, &["--status", "all"]);
}

#[test]
fn an_ekos_export_left_in_the_sources_is_not_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());
    pipeline(&ws);
    ok(&ws, &["export", "ossie", "--out", "exported.ossie.yaml"]);
    let log = pipeline(&ws);
    assert!(!log.contains("Ossie datasets"), "{log}");
    let found = ok(&ws, &["ekl", "FIND Object WHERE kind = 'OssieDataset'"]);
    assert!(found.contains("0 rows"), "{found}");
}

/// RFC 0246: `ekos export dbt-semantic` on the fixture, then dbt's own `dbt parse` with
/// MetricFlow over the result. Opt-in (`EKOS_DBT_VALIDATE=1`; installs dbt-core, dbt-duckdb and
/// dbt-metricflow through `uv`). The official TPC-DS model has no time axis on `store_sales`, so a
/// second, small model over `date_dim` gives MetricFlow a measure to validate.
#[test]
fn the_dbt_semantic_layer_parses_with_metricflow() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());
    std::fs::create_dir_all(ws.join("semantic")).unwrap();
    std::fs::copy(
        fixture().join("semantic/tpcds.ossie.yaml"),
        ws.join("semantic/tpcds.ossie.yaml"),
    )
    .unwrap();
    std::fs::write(
        ws.join("semantic/calendar.ossie.yaml"),
        r#"version: 0.2.0.dev0
name: calendar
datasets:
  - name: date_dim
    source: date_dim
    primary_key: [d_date_sk]
    fields:
      - {name: d_date_sk, expression: {sql: d_date_sk}, datatype: Integer}
      - {name: d_date, expression: {sql: d_date}, datatype: Date}
      - {name: d_year, expression: {sql: d_year}, datatype: Integer, dimension: {is_time: false}}
    metrics:
      - {name: days, expression: {sql: COUNT(d_date_sk)}, description: Calendar days}
      - {name: years_per_day, expression: {sql: COUNT(DISTINCT d_year) / COUNT(d_date_sk)}}
"#,
    )
    .unwrap();
    pipeline(&ws);
    let out = dir.path().join("dbt");
    let said = ok(
        &ws,
        &[
            "export",
            "dbt-semantic",
            "--out",
            out.to_str().unwrap(),
            "--with-time-spine",
        ],
    );
    assert!(said.contains("semantic model(s)"), "{said}");
    let layer = std::fs::read_to_string(out.join("semantic_models.yml")).unwrap();
    let y: serde_yaml::Value = serde_yaml::from_str(&layer).unwrap();
    let names: Vec<&str> = y["metrics"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"days") && names.contains(&"years_per_day"),
        "{names:?}"
    );
    let skipped = std::fs::read_to_string(out.join("skipped.md")).unwrap();
    assert!(
        skipped.contains("metric `cumulative_sales`"),
        "windows are skipped: {skipped}"
    );
    assert!(
        skipped.contains("time dimension on store_sales"),
        "{skipped}"
    );

    if std::env::var("EKOS_DBT_VALIDATE").is_err() {
        eprintln!("dbt parse skipped: set EKOS_DBT_VALIDATE=1 (needs uv and network)");
        return;
    }
    // A minimal dbt project: one model per semantic model, the layer, a DuckDB profile.
    let project = dir.path().join("dbtproj");
    std::fs::create_dir_all(project.join("models")).unwrap();
    std::fs::write(
        project.join("dbt_project.yml"),
        "name: ekos_ossie\nversion: '1.0'\nconfig-version: 2\nprofile: ekos_ossie\n",
    )
    .unwrap();
    std::fs::write(
        project.join("profiles.yml"),
        "ekos_ossie:\n  target: dev\n  outputs:\n    dev:\n      type: duckdb\n      path: ':memory:'\n",
    )
    .unwrap();
    for m in y["semantic_models"].as_sequence().unwrap() {
        let name = m["name"].as_str().unwrap();
        std::fs::write(
            project.join(format!("models/{name}.sql")),
            "select 1 as x\n",
        )
        .unwrap();
    }
    std::fs::copy(
        out.join("semantic_models.yml"),
        project.join("models/semantic_models.yml"),
    )
    .unwrap();
    for f in ["models.yml", "metricflow_time_spine.sql", "time_spine.yml"] {
        std::fs::copy(out.join(f), project.join("models").join(f)).unwrap();
    }
    let parse = Command::new("uv")
        .args([
            "run",
            "--no-project",
            "--with",
            "dbt-core",
            "--with",
            "dbt-duckdb",
        ])
        .args([
            "--with",
            "dbt-metricflow",
            "dbt",
            "parse",
            "--profiles-dir",
            ".",
        ])
        .current_dir(&project)
        .output()
        .expect("uv runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&parse.stdout),
        String::from_utf8_lossy(&parse.stderr)
    );
    assert!(parse.status.success(), "dbt parse failed:\n{text}");
}
