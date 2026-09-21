//! `ekos session note|status` (RFC 0151 Phase 1) — the agent session inbox.
//!
//! Writes only to the redacted, capped inbox under `.ekos/session/inbox`; never opens a ledger.

use anyhow::{Result, bail};
use ekos_compiler_core::EkosConfig;
use ekos_session::capture::SliceStore;
use ekos_session::{
    DEFAULT_INBOX_DIR, Inbox, InboxError, InboxLimits, NoteInput, NoteKind, NoteOutcome,
};
use std::path::{Path, PathBuf};

/// Default session id when `--session` is not given: one inbox file per calendar day, so notes
/// from separate sittings do not pile into one unbounded file.
pub fn default_session_id() -> String {
    format!("day-{}", chrono::Utc::now().format("%Y-%m-%d"))
}

fn open_inbox(config: &EkosConfig, cwd: &Path) -> Result<Inbox> {
    let sm = &config.session_memory;
    if !sm.enabled {
        bail!(
            "session memory is disabled. Enable it in ekos.toml:\n\n[session-memory]\nenabled = true"
        );
    }
    let dir = sm
        .inbox_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_INBOX_DIR));
    Ok(Inbox::open(
        cwd,
        &dir,
        InboxLimits {
            max_note_chars: sm.max_note_chars,
            max_entries_per_session: sm.max_entries_per_session,
            max_bytes_per_session: sm.max_bytes_per_session,
        },
    )?)
}

pub fn note(
    config: &EkosConfig,
    cwd: &Path,
    session: Option<String>,
    kind: &str,
    text: String,
    rationale: Option<String>,
    anchors: Vec<String>,
) -> Result<()> {
    let kind: NoteKind = kind.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let inbox = open_inbox(config, cwd)?;
    let session = session.unwrap_or_else(default_session_id);
    let redactor = config.redaction_config();
    match inbox.append(
        &session,
        NoteInput {
            kind,
            text,
            rationale,
            anchors,
        },
        &redactor,
    ) {
        Ok(NoteOutcome::Accepted { redactions_applied }) => {
            println!("recorded note in session `{session}`");
            if redactions_applied {
                println!("  (secret-shaped content was redacted before writing)");
            }
        }
        Ok(NoteOutcome::Duplicate) => {
            println!("note already recorded in session `{session}` (nothing written)")
        }
        Ok(NoteOutcome::DroppedOverCap) => {
            println!("note DROPPED: session `{session}` is over a size or count cap");
        }
        Err(e @ (InboxError::Redaction(_) | InboxError::InvalidNote(_))) => {
            bail!("note not recorded: {e}")
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

pub fn status(config: &EkosConfig, cwd: &Path, session: Option<String>) -> Result<()> {
    let inbox = open_inbox(config, cwd)?;
    let ids = match session {
        Some(s) => vec![s],
        None => inbox.sessions()?,
    };
    if ids.is_empty() {
        println!("no session notes recorded yet");
        return Ok(());
    }
    for id in ids {
        let st = inbox.status(&id)?;
        println!(
            "{}: {} note(s), {} pending commit, {} dropped",
            st.session_id, st.entries, st.pending_commit, st.dropped
        );
    }
    Ok(())
}

fn inbox_and_slices(config: &EkosConfig, cwd: &Path) -> Result<(Inbox, SliceStore)> {
    let inbox = open_inbox(config, cwd)?;
    let slices = SliceStore::new(&inbox);
    Ok((inbox, slices))
}

fn is_lock_error(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        matches!(
            c.downcast_ref::<ekos_ledger::LedgerError>(),
            Some(ekos_ledger::LedgerError::Locked(_))
        ) || c.to_string().contains("cannot write:")
    })
}

/// Opens the writable ledger, retrying with backoff while another writer holds the lock. `None`
/// means the lock stayed busy — the caller leaves notes pending and reports it; a lock error is
/// never surfaced as a failure.
fn open_writable_with_retry(
    config: &EkosConfig,
    cwd: &Path,
) -> Result<Option<Box<dyn ekos_ledger::KnowledgeStore>>> {
    for delay_ms in [0u64, 200, 400, 800, 1600, 3000] {
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        match super::store::open_store(config, cwd) {
            Ok(s) => return Ok(Some(s)),
            Err(e) if is_lock_error(&e) => continue,
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "{e}\nRun `ekos build && ekos recover && ekos resolve && ekos compile && ekos commit` first."
                ));
            }
        }
    }
    Ok(None)
}

pub fn commit(config: &EkosConfig, cwd: &Path, session: Option<String>) -> Result<()> {
    let inbox = open_inbox(config, cwd)?;
    let ids = match session {
        Some(s) => vec![s],
        None => inbox.sessions()?,
    };
    let mut todo = Vec::new();
    for id in ids {
        if inbox.status(&id)?.pending_commit > 0 {
            todo.push(id);
        }
    }
    if todo.is_empty() {
        println!("nothing pending");
        return Ok(());
    }
    let Some(store) = open_writable_with_retry(config, cwd)? else {
        println!(
            "ledger is busy (another writer holds it); {} session(s) left pending — nothing was lost, re-run `ekos session commit` later",
            todo.len()
        );
        return Ok(());
    };
    let run_id = ekos_ledger::provenance::new_run_id();
    let redactor = config.redaction_config();
    for id in todo {
        let r =
            ekos_session::commit::commit_session(store.as_ref(), &inbox, &redactor, &id, &run_id)?;
        println!(
            "session `{}`: committed {} note(s) ({} new claim(s), {} already present); anchors resolved {}, unresolved/ambiguous {}",
            r.session_id,
            r.committed,
            r.claims_written,
            r.claims_already_present,
            r.anchors_resolved,
            r.anchors_unresolved_or_ambiguous
        );
    }
    Ok(())
}

pub fn recall(
    config: &EkosConfig,
    cwd: &Path,
    query: &str,
    limit: usize,
    json: bool,
) -> Result<()> {
    open_inbox(config, cwd)?;
    let store = super::store::open_store_read_only(config, cwd)?;
    let result = ekos_session::read::recall(store.as_ref(), Some(cwd), query, limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }
    match result {
        ekos_session::read::RecallResult::NoRelevantSessionMemory => {
            println!("no relevant session memory")
        }
        ekos_session::read::RecallResult::Hits { hits } => {
            for h in hits {
                let c = h.claim;
                println!(
                    "[{}] [{}] [{}] {}",
                    c.note_kind,
                    c.tier,
                    c.verdict.label(),
                    c.text
                );
                for a in c.anchors.iter().filter_map(|a| a.change_summary.as_ref()) {
                    println!("    anchor {a}");
                }
            }
        }
    }
    Ok(())
}

fn git_scope(cwd: &Path) -> Vec<String> {
    std::process::Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn brief(
    config: &EkosConfig,
    cwd: &Path,
    mut scope: Vec<String>,
    scope_from_git: bool,
    budget: usize,
    format: &str,
) -> Result<()> {
    let inbox = open_inbox(config, cwd)?;
    if scope_from_git {
        scope.extend(git_scope(cwd));
    }
    let mut pending = Vec::new();
    for id in inbox.sessions()? {
        pending.extend(inbox.pending(&id)?.0);
    }
    // A missing/unbuilt ledger must never break a session start: brief from the inbox alone.
    let marker = inbox.dir().join(".last-brief");
    let since = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s.trim()).ok())
        .map(|t| t.with_timezone(&chrono::Utc));
    let claims = match super::store::open_store_read_only(config, cwd) {
        Ok(store) => ekos_session::read::session_claims(store.as_ref(), Some(cwd))?,
        Err(_) => Vec::new(),
    };
    let b = ekos_session::read::brief_since(claims, &pending, &scope, budget, since);
    let _ = std::fs::write(&marker, chrono::Utc::now().to_rfc3339());
    match format {
        "claude-hook" => println!(
            "{}",
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": b.text,
                }
            })
        ),
        "text" => print!("{}", b.text),
        other => bail!("unknown --format `{other}` (want text|claude-hook)"),
    }
    Ok(())
}

pub fn review(
    config: &EkosConfig,
    cwd: &Path,
    claim_id: &str,
    decision: &str,
    by: Option<String>,
) -> Result<()> {
    use ekos_session::lifecycle::{Actor, Decision, review as do_review, supersede};
    open_inbox(config, cwd)?;
    let id: ekos_kir::KirId = claim_id
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid claim id"))?;
    let Some(store) = open_writable_with_retry(config, cwd)? else {
        bail!("ledger is busy; try again shortly");
    };
    match decision {
        "confirm" => do_review(store.as_ref(), &id, Decision::Confirm, Actor::Human)?,
        "reject" => do_review(store.as_ref(), &id, Decision::Reject, Actor::Human)?,
        "supersede" => {
            let by = by.ok_or_else(|| anyhow::anyhow!("`supersede` needs --by <claim id>"))?;
            let new_id = by.parse().map_err(|_| anyhow::anyhow!("invalid --by id"))?;
            supersede(store.as_ref(), &id, &new_id, Actor::Human)?
        }
        other => bail!("unknown decision `{other}` (want confirm|reject|supersede)"),
    }
    println!("recorded: {decision} {claim_id}");
    Ok(())
}

pub fn purge(
    config: &EkosConfig,
    cwd: &Path,
    session: Option<String>,
    older_than_days: Option<u64>,
) -> Result<()> {
    let (inbox, slices) = inbox_and_slices(config, cwd)?;
    let mut targets: Vec<String> = session.into_iter().collect();
    if let Some(days) = older_than_days {
        let cutoff = std::time::Duration::from_secs(days * 86_400);
        for id in inbox.sessions()? {
            if inbox.session_age(&id).is_some_and(|a| a > cutoff) {
                targets.push(id);
            }
        }
        let pruned = slices.prune_older_than(cutoff);
        println!("pruned {pruned} expired slice file(s)");
    }
    if targets.is_empty() && older_than_days.is_none() {
        bail!("give --session <id> and/or --older-than-days <n>");
    }
    for id in targets {
        let inbox_gone = inbox.purge(&id)?;
        let n = slices.purge_session(&id);
        println!(
            "session `{id}`: inbox {}, {n} slice file(s) removed",
            if inbox_gone { "removed" } else { "absent" }
        );
    }
    println!(
        "note: claims already committed to the ledger remain (the ledger is append-only); their evidence now reads \"source purged\""
    );
    Ok(())
}

pub fn capture(
    config: &EkosConfig,
    cwd: &Path,
    session: &str,
    file: Option<PathBuf>,
) -> Result<()> {
    use std::io::Read;
    let (_inbox, slices) = inbox_and_slices(config, cwd)?;
    let text = match file {
        Some(f) => std::fs::read_to_string(&f)?,
        None => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        }
    };
    let sums = slices.capture(session, &text, &config.redaction_config(), 6000)?;
    let pruned = slices.prune_older_than(std::time::Duration::from_secs(
        config.session_memory.capture_retention_days * 86_400,
    ));
    println!(
        "captured {} redacted slice(s); pruned {pruned} expired",
        sums.len()
    );
    Ok(())
}

pub fn extract(config: &EkosConfig, cwd: &Path, session: &str) -> Result<()> {
    if !config.session_memory.extraction {
        bail!(
            "extraction is off. Set [session-memory] extraction = true (uses the [llm] provider; a cloud provider is a metered call)"
        );
    }
    let (inbox, slices) = inbox_and_slices(config, cwd)?;
    let artifact_dir = config.artifact_dir(cwd);
    let llm = super::recover::build_llm_provider(config, &artifact_dir);
    let redactor = config.redaction_config();
    let mut accepted = 0;
    let (mut unsupported, mut invalid) = (0, 0);
    let mut complete = |system: &str, user: &str| -> std::result::Result<String, String> {
        let req = ekos_recovery::llm::LlmRequest {
            system,
            user,
            prompt_version: ekos_session::capture::EXTRACT_PROMPT_VERSION,
            max_tokens: 1500,
            history: &[],
        };
        let call = llm.complete(&req);
        let out = match tokio::runtime::Handle::try_current() {
            Ok(h) => tokio::task::block_in_place(|| h.block_on(call)),
            Err(_) => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?
                .block_on(call),
        };
        out.map(|r| r.content).map_err(|e| e.to_string())
    };
    for sum in slices.list(session) {
        let o = ekos_session::capture::extract_slice(
            &slices,
            &inbox,
            &redactor,
            session,
            &sum,
            &mut complete,
        )?;
        accepted += o.accepted.len();
        unsupported += o.dropped_unsupported;
        invalid += o.dropped_invalid_response;
    }
    println!(
        "extracted {accepted} proposal(s) into the inbox (unconfirmed); dropped {unsupported} unsupported, {invalid} invalid response(s)"
    );
    Ok(())
}

pub fn eval(runs: usize) -> Result<()> {
    let results = ekos_session::eval::run(runs.max(1));
    println!("{}", ekos_session::eval::report_markdown(&results));
    let (go, why) = ekos_session::eval::go_no_go(&results);
    println!("{why}\n(deterministic proxy — see the module docs; not a live-model run)");
    if !go {
        bail!("NO-GO");
    }
    Ok(())
}

pub fn fingerprint_noise(config: &EkosConfig, cwd: &Path) -> Result<()> {
    let store = super::store::open_store_read_only(config, cwd)?;
    let (mut pairs, mut raw_changed, mut fp_changed) = (0usize, 0usize, 0usize);
    let mut by_key: std::collections::BTreeMap<(String, String), usize> = Default::default();
    for o in store.all_objects()? {
        let hist = store.object_history(&o.id)?;
        for w in hist.windows(2) {
            pairs += 1;
            if w[0].properties != w[1].properties {
                raw_changed += 1;
            }
            if ekos_session::anchor::anchor_fingerprint(&w[0])
                != ekos_session::anchor::anchor_fingerprint(&w[1])
            {
                fp_changed += 1;
                let a = ekos_session::anchor::anchor_projection(&w[0]);
                let b = ekos_session::anchor::anchor_projection(&w[1]);
                for k in a.keys().chain(b.keys()) {
                    if a.get(k) != b.get(k) {
                        *by_key.entry((o.kind.to_string(), k.clone())).or_default() += 1;
                    }
                }
            }
        }
    }
    let mut top: Vec<_> = by_key.into_iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("top (kind, key) causes of fingerprint flips:");
    for ((kind, key), n) in top.iter().take(14) {
        println!("  {n:>6}  {kind}.{key}");
    }
    println!("consecutive object-version pairs: {pairs}");
    println!("  any property differs:      {raw_changed}");
    println!("  anchor fingerprint flips:  {fp_changed}");
    if raw_changed > 0 {
        println!(
            "  fingerprint suppresses {:.1}% of raw churn",
            100.0 * (raw_changed - fp_changed.min(raw_changed)) as f64 / raw_changed as f64
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled() -> EkosConfig {
        let mut c = EkosConfig::default();
        c.session_memory.enabled = true;
        c
    }

    #[test]
    fn disabled_by_default_refuses_to_write() {
        let d = tempfile::tempdir().unwrap();
        let err = note(
            &EkosConfig::default(),
            d.path(),
            Some("s1".into()),
            "finding",
            "x".into(),
            None,
            vec![],
        )
        .unwrap_err();
        assert!(err.to_string().contains("disabled"));
        assert!(!d.path().join(DEFAULT_INBOX_DIR).exists());
    }

    #[test]
    fn three_notes_then_status_reports_three_pending() {
        let d = tempfile::tempdir().unwrap();
        let cfg = enabled();
        for t in ["one", "two", "three"] {
            note(
                &cfg,
                d.path(),
                Some("s1".into()),
                "decision",
                t.into(),
                Some("because".into()),
                vec!["orders".into()],
            )
            .unwrap();
        }
        let inbox = open_inbox(&cfg, d.path()).unwrap();
        let st = inbox.status("s1").unwrap();
        assert_eq!((st.entries, st.pending_commit), (3, 3));
        let raw =
            std::fs::read_to_string(d.path().join(DEFAULT_INBOX_DIR).join("s1.jsonl")).unwrap();
        let first: serde_json::Value = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
        assert_eq!(first["schema"], 1);
        assert_eq!(first["kind"], "decision");
        assert_eq!(first["anchors"][0], "orders");
        status(&cfg, d.path(), None).unwrap();
    }

    #[test]
    fn no_mcp_code_can_reach_the_lifecycle_module() {
        let mcp = include_str!("mcp.rs");
        assert!(
            !mcp.contains("ekos_session::lifecycle"),
            "promotion must stay human-only (CLI)"
        );
        assert!(!mcp.contains("Actor::Human"));
    }

    #[test]
    fn unknown_kind_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        assert!(
            note(
                &enabled(),
                d.path(),
                Some("s1".into()),
                "gossip",
                "x".into(),
                None,
                vec![]
            )
            .is_err()
        );
    }
}
