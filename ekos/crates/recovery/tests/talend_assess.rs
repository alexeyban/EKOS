//! RFC 0265's exit on real projects: the migration assessment on Talend fixtures B–E.
//!
//! Needs `EKOS_TALEND_FIXTURE_B` … `_E` (printed by `scripts/talend-fixtures.sh`); each is skipped,
//! saying so, when unset. Third-party projects, never vendored (RFC 0262 §Fixtures).

use std::path::Path;

use ekos_kir::{KirObject, ObjectKind};
use ekos_recovery::talend_analyzer::project_kir;
use ekos_talend::{PROJECT_MARKER, Project, ProjectFile, Role, read_project, role};
use serde_json::Value;

/// Every project under `root`, read the way the observer reads it.
fn projects(root: &Path) -> Vec<Project> {
    let mut out = Vec::new();
    for marker in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            !ekos_talend::SKIPPED_DIRS.contains(&e.file_name().to_string_lossy().as_ref())
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == PROJECT_MARKER)
    {
        let dir = marker.path().parent().unwrap();
        let files: Vec<ProjectFile> = walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let rel = e
                    .path()
                    .strip_prefix(dir)
                    .ok()?
                    .to_string_lossy()
                    .replace('\\', "/");
                (role(&rel) != Role::Other).then_some(())?;
                Some(ProjectFile {
                    text: std::fs::read_to_string(e.path()).ok()?,
                    path: rel,
                })
            })
            .collect();
        let rel = dir
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        out.push(read_project(&rel, &files));
    }
    out
}

fn findings(p: &Project) -> Vec<KirObject> {
    let (kir, _, diagnostics) = project_kir(p);
    assert!(diagnostics.is_empty(), "{}: {diagnostics:?}", p.root);
    kir.objects
        .into_iter()
        .filter(|o| o.kind == ObjectKind::Custom("TalendFinding".into()))
        .collect()
}

fn prop<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties
        .get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn print(fixture: &str, p: &Project, f: &[KirObject]) {
    for o in f {
        eprintln!(
            "{fixture} {} | {} | {} | {}",
            p.root,
            prop(o, "rule"),
            prop(o, "subject"),
            prop(o, "message")
        );
    }
}

fn fixture(var: &str) -> Option<std::path::PathBuf> {
    let p = std::env::var_os(var).map(std::path::PathBuf::from);
    if p.is_none() {
        eprintln!("skipped: {var} is not set (scripts/talend-fixtures.sh)");
    }
    p
}

#[test]
fn fixture_b_assessment_finds_every_acceptance_example() {
    let Some(root) = fixture("EKOS_TALEND_FIXTURE_B") else {
        return;
    };
    let all = projects(&root);
    let p = all
        .iter()
        .find(|p| p.root.ends_with("ALIMENTATION_DATA_WAREHOUSE"))
        .unwrap();
    let f = findings(p);
    print("B", p, &f);
    let has = |rule: &str, subject_part: &str| {
        f.iter()
            .any(|o| prop(o, "rule") == rule && prop(o, "subject").contains(subject_part))
    };
    assert!(has("measure_ignores_named_column", "NB_QTE_VENTE"));
    assert!(has("leap_year_by_four", "jGenerateCalendar"));
    assert!(f.iter().any(|o| prop(o, "rule") == "repeated_message"
        && prop(o, "message").contains("fichier Categorie")
        && prop(o, "message").contains("in 6 jobs")));
    assert!(
        !has("unused_item", "PARAMS_LOG"),
        "implicit context load reads through PARAMS_LOG"
    );
    assert!(has("foreign_lookup_name", "jOdsVente.sousCategorie"));
    assert!(has("uncalled_job", "jGenerateCalendar"));
    assert!(f.iter().any(|o| prop(o, "rule") == "absolute_path"
        && prop(o, "subject").starts_with("jGenerateCalendar")
        && prop(o, "message").contains("person's own folders")));
    let (kir, _, _) = project_kir(p);
    let project = kir
        .objects
        .iter()
        .find(|o| o.kind == ObjectKind::Custom("TalendProject".into()))
        .unwrap();
    let clusters = &project.properties["assessment"]["clone_clusters"];
    eprintln!("clusters: {clusters}");
    let ods: Vec<&str> = [
        "jOdsCategorie",
        "jOdsClient",
        "jOdsProduit",
        "jOdsSousCategorie",
        "jOdsTypeClient",
        "jOdsVente",
    ]
    .to_vec();
    assert!(
        clusters.as_array().unwrap().iter().any(|c| {
            let jobs: Vec<&str> = c["jobs"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect();
            ods.iter().all(|j| jobs.contains(j))
        }),
        "the six ODS jobs are one cluster: {clusters}"
    );
}

#[test]
fn fixtures_c_d_e_are_assessed_and_every_finding_cites_a_file() {
    for (name, var) in [
        ("C", "EKOS_TALEND_FIXTURE_C"),
        ("D", "EKOS_TALEND_FIXTURE_D"),
        ("E", "EKOS_TALEND_FIXTURE_E"),
    ] {
        let Some(root) = fixture(var) else {
            continue;
        };
        for p in projects(&root) {
            let f = findings(&p);
            print(name, &p, &f);
            if name == "E" && p.root == "POC_DATACLOUD" {
                // A real copy: the STRATIO extract job's error messages name the SAP job's children.
                assert!(f.iter().any(|o| prop(o, "rule") == "repeated_message"
                    && prop(o, "message").contains("ext_src_to_stg_DIM_MARA")
                    && prop(o, "message").contains("main_extract_STRATIO")));
                assert!(
                    !f.iter()
                        .any(|o| prop(o, "rule") == "measure_ignores_named_column")
                );
            }
            for o in &f {
                let ev = o.properties["evidence"].as_array().unwrap();
                assert!(
                    ev.iter()
                        .all(|e| !e["path"].as_str().unwrap_or_default().is_empty()),
                    "{o:?}"
                );
            }
        }
    }
}
