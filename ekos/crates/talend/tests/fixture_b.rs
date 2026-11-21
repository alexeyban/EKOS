//! RFC 0263 exit gate on fixture B (a real Talend Open Studio 8.0.1 project), compared with the
//! independent census in `evals/talend/census.json` (`scripts/talend-census.py`).
//!
//! Needs `EKOS_TALEND_FIXTURE_B` (printed by `scripts/talend-fixtures.sh`); skipped, saying so,
//! when it is unset. The project is third-party and never vendored (RFC 0262 §Fixtures).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ekos_talend::{
    LinkKind, PROJECT_MARKER, Project, ProjectFile, Role, TriggerKind, read_project, role,
};

fn fixture() -> Option<PathBuf> {
    match std::env::var_os("EKOS_TALEND_FIXTURE_B") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("skipped: EKOS_TALEND_FIXTURE_B is not set (scripts/talend-fixtures.sh)");
            None
        }
    }
}

/// Every project under `root`, read the way the observer reads it: files whose role is kept.
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
                if role(&rel) == Role::Other {
                    return None;
                }
                Some(ProjectFile {
                    text: std::fs::read_to_string(e.path()).ok()?,
                    path: rel,
                })
            })
            .collect();
        let rel_dir = dir
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        out.push(read_project(&rel_dir, &files));
    }
    out
}

fn census() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../evals/talend/census.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn counts(value: &serde_json::Value) -> BTreeMap<String, u64> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_u64().unwrap()))
        .collect()
}

fn link_name(kind: &LinkKind) -> String {
    match kind {
        LinkKind::Main => "FLOW".into(),
        LinkKind::Lookup => "LOOKUP".into(),
        LinkKind::Filter => "FILTER".into(),
        LinkKind::Reject => "REJECT".into(),
        LinkKind::Unique => "UNIQUE".into(),
        LinkKind::Duplicate => "DUPLICATE".into(),
        LinkKind::Iterate => "ITERATE".into(),
        LinkKind::Other(o) => o.clone(),
    }
}

fn trigger_name(kind: &TriggerKind) -> String {
    match kind {
        TriggerKind::ComponentOk => "COMPONENT_OK".into(),
        TriggerKind::ComponentError => "COMPONENT_ERROR".into(),
        TriggerKind::SubjobOk => "SUBJOB_OK".into(),
        TriggerKind::SubjobError => "SUBJOB_ERROR".into(),
        TriggerKind::RunIf => "RUN_IF".into(),
        TriggerKind::Other(o) => o.clone(),
    }
}

fn tally<'a>(names: impl Iterator<Item = String> + 'a) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for n in names {
        *out.entry(n).or_insert(0) += 1;
    }
    out
}

#[test]
fn fixture_b_reads_exactly_as_the_census_counts() {
    let Some(root) = fixture() else { return };
    let projects = projects(&root);
    assert_eq!(
        projects.len(),
        1,
        "fixture B holds one project outside poms/"
    );
    let p = &projects[0];
    let expected = &census()["B"]["projects"][p.root.as_str()];
    assert!(expected.is_object(), "no census for project {}", p.root);
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);

    assert_eq!(p.jobs.len() as u64, expected["jobs"].as_u64().unwrap());
    let components = p.jobs.iter().flat_map(|j| &j.components);
    assert_eq!(
        tally(components.map(|c| c.component_type.clone())),
        counts(&expected["component_types"])
    );
    let links = p.jobs.iter().flat_map(|j| &j.links);
    assert_eq!(
        tally(links.map(|l| link_name(&l.kind))),
        counts(&expected["row_links"])
    );
    let triggers = p.jobs.iter().flat_map(|j| &j.triggers);
    assert_eq!(
        tally(triggers.map(|t| trigger_name(&t.kind))),
        counts(&expected["triggers"])
    );
    let actions = p.jobs.iter().flat_map(|j| &j.components).filter_map(|c| {
        c.params
            .get("DATA_ACTION")
            .and_then(ekos_talend::Param::text)
            .map(str::to_string)
    });
    assert_eq!(tally(actions), counts(&expected["data_actions"]));

    let mappers: Vec<_> = p
        .jobs
        .iter()
        .flat_map(|j| &j.components)
        .filter_map(|c| c.mapper.as_ref())
        .collect();
    let inputs: usize = mappers.iter().map(|m| m.inputs.len()).sum();
    let outputs: usize = mappers.iter().map(|m| m.outputs.len()).sum();
    let expressions: usize = mappers
        .iter()
        .flat_map(|m| {
            m.inputs
                .iter()
                .flat_map(|t| &t.entries)
                .chain(m.outputs.iter().flat_map(|t| &t.entries))
                .chain(&m.vars)
        })
        .filter(|e| e.expression.is_some())
        .count();
    assert_eq!(inputs as u64, expected["tmap_inputs"].as_u64().unwrap());
    assert_eq!(outputs as u64, expected["tmap_outputs"].as_u64().unwrap());
    assert_eq!(
        expressions as u64,
        expected["tmap_expressions"].as_u64().unwrap()
    );
}

#[test]
fn fixture_b_resolves_every_call_and_keeps_no_secret() {
    let Some(root) = fixture() else { return };
    let p = &projects(&root)[0];
    let calls: Vec<_> = p.jobs.iter().flat_map(|j| p.calls(j)).collect();
    assert_eq!(calls.len(), 11);
    for call in &calls {
        let r = call
            .resolved
            .as_ref()
            .unwrap_or_else(|| panic!("unresolved: {call:?}"));
        assert!(r.via_latest, "fixture B calls every child as Latest");
        assert!(call.die_on_child_error && call.transmit_whole_context);
    }
    let custom: Vec<_> = p.routines.iter().filter(|r| !r.system).collect();
    assert_eq!(custom.len(), 1);
    assert_eq!(p.routines.iter().filter(|r| r.system).count(), 9);
    assert_eq!(p.connections.len(), 3);
    assert!(
        p.connections
            .iter()
            .all(|c| c.engine.as_deref() == Some("PostgreSQL"))
    );

    let serialized = serde_json::to_string(p).unwrap();
    assert!(
        !serialized.contains("enc:system.encryption"),
        "an encrypted value reached the model"
    );
    assert!(!serialized.contains("D41D8CD98F00B204E9800998ECF8427E"));
}

#[test]
fn fixture_b_queries_fold_to_sql_over_the_default_context() {
    let Some(root) = fixture() else { return };
    let p = &projects(&root)[0];
    let mut complete = 0;
    let mut partial = 0;
    for job in &p.jobs {
        for c in job.components.iter().filter(|c| c.logical == "db_input") {
            let query = c.params["QUERY"].text().unwrap();
            let folded = job.fold(query);
            if folded.complete() {
                complete += 1;
                assert!(
                    folded
                        .text
                        .trim_start()
                        .to_ascii_uppercase()
                        .starts_with("SELECT"),
                    "{}",
                    folded.text
                );
                assert!(!folded.text.contains("context."), "{}", folded.text);
            } else {
                partial += 1;
            }
        }
    }
    // 22 tPostgresqlInput components, each a literal + context.schema_* concatenation.
    assert_eq!((complete, partial), (22, 0));
}

/// RFC 0264 Phase 2 exit: every target column has a lineage, and the plan's acceptance examples
/// hold (read by name here; `ekos-recovery` reads the SQL itself).
#[test]
fn fixture_b_column_lineage_reaches_the_acceptance_examples() {
    use ekos_talend::lineage::{NoQueries, job_lineage};
    let Some(root) = fixture() else { return };
    let p = &projects(&root)[0];
    assert!(p.joblets.is_empty(), "fixture B has no joblet");
    let mut all = Vec::new();
    for job in &p.jobs {
        all.extend(job_lineage(&p.expand(job).job, &NoQueries));
    }
    let formula = |table: &str, column: &str| {
        all.iter()
            .find(|l| l.target.name == table && l.target.column == column)
            .unwrap_or_else(|| panic!("no lineage for {table}.{column}"))
            .formula
            .clone()
    };
    assert_eq!(
        formula("FAIT_VENTE", "NB_MT_VENTE"),
        "sum(ODS_VENTE.QTE_VENTE*ODS_VENTE.PRIX_VENTE)"
    );
    // The probable defect the plan names: the quantity measure counts product keys.
    assert_eq!(
        formula("FAIT_VENTE", "NB_QTE_VENTE"),
        "count(DIM_PRODUIT.ID_DIM_PRODUIT)"
    );
    // The sales loader reads files a tFileList iterates: named by directory and mask.
    let ods = all
        .iter()
        .find(|l| l.target.name == "ODS_VENTE" && l.target.column == "QTE_VENTE")
        .expect("ODS_VENTE.QTE_VENTE");
    let files: Vec<String> = ods
        .origins
        .iter()
        .filter_map(|o| match o {
            ekos_talend::lineage::Origin::File {
                file,
                iterated_by: Some(_),
                ..
            } => Some(file.clone()),
            _ => None,
        })
        .collect();
    eprintln!("ODS_VENTE.QTE_VENTE from {files:?} as {}", ods.formula);
    assert!(
        files.iter().any(|f| f.contains("VENTE") && f.contains('*')),
        "{files:?}"
    );
    let gaps: Vec<String> = all
        .iter()
        .flat_map(|l| &l.gaps)
        .map(|g| format!("{} {} {}", g.component_type, g.column, g.reason))
        .collect();
    eprintln!(
        "{} lineages, {} with a gap: {gaps:#?}",
        all.len(),
        all.iter().filter(|l| !l.gaps.is_empty()).count()
    );
}
