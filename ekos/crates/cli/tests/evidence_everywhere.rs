//! Every object and every edge a full pipeline commits cites evidence (the Key Invariant "every
//! semantic conclusion must be traceable to Evidence"; issue #12 follow-up). One mixed workspace —
//! Rust, Python, SQL with a foreign key and a view, Markdown, a CI workflow, git history — goes
//! through `build → recover → resolve → compile → commit`, and the ledger is read back.
//!
//! A kind that legitimately carries no evidence must be listed in `EXEMPT` with its reason, so the
//! exemption is a decision someone made, not an accident nobody saw.

use ekos_kir::{KirObject, KirRelationship};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Kinds (object or relationship) allowed to have no evidence, with the reason.
const EXEMPT: &[(&str, &str)] = &[];

fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn workspace(dir: &Path) {
    write(
        dir,
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n",
    );
    write(
        dir,
        "app/src/lib.rs",
        "//! The app crate.\nuse std::collections::HashMap;\n\npub struct Ledger {\n    rows: HashMap<u32, i32>,\n}\n\nimpl Ledger {\n    pub fn sum(&self) -> i32 { total(1, 2) }\n}\n\npub fn total(a: i32, b: i32) -> i32 { add(a, b) }\nfn add(a: i32, b: i32) -> i32 { a + b }\n",
    );
    write(
        dir,
        "py/models.py",
        "import os\n\nclass Base:\n    pass\n\nclass Order(Base):\n    def total(self):\n        return helper()\n\ndef helper():\n    return os.getcwd()\n",
    );
    write(
        dir,
        "db/schema.sql",
        "CREATE TABLE customers (id INT PRIMARY KEY, name TEXT);\n\
         CREATE TABLE orders (id INT PRIMARY KEY, customer_id INT REFERENCES customers(id), status TEXT);\n\
         CREATE VIEW open_orders AS SELECT id FROM orders WHERE status IN ('new', 'paid');\n",
    );
    write(
        dir,
        "docs/guide.md",
        "# Guide\n\n## Orders\n\nThe `orders` table holds every order.\n",
    );
    write(
        dir,
        ".github/workflows/ci.yml",
        "name: CI\non: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo test\n",
    );
    write(
        dir,
        "ekos.toml",
        "[workspace]\nroot = \".\"\n\n[observe]\npaths = [\".\"]\nignore-patterns = [\".ekos\", \".git\"]\n\n\
         [recover.sql]\ndefault-dialect = \"postgres\"\n\n\
         [llm]\napi-key-env = \"EKOS_TEST_KEY_THAT_DOES_NOT_EXIST\"\n",
    );
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .unwrap();
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "initial"]);
}

fn kind_of_object(o: &KirObject) -> String {
    o.kind.to_string()
}

fn kind_of_rel(r: &KirRelationship) -> String {
    r.kind.to_string()
}

#[tokio::test]
async fn every_committed_object_and_edge_cites_evidence() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    workspace(dir);
    let config = ekos_compiler_core::EkosConfig::from_file(&dir.join("ekos.toml")).unwrap();
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();
    ekos::commands::recover::run(&config, dir, false)
        .await
        .unwrap();
    ekos::commands::resolve::run(&config, dir, false).unwrap();
    ekos::commands::compile::run(&config, dir).await.unwrap();
    ekos::commands::commit::run(&config, dir, true)
        .await
        .unwrap();

    let ledger = ekos::commands::store::open_store(&config, dir).unwrap();
    let objects = ledger.all_objects().unwrap();
    let rels = ledger.all_relationships().unwrap();
    assert!(
        objects.len() > 10,
        "the fixture compiled: {} objects",
        objects.len()
    );
    assert!(
        rels.len() > 10,
        "the fixture compiled: {} edges",
        rels.len()
    );

    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for o in &objects {
        *kinds
            .entry(format!("object {}", kind_of_object(o)))
            .or_default() += 1;
    }
    for r in &rels {
        *kinds.entry(format!("edge {}", kind_of_rel(r))).or_default() += 1;
    }
    eprintln!("compiled: {kinds:#?}");

    let exempt = |k: &str| EXEMPT.iter().any(|(e, _)| *e == k);
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    for o in objects.iter().filter(|o| o.evidence.is_empty()) {
        let k = format!("object {}", kind_of_object(o));
        if !exempt(&k) {
            *missing.entry(k).or_default() += 1;
        }
    }
    let kind_by_id: std::collections::HashMap<_, String> =
        objects.iter().map(|o| (o.id, kind_of_object(o))).collect();
    for r in rels.iter().filter(|r| r.evidence.is_empty()) {
        eprintln!(
            "bare edge: {} {} -> {}",
            kind_of_rel(r),
            kind_by_id.get(&r.from).map_or("?", String::as_str),
            kind_by_id.get(&r.to).map_or("?", String::as_str)
        );
        let k = format!("edge {}", kind_of_rel(r));
        if !exempt(&k) {
            *missing.entry(k).or_default() += 1;
        }
    }
    assert!(
        missing.is_empty(),
        "committed without evidence (kind → count): {missing:#?}\n\
         Attach the evidence the conclusion rests on, or list the kind in EXEMPT with the reason."
    );
}
