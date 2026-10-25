//! `ekos workflow status` — RFC 0213 §3.5, the step bar — and [`require_signoff`], the gate a
//! step 4–7 command calls before it writes anything.
//!
//! Step 1 comes from the committed manifest and freshness (RFC 0171), step 3 from the review
//! subjects' sign-offs ([`super::review::subject_statuses`]), steps 2 and 4–7 from
//! [`crate::extension::EkosExtension::workflow_steps`]; `ekos_semantic::workflow::assemble` puts
//! them together and applies the prerequisites.

use super::store::open_store_read_only;
use crate::extension::{Extensions, WorkflowContext};
use crate::freshness::{self, Status};
use anyhow::{Result, bail};
use ekos_compiler_core::EkosConfig;
use ekos_ledger::KnowledgeReader;
use ekos_semantic::review::Subject;
use ekos_semantic::workflow::{self, Step, StepState, SubjectStatus};
use serde_json::json;
use std::path::Path;

/// Step 1: done when the ledger was committed and still matches the source, stale when the source
/// changed since, todo before the first commit.
fn analyse_step(config: &EkosConfig, cwd: &Path, ledger: &dyn KnowledgeReader) -> Result<Step> {
    let f = freshness::check(config, cwd, 5);
    Ok(match f.status {
        Status::Fresh => Step::new(1, StepState::Done, f.summary_line()),
        Status::SourceChanged => Step::new(1, StepState::Stale, f.summary_line()),
        Status::Unknown if ledger.entry_count()? > 0 => Step::new(
            1,
            StepState::Done,
            format!("committed; freshness {}", f.summary_line()),
        ),
        Status::Unknown => Step::new(1, StepState::Todo, "nothing committed yet"),
    })
}

/// The whole bar, plus the subjects it was built from.
pub fn steps(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
    extensions: &Extensions,
) -> Result<(Vec<Step>, Vec<SubjectStatus>)> {
    let subjects = super::review::subject_statuses(config, cwd, ledger)?;
    let analyse = analyse_step(config, cwd, ledger)?;
    let ext = extensions.workflow_steps(&WorkflowContext {
        config,
        cwd,
        ledger,
        subjects: &subjects,
    });
    let review = workflow::review_step(subjects.clone());
    Ok((workflow::assemble(analyse, review, ext), subjects))
}

/// `ekos workflow status`
pub fn status(
    config: &EkosConfig,
    cwd: &Path,
    extensions: &Extensions,
    json_out: bool,
) -> Result<()> {
    let ledger = open_store_read_only(config, cwd)?;
    let (bar, _) = steps(config, cwd, &*ledger, extensions)?;
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "steps": bar }))?
        );
        return Ok(());
    }
    for s in &bar {
        println!(
            "{} {:<24} {:<8} {}",
            s.n,
            s.title,
            serde_json::to_value(s.state)?.as_str().unwrap_or(""),
            s.detail
        );
        for sub in &s.subjects {
            println!(
                "    {:<16} {:<8} {} item(s), {} blocking",
                sub.subject,
                sub.signoff.name(),
                sub.items,
                sub.blocking
            );
        }
    }
    Ok(())
}

/// RFC 0213: refuse unless every subject in `required` (all six when empty) has a valid
/// sign-off. A step 4–7 command calls this before writing anything; the error names each subject
/// that is not signed, stale or revoked. Recomputes from the ledger (through the summary cache,
/// which is keyed by everything the states depend on).
pub fn require_signoff(
    config: &EkosConfig,
    cwd: &Path,
    ledger: &dyn KnowledgeReader,
    required: &[Subject],
) -> Result<()> {
    let subjects = super::review::subject_statuses(config, cwd, ledger)?;
    let required = if required.is_empty() {
        &Subject::ALL[..]
    } else {
        required
    };
    if let Err(e) = workflow::gate(&subjects, required) {
        bail!(e);
    }
    Ok(())
}

/// [`require_signoff`] over the project's ledger, opened read-only — for a command that has no
/// store open yet.
pub fn require_signoff_in(config: &EkosConfig, cwd: &Path, required: &[Subject]) -> Result<()> {
    let ledger = open_store_read_only(config, cwd)?;
    require_signoff(config, cwd, &*ledger, required)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::store::open_store;
    use ekos_kir::{KirObject, ObjectKind};

    fn table(name: &str) -> KirObject {
        let mut o = KirObject::new(name, ObjectKind::Table);
        o.properties
            .insert("dialect".into(), serde_json::json!("oracle"));
        o
    }

    /// The summary is cached under a key that any ledger write moves: a cached answer is served
    /// while nothing changed, and recomputed after a write.
    #[test]
    fn the_summary_cache_is_served_until_the_ledger_changes() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let config = EkosConfig::default();
        {
            let ledger = open_store(&config, cwd).unwrap();
            ledger.append_object(&table("C_ORDER")).unwrap();
        }
        let ledger = open_store_read_only(&config, cwd).unwrap();
        let first = super::super::review::subject_statuses(&config, cwd, &*ledger).unwrap();
        assert_eq!(first[0].subject, "scope");
        assert_eq!(first[0].items, 1);

        // Tamper with the cached body, keep its key: the tampered value is what comes back.
        let path = config.ekos_dir(cwd).join("review").join("summary.json");
        let mut v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        v["subjects"][0]["items"] = serde_json::json!(999);
        std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
        let cached = super::super::review::subject_statuses(&config, cwd, &*ledger).unwrap();
        assert_eq!(cached[0].items, 999);
        drop(ledger);

        let ledger = open_store(&config, cwd).unwrap();
        ledger.append_object(&table("C_INVOICE")).unwrap();
        let fresh = super::super::review::subject_statuses(&config, cwd, &*ledger).unwrap();
        assert_eq!(fresh[0].items, 2, "a write must move the key");
    }

    #[test]
    fn the_gate_refuses_an_unsigned_project_and_the_bar_blocks_steps_4_to_7() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let config = EkosConfig::default();
        let ledger = open_store(&config, cwd).unwrap();
        ledger.append_object(&table("C_ORDER")).unwrap();
        let err = require_signoff(&config, cwd, &*ledger, &[])
            .unwrap_err()
            .to_string();
        for s in Subject::ALL {
            assert!(
                err.contains(&format!("{}: not signed off", s.as_str())),
                "{err}"
            );
        }
        let (bar, subjects) = steps(&config, cwd, &*ledger, &Extensions::none()).unwrap();
        assert_eq!(subjects.len(), 6);
        assert_eq!(bar.len(), 7);
        assert_eq!(bar[0].state, StepState::Done, "{}", bar[0].detail);
        assert_eq!(bar[2].state, StepState::Todo);
        assert!(bar[3..].iter().all(|s| s.state == StepState::Blocked));
    }
}
