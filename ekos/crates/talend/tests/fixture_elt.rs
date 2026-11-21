//! ELT maps and several engines in one project (RFC 0262 Part 0, fixture E), compared with the
//! independent census in `evals/talend/census.json` (`scripts/talend-census.py`).
//!
//! Needs `EKOS_TALEND_FIXTURE_E` (printed by `scripts/talend-fixtures.sh`); skipped, saying so,
//! when unset. A third-party project, never vendored (RFC 0262 §Fixtures).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ekos_talend::LinkKind;
use ekos_talend::lineage::{NoQueries, job_lineage};

mod common;
use common::projects;

const PROJECT: &str = "POC_DATACLOUD";

fn census() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../evals/talend/census.json");
    let all: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    all["E"]["projects"][PROJECT].clone()
}

fn count(v: &serde_json::Value) -> usize {
    v.as_u64().unwrap() as usize
}

#[test]
fn fixture_e_elt_maps_read_as_the_census_counts_them_and_their_columns_reach_tables() {
    let Some(root) = std::env::var_os("EKOS_TALEND_FIXTURE_E").map(PathBuf::from) else {
        eprintln!("skipped: EKOS_TALEND_FIXTURE_E is not set (scripts/talend-fixtures.sh)");
        return;
    };
    let all = projects(&root);
    let p = all.iter().find(|p| p.root == PROJECT).expect(PROJECT);
    let expected = census();
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    assert_eq!(p.jobs.len(), count(&expected["jobs"]));
    let components: usize = p.jobs.iter().map(|j| j.components.len()).sum();
    assert_eq!(components, count(&expected["components"]));
    let links: usize = p
        .jobs
        .iter()
        .map(|j| j.links.len() + j.triggers.len())
        .sum();
    assert_eq!(links, count(&expected["links"]));
    let table_links = p
        .jobs
        .iter()
        .flat_map(|j| &j.links)
        .filter(|l| l.kind == LinkKind::Table)
        .count();
    assert_eq!(table_links, count(&expected["row_links"]["TABLE"]));

    let maps: Vec<_> = p
        .jobs
        .iter()
        .flat_map(|j| &j.components)
        .filter_map(|c| c.elt.as_ref())
        .collect();
    let inputs: usize = maps.iter().map(|m| m.inputs.len()).sum();
    let outputs: usize = maps.iter().map(|m| m.outputs.len()).sum();
    let expressions = maps
        .iter()
        .flat_map(|m| m.outputs.iter().flat_map(|o| &o.entries))
        .chain(maps.iter().flat_map(|m| {
            m.inputs
                .iter()
                .flat_map(|i| i.join_keys.iter().map(|_| &JOIN_KEY))
        }))
        .filter(|e| e.expression.is_some())
        .count();
    let filters: usize = maps
        .iter()
        .flat_map(|m| &m.outputs)
        .map(|o| o.filters.len() + o.clauses.len())
        .sum();
    assert_eq!(inputs, count(&expected["elt_inputs"]));
    assert_eq!(outputs, count(&expected["elt_outputs"]));
    assert_eq!(expressions, count(&expected["elt_expressions"]));
    // The census counts every filter entry; the model keeps the ones with text.
    assert!(filters <= count(&expected["elt_filters"]));

    let mut engines: BTreeMap<String, usize> = BTreeMap::new();
    for c in p.jobs.iter().flat_map(|j| &j.components) {
        if let Some(e) = &c.engine {
            *engines.entry(e.clone()).or_default() += 1;
        }
    }
    eprintln!("engines: {engines:?}");
    for engine in ["snowflake", "oracle", "postgresql"] {
        assert!(engines.contains_key(engine), "{engine}: {engines:?}");
    }

    let (mut elt, mut with_gap, mut reasons) = (0, 0, BTreeMap::<String, usize>::new());
    for job in &p.jobs {
        for l in job_lineage(job, &NoQueries) {
            if job
                .component(&l.component)
                .is_some_and(|c| c.logical == "elt_output")
            {
                elt += 1;
                if !l.gaps.is_empty() {
                    with_gap += 1;
                    for g in &l.gaps {
                        *reasons.entry(g.reason.clone()).or_default() += 1;
                    }
                }
                assert!(
                    l.steps.iter().any(|s| s.kind == "elt_map"),
                    "{}: {}.{}",
                    job.name,
                    l.target.name,
                    l.target.column
                );
            }
        }
    }
    eprintln!("ELT target columns: {elt}, with a gap: {with_gap}, reasons: {reasons:?}");
    assert!(elt > 0);
    assert_eq!(with_gap, 0, "{reasons:?}");
}

/// A join key counts as an expression in the census; this stands in for it in the count above.
static JOIN_KEY: ekos_talend::MapperEntry = ekos_talend::MapperEntry {
    name: String::new(),
    talend_type: None,
    expression: Some(String::new()),
    operator: None,
};
