//! RFC 0245: reading the official examples, building from a ledger, and validating every result
//! against the vendored official JSON Schema.

use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ossie::*;
use serde_json::{Value, json};

const FLIGHTS: &str = include_str!("fixtures/flights.semantic_model.yaml");
const TPCDS: &str = include_str!("fixtures/tpcds_semantic_model.yaml");
const TPCDS_011: &str = include_str!("fixtures/tpcds_semantic_model.v0_1_1.yaml");

fn schema_errors(m: &SemanticModel) -> Vec<String> {
    let schema: Value = serde_json::from_str(SCHEMA_JSON).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let instance = m.to_json();
    validator
        .iter_errors(&instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect()
}

fn assert_valid(m: &SemanticModel) {
    assert_eq!(schema_errors(m), Vec::<String>::new(), "official schema");
    assert_eq!(check(m), Vec::<Problem>::new(), "structural checks");
}

#[test]
fn the_official_examples_read_and_write_back_valid() {
    for text in [FLIGHTS, TPCDS] {
        assert!(sniff(text));
        let models = parse(text).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].version, SPEC_VERSION);
        assert_valid(&models[0]);
        // Written back and read again, nothing is lost.
        let again = parse(&models[0].to_yaml()).unwrap();
        assert_eq!(again, models);
    }
    let flights = &parse(FLIGHTS).unwrap()[0];
    assert_eq!(flights.datasets.len(), 6);
}

#[test]
fn a_0_1_1_document_is_migrated_model_by_model() {
    assert!(sniff(TPCDS_011));
    let models = parse(TPCDS_011).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].version, "0.1.1");
    assert_eq!(models[0].name, "tpcds_retail_model");
    // 0.1.1 expressions carry only `dialects`; the portable text is still found.
    let f = &models[0].datasets[0].fields[0];
    assert_eq!(f.expression.text(), Some("ss_sold_date_sk"));
    let mut current = models[0].clone();
    current.version = SPEC_VERSION.into();
    assert_eq!(schema_errors(&current), Vec::<String>::new());
    // 0.1.1 keys named source columns; 0.2 requires fields, and this example's key names a
    // column it never declares as a field. The checker says so rather than passing it.
    let problems: Vec<String> = check(&current).into_iter().map(|p| p.message).collect();
    assert_eq!(
        problems,
        [
            "`ss_ticket_number` is not a field of `store_sales`",
            "`ss_ticket_number` is not a field of `store_sales`"
        ]
    );

    let two = "version: '0.1.1'\nsemantic_model:\n- name: a\n  datasets: [{name: x, source: t}]\n\
               - name: b\n  datasets: [{name: y, source: u}]\n";
    let names: Vec<String> = parse(two).unwrap().into_iter().map(|m| m.name).collect();
    assert_eq!(names, ["a", "b"], "never only the first model");
    assert!(parse("version: '0.1.1'\nsemantic_model: []\n").is_err());
}

#[test]
fn sniffing_rejects_other_yaml() {
    for text in [
        "version: 2\nmodels:\n- name: x\n",
        "name: CI\non: push\n",
        "version: '3.8'\nservices: {}\ndatasets: 1\n",
        "not: [valid",
    ] {
        assert!(!sniff(text), "{text}");
    }
}

// ── Building from a ledger ──────────────────────────────────────────────────────────────────

fn obj(name: &str, kind: ObjectKind, props: Value) -> KirObject {
    let mut o = KirObject::new(name, kind);
    o.id = KirId(uuid_of(&format!("{kind:?}:{name}", kind = o.kind)));
    o.properties = serde_json::from_value(props).unwrap();
    o
}

fn uuid_of(s: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, s.as_bytes())
}

fn rel(kind: RelationshipKind, from: &KirObject, to: &KirObject, props: Value) -> KirRelationship {
    let mut r = KirRelationship::new(kind, from.id, to.id);
    r.id = KirId(uuid_of(&format!("{}->{}", from.name, to.name)));
    r.properties = serde_json::from_value(props).unwrap();
    r
}

fn custom(k: &str) -> ObjectKind {
    ObjectKind::Custom(k.into())
}

struct Fixture {
    objects: Vec<KirObject>,
    relationships: Vec<KirRelationship>,
    items: Vec<KirObject>,
}

fn fixture() -> Fixture {
    let orders = obj(
        "public.orders",
        ObjectKind::Table,
        json!({
            "dialect": "postgres",
            "description": "Customer orders",
            "columns": [
                {"name": "id", "data_type": "integer", "primary_key": true},
                {"name": "customer_id", "data_type": "integer"},
                {"name": "status", "data_type": "char(1)"},
                {"name": "placed_at", "data_type": "timestamp with time zone"},
                {"name": "total", "data_type": "numeric(12,2)"},
                {"name": "Odd Name", "data_type": "text"},
            ],
        }),
    );
    let customers = obj(
        "customers",
        ObjectKind::Table,
        json!({"columns": [{"name": "id", "data_type": "bigint"}, {"name": "email", "data_type": "text", "unique": true}],
               "primary_key": ["id"]}),
    );
    let lines = obj(
        "ORDER_LINE",
        ObjectKind::Table,
        json!({"origin": "live", "database": "erp", "owner": "SALES",
               "columns": [{"name": "ORDER_ID", "data_type": "NUMBER(10)"},
                           {"name": "LINE_NO", "data_type": "NUMBER(4,0)"},
                           {"name": "AMOUNT", "data_type": "NUMBER(12,2)"}],
               "primary_key": ["ORDER_ID", "LINE_NO"],
               "unique_keys": [["ORDER_ID", "LINE_NO"]]}),
    );
    let view = obj(
        "open_orders",
        custom("View"),
        json!({"columns": ["id", "customer_id"]}),
    );
    // An Ossie file in the sources, linked to `customers` at commit.
    let od = obj(
        "customers",
        custom("OssieDataset"),
        json!({"path": "semantic/sales.ossie.yaml", "synonyms": ["clients"],
               "fields": [{"name": "id", "sql": "id"},
                          {"name": "email", "sql": "email", "synonyms": ["e-mail"]}]}),
    );
    let om = obj(
        "customer_count",
        custom("OssieMetric"),
        json!({"expression": {"sql": "COUNT(DISTINCT id)"}, "datatype": "Integer",
               "description": "Number of customers"}),
    );
    let relationships = vec![
        rel(
            RelationshipKind::ForeignKey,
            &orders,
            &customers,
            json!({"fk_desc": "public.orders.customer_id → customers.id"}),
        ),
        rel(
            RelationshipKind::ForeignKey,
            &lines,
            &orders,
            json!({"fk_desc": "ORDER_LINE.ORDER_ID → orders.id", "origin": "live"}),
        ),
        rel(
            RelationshipKind::DependsOn,
            &od,
            &customers,
            json!({"link": "ossie_source"}),
        ),
        rel(RelationshipKind::Contains, &od, &om, json!({})),
    ];
    let items = vec![
        obj(
            "orders.status = 'O'",
            custom("EnumMeaning"),
            json!({"table": "orders", "column": "status", "value": "'O'", "label": "open", "status": "confirmed"}),
        ),
        obj(
            "orders.status = 'C'",
            custom("EnumMeaning"),
            json!({"table": "orders", "column": "status", "value": "'C'", "label": "closed",
                   "expert_label": "closed by finance", "status": "hypothesis"}),
        ),
        obj(
            "OpenOrder",
            ObjectKind::BusinessConcept,
            json!({"table": "orders", "definition": "status = 'O'", "description": "An order not yet shipped",
                   "status": "confirmed"}),
        ),
    ];
    Fixture {
        objects: vec![orders, customers, lines, view, od, om],
        relationships,
        items,
    }
}

fn build_with(f: &Fixture, status: &str) -> SemanticModel {
    let admits = |s: &str| match status {
        "confirmed" => s == "confirmed",
        "hypothesis" => s == "hypothesis",
        _ => s != "rejected",
    };
    let opts = BuildOptions {
        name: "shop".into(),
        export_status: status.into(),
        admits: &admits,
        database: None,
        source: None,
        ekos_version: "test".into(),
        project: Some("shop".into()),
    };
    let view = LedgerView {
        objects: &f.objects,
        relationships: &f.relationships,
        items: &f.items,
    };
    from_ledger(&view, &opts, &|o| vec![format!("{}.sql:1", o.name)]).unwrap()
}

fn dataset<'a>(m: &'a SemanticModel, name: &str) -> &'a Dataset {
    m.datasets.iter().find(|d| d.name == name).unwrap()
}

#[test]
fn a_ledger_exports_as_a_valid_model() {
    let m = build_with(&fixture(), "all");
    assert_valid(&m);
    let names: Vec<&str> = m.datasets.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["customers", "open_orders", "ORDER_LINE", "orders"]);

    let orders = dataset(&m, "orders");
    assert_eq!(orders.source, "public.orders");
    assert_eq!(orders.primary_key, ["id"]);
    let odd = orders.fields.iter().find(|f| f.name == "Odd Name").unwrap();
    assert_eq!(odd.expression.sql.as_deref(), Some("\"Odd Name\""));
    let placed = orders
        .fields
        .iter()
        .find(|f| f.name == "placed_at")
        .unwrap();
    assert_eq!(placed.datatype, Some(DataType::DateTimeTz));
    assert!(placed.is_time());

    let lines = dataset(&m, "ORDER_LINE");
    assert_eq!(lines.source, "erp.SALES.ORDER_LINE");
    assert_eq!(lines.primary_key, ["ORDER_ID", "LINE_NO"]);
    assert_eq!(lines.unique_keys, [vec!["ORDER_ID", "LINE_NO"]]);
    assert_eq!(lines.fields[0].datatype, Some(DataType::Integer));
    assert_eq!(lines.fields[2].datatype, Some(DataType::Decimal));

    let customers = dataset(&m, "customers");
    assert_eq!(customers.unique_keys, [vec!["email"]]);

    // Both foreign keys, over fields.
    let rels: Vec<(&str, &str, &[String])> = m
        .relationships
        .iter()
        .map(|r| (r.from.as_str(), r.to.as_str(), r.from_columns.as_slice()))
        .collect();
    assert_eq!(rels.len(), 2);
    assert!(rels.contains(&("orders", "customers", &["customer_id".to_string()][..])));
    assert!(rels.contains(&("ORDER_LINE", "orders", &["ORDER_ID".to_string()][..])));
}

#[test]
fn meanings_carry_their_review_status_in_words() {
    let m = build_with(&fixture(), "all");
    let orders = dataset(&m, "orders");
    let status = orders.fields.iter().find(|f| f.name == "status").unwrap();
    let text = status.ai_context.as_ref().unwrap().instructions().unwrap();
    assert_eq!(
        text,
        "Codes: 'C' = closed by finance (hypothesis, unreviewed); 'O' = open."
    );
    let ext = ekos_data(&status.custom_extensions).unwrap();
    assert_eq!(ext["enum_meanings"].as_array().unwrap().len(), 2);
    let ds = orders.ai_context.as_ref().unwrap().instructions().unwrap();
    assert!(ds.contains("Business concept \"OpenOrder\": An order not yet shipped"));

    // `--status confirmed` leaves the hypothesis out entirely.
    let m = build_with(&fixture(), "confirmed");
    let status = dataset(&m, "orders")
        .fields
        .iter()
        .find(|f| f.name == "status")
        .unwrap();
    let text = status.ai_context.as_ref().unwrap().instructions().unwrap();
    assert_eq!(text, "Codes: 'O' = open.");
    assert_valid(&m);
}

#[test]
fn ingested_ossie_metrics_and_synonyms_ride_along_never_invented_ones() {
    let m = build_with(&fixture(), "all");
    let customers = dataset(&m, "customers");
    assert_eq!(
        customers.ai_context.as_ref().unwrap().synonyms(),
        ["clients"]
    );
    let email = customers.fields.iter().find(|f| f.name == "email").unwrap();
    assert_eq!(email.ai_context.as_ref().unwrap().synonyms(), ["e-mail"]);
    assert_eq!(customers.metrics.len(), 1);
    assert_eq!(customers.metrics[0].name, "customer_count");
    let ext = ekos_data(&customers.metrics[0].custom_extensions).unwrap();
    assert_eq!(ext["origin"], "ossie:semantic/sales.ossie.yaml");
    // No other dataset has a metric, and the model has none of its own.
    assert!(
        m.datasets
            .iter()
            .filter(|d| d.name != "customers")
            .all(|d| d.metrics.is_empty())
    );
    assert!(m.metrics.is_empty());

    // A renamed field makes the ingested metric's references unsafe: it is not carried.
    let mut f = fixture();
    f.objects[4].properties.insert(
        "fields".into(),
        json!([{"name": "customer_key", "sql": "id"}]),
    );
    let m = build_with(&f, "all");
    assert!(dataset(&m, "customers").metrics.is_empty());
}

#[test]
fn the_export_is_deterministic_whatever_the_input_order() {
    let a = build_with(&fixture(), "all").to_yaml();
    let mut f = fixture();
    f.objects.reverse();
    f.relationships.reverse();
    f.items.reverse();
    let b = build_with(&f, "all").to_yaml();
    assert_eq!(a, b);
}

#[test]
fn an_empty_ledger_is_an_error_not_an_invalid_document() {
    let f = Fixture {
        objects: vec![],
        relationships: vec![],
        items: vec![],
    };
    let admits = |_: &str| true;
    let opts = BuildOptions {
        name: "x".into(),
        export_status: "all".into(),
        admits: &admits,
        database: None,
        source: None,
        ekos_version: "t".into(),
        project: None,
    };
    let view = LedgerView {
        objects: &f.objects,
        relationships: &f.relationships,
        items: &f.items,
    };
    assert!(from_ledger(&view, &opts, &|_| vec![]).is_err());
}

#[test]
fn the_checker_finds_what_the_official_validator_finds() {
    let mut m = parse(FLIGHTS).unwrap().remove(0);
    m.relationships.push(Relationship {
        name: "bad".into(),
        from: "FLIGHT".into(),
        to: "NOPE".into(),
        from_columns: vec!["tail_nr".into(), "x".into()],
        to_columns: vec!["y".into()],
        ..Default::default()
    });
    m.datasets[0].primary_key = vec!["missing".into()];
    m.datasets[1].name = "runway".into(); // case-folded duplicate of RUNWAY
    let problems: Vec<String> = check(&m).into_iter().map(|p| p.message).collect();
    assert!(
        problems.iter().any(|p| p.contains("used twice")),
        "{problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|p| p.contains("`missing` is not a field"))
    );
    assert!(problems.iter().any(|p| p.contains("no dataset `NOPE`")));
    assert!(problems.iter().any(|p| p.contains("equally long")));
    assert!(
        problems
            .iter()
            .any(|p| p.contains("`x` is not a field of `FLIGHT`"))
    );
}

/// The official `validation/validate.py` (JSON Schema, names, references, arity, and a sqlglot
/// parse of every expression) on the fixture export and an official example. Opt-in: set
/// `EKOS_OSSIE_REPO` to a clone of apache/ossie; runs Python through `uv` (which fetches the
/// validator's three packages).
#[test]
fn the_official_validator_accepts_the_export() {
    let Ok(repo) = std::env::var("EKOS_OSSIE_REPO") else {
        eprintln!("skipped: set EKOS_OSSIE_REPO to a clone of apache/ossie");
        return;
    };
    let repo = std::path::Path::new(&repo);
    let dir = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    for (name, model) in [
        ("export_all.yaml", build_with(&fixture(), "all")),
        ("export_confirmed.yaml", build_with(&fixture(), "confirmed")),
        ("flights.yaml", parse(FLIGHTS).unwrap().remove(0)),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, model.to_yaml()).unwrap();
        files.push(path);
    }
    let out = std::process::Command::new("uv")
        .args([
            "run",
            "--no-project",
            "--with",
            "pyyaml",
            "--with",
            "jsonschema",
        ])
        .args(["--with", "sqlglot", "python", "-I"])
        .arg(repo.join("validation/validate.py"))
        .arg("--schema")
        .arg(repo.join("core-spec/ossie-schema.json"))
        .arg("--")
        .args(&files)
        .current_dir(dir.path())
        .output()
        .expect("uv runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert_eq!(text.matches("Validation PASSED").count(), 3, "{text}");
}
