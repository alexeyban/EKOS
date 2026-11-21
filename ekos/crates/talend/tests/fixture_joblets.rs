//! Joblets as Talend Studio writes them (RFC 0262 Part 0, fixtures C and D), compared with the
//! independent census in `evals/talend/census.json` (`scripts/talend-census.py`).
//!
//! Needs `EKOS_TALEND_FIXTURE_C` and `EKOS_TALEND_FIXTURE_D` (printed by
//! `scripts/talend-fixtures.sh`); skipped, saying so, when unset. Third-party projects, never
//! vendored (RFC 0262 §Fixtures).

use std::path::{Path, PathBuf};

use ekos_talend::{LinkKind, Project, TriggerKind};

mod common;
use common::projects;

fn fixture(var: &str) -> Option<PathBuf> {
    match std::env::var_os(var) {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("skipped: {var} is not set (scripts/talend-fixtures.sh)");
            None
        }
    }
}

fn census(fixture: &str, project: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../evals/talend/census.json");
    let all: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    all[fixture]["projects"][project].clone()
}

fn project<'a>(all: &'a [Project], root: &str) -> &'a Project {
    all.iter().find(|p| p.root == root).expect(root)
}

/// Every joblet is read, every use is counted as the census counts it, and every job expands
/// without a diagnostic.
fn joblets_match_the_census(fixture_name: &str, p: &Project) {
    let expected = census(fixture_name, &p.root);
    assert_eq!(
        p.joblets.len() as u64,
        expected["joblets"].as_u64().unwrap()
    );
    let uses: usize = p.jobs.iter().map(|j| p.joblet_uses(j).len()).sum();
    assert_eq!(uses as u64, expected["joblet_uses"].as_u64().unwrap());
    for job in &p.jobs {
        let e = p.expand(job);
        assert!(
            e.diagnostics.is_empty(),
            "{}: {:?}",
            job.name,
            e.diagnostics
        );
        assert_eq!(e.inlined, p.joblet_uses(job).len(), "{}", job.name);
    }
}

#[test]
fn fixture_c_joblets_expand_with_their_lookups_and_triggers() {
    let Some(root) = fixture("EKOS_TALEND_FIXTURE_C") else {
        return;
    };
    let all = projects(&root);
    let p = project(&all, "ACCOUNT5");
    joblets_match_the_census("C", p);

    // One data joblet used twice: the main flow and a lookup into the same tMap.
    let job = p
        .jobs
        .iter()
        .find(|j| j.name == "CDC_HashBased_Customers_WithJoble")
        .unwrap();
    let e = p.expand(job);
    let into_map: Vec<(&str, &LinkKind, Option<&str>)> = e
        .job
        .links
        .iter()
        .filter(|l| l.to == "tMap_1")
        .map(|l| (l.from.as_str(), &l.kind, l.label.as_deref()))
        .collect();
    assert!(
        into_map.contains(&("cdcHashJoblet_1__tMap_1", &LinkKind::Main, Some("row5"))),
        "{into_map:?}"
    );
    assert!(
        into_map.contains(&("cdcHashJoblet_3__tMap_1", &LinkKind::Lookup, Some("row6"))),
        "{into_map:?}"
    );
    // Each inlined copy is fed by its own file, though both of its tMaps read the label `row1`
    // (the job ends in tLogRow, so it has no target column to walk back from).
    for (file, copy) in [
        ("tFileInputDelimited_1", "cdcHashJoblet_1__tMap_1"),
        ("tFileInputDelimited_2", "cdcHashJoblet_3__tMap_1"),
    ] {
        let feeds: Vec<(&str, Option<&str>)> = e
            .job
            .links
            .iter()
            .filter(|l| l.to == copy)
            .map(|l| (l.from.as_str(), l.label.as_deref()))
            .collect();
        assert_eq!(feeds, [(file, Some("row1"))]);
    }

    // One trigger joblet fires the next through a link named after its TRIGGER_OUTPUT port.
    let job = p
        .jobs
        .iter()
        .find(|j| j.name == "TimeCalculationJob")
        .unwrap();
    let e = p.expand(job);
    assert!(
        e.job
            .triggers
            .iter()
            .any(|t| t.kind == TriggerKind::ComponentOk
                && t.from == "StartTime_Joblet1_1__tJava_1"
                && t.to == "EndTime_Joblet1_1__tJava_1"),
        "{:?}",
        e.job.triggers
    );
    // No secret value survives reading, whatever form the item stores it in.
    assert!(!format!("{all:?}").contains("enc:system"));
}

#[test]
fn fixture_d_a_caller_uses_the_second_output_of_a_two_port_joblet() {
    let Some(root) = fixture("EKOS_TALEND_FIXTURE_D") else {
        return;
    };
    let all = projects(&root);
    let p = project(&all, "SSS_SOURCE");
    joblets_match_the_census("D", p);
    let job = p
        .jobs
        .iter()
        .find(|j| j.name == "JOblet_refractor")
        .unwrap();
    let e = p.expand(job);
    // `OUTPUT_2` is fed by the joblet tMap's output `out1`; `OUTPUT_3` (`MM`) is not used.
    let out: Vec<(&str, Option<&str>)> = e
        .job
        .links
        .iter()
        .filter(|l| l.to == "tLogRow_3")
        .map(|l| (l.from.as_str(), l.metaname.as_deref()))
        .collect();
    assert_eq!(out, [("SAmple_file_tmap_1__tMap_1", Some("out1"))]);
}
