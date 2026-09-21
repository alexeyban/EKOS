//! `ekos session commit` core (RFC 0151 Phase 2): observe → map → append, for the session source
//! only. The inbox is the observed source: the pending batch is re-redacted (a hand-edited inbox
//! must not smuggle a secret past the write-time pass) and content-hashed into an artifact id.

use crate::anchor::AnchorIndex;
use crate::inbox::{Inbox, InboxError, Redactor, SessionEntry};
use crate::map::map_entries;
use ekos_common::ContentHash;
use ekos_ledger::{KnowledgeStore, LedgerError, provenance::WriteContext};

#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    #[error(transparent)]
    Inbox(#[from] InboxError),
    #[error(transparent)]
    Ledger(#[from] LedgerError),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitReport {
    pub session_id: String,
    pub artifact_id: String,
    pub committed: usize,
    pub claims_written: usize,
    pub claims_already_present: usize,
    pub anchors_resolved: usize,
    pub anchors_unresolved_or_ambiguous: usize,
}

/// The observation step: re-redacts each pending entry and hashes the batch.
pub fn observe(
    entries: &[SessionEntry],
    redactor: &dyn Redactor,
) -> Result<(Vec<SessionEntry>, String), CommitError> {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let mut e = e.clone();
        let mut r = |s: &str| {
            redactor
                .redact(s)
                .map_err(|m| CommitError::Inbox(InboxError::Redaction(m)))
        };
        e.text = r(&e.text)?;
        e.rationale = e.rationale.as_deref().map(&mut r).transpose()?;
        e.quote = e.quote.as_deref().map(&mut r).transpose()?;
        e.anchors = e.anchors.iter().map(|a| r(a)).collect::<Result<_, _>>()?;
        out.push(e);
    }
    let canonical = serde_json::to_string(&out).map_err(InboxError::from)?;
    Ok((out, ContentHash::of_str(&canonical).0))
}

/// Commits one session's pending notes. Claims already in the ledger (same `entry_id`) are left
/// untouched — a reviewed claim is never overwritten by a re-commit.
pub fn commit_session(
    store: &dyn KnowledgeStore,
    inbox: &Inbox,
    redactor: &dyn Redactor,
    session_id: &str,
    run_id: &str,
) -> Result<CommitReport, CommitError> {
    let (pending, total) = inbox.pending(session_id)?;
    let mut report = CommitReport {
        session_id: session_id.to_string(),
        ..Default::default()
    };
    if pending.is_empty() {
        return Ok(report);
    }
    let (observed, artifact_id) = observe(&pending, redactor)?;
    report.artifact_id = artifact_id.clone();

    let index = AnchorIndex::from_objects(store.all_objects()?);
    let batch = map_entries(
        session_id,
        &inbox.session_file_rel(session_id),
        &artifact_id,
        total - pending.len(),
        &observed,
        &index,
        total,
    );

    store.set_write_context(Some(WriteContext {
        run_id: run_id.to_string(),
        stage: "session-commit".into(),
        source_artifact_id: Some(format!("session:{artifact_id}")),
    }));
    let result = write_batch(store, &batch, &mut report);
    store.set_write_context(None);
    result?;

    inbox.mark_committed(session_id, total)?;
    report.committed = pending.len();
    Ok(report)
}

fn write_batch(
    store: &dyn KnowledgeStore,
    batch: &crate::map::MappedBatch,
    report: &mut CommitReport,
) -> Result<(), CommitError> {
    let fresh: std::collections::HashSet<_> = {
        let mut set = std::collections::HashSet::new();
        for c in &batch.claims {
            if store.get_object(&c.id)?.is_none() {
                set.insert(c.id);
            }
        }
        set
    };
    for ev in &batch.evidence {
        if store.get_evidence(&ev.id)?.is_none() {
            store.append_evidence(ev)?;
        }
    }
    if let Some(s) = &batch.session {
        store.append_object(s)?;
    }
    for c in &batch.claims {
        if fresh.contains(&c.id) {
            store.append_object(c)?;
            report.claims_written += 1;
        } else {
            report.claims_already_present += 1;
        }
    }
    for r in &batch.relationships {
        if fresh.contains(&r.from) {
            store.append_relationship(r)?;
        }
    }
    for e in &batch.events {
        if fresh.contains(&e.subject) {
            store.append_event(e)?;
        }
    }
    for c in &batch.claims {
        if let Some(anchors) = c.properties.get("anchors").and_then(|v| v.as_array()) {
            for a in anchors {
                if a["resolution"]["status"] == "resolved" {
                    report.anchors_resolved += 1;
                } else {
                    report.anchors_unresolved_or_ambiguous += 1;
                }
            }
        }
    }
    Ok(())
}
