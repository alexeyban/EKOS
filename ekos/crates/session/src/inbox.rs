use chrono::{DateTime, Utc};
use ekos_common::ContentHash;
use ekos_common::redaction::{self, RedactionConfig};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_INBOX_DIR: &str = ".ekos/session/inbox";
const SCHEMA_VERSION: u32 = 1;
const MAX_ANCHORS: usize = 16;
const MAX_ANCHOR_CHARS: usize = 256;

#[derive(Debug, thiserror::Error)]
pub enum InboxError {
    #[error("invalid session id `{0}` (want 1-64 chars of A-Z a-z 0-9 _ -)")]
    InvalidSessionId(String),
    #[error("path escapes the workspace: {0}")]
    OutsideWorkspace(String),
    #[error("invalid note: {0}")]
    InvalidNote(String),
    #[error("redaction failed, note dropped: {0}")]
    Redaction(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialisation error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Finding,
    Decision,
    DeadEnd,
    Constraint,
    Todo,
}

impl std::str::FromStr for NoteKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "finding" => Ok(Self::Finding),
            "decision" => Ok(Self::Decision),
            "dead_end" | "dead-end" => Ok(Self::DeadEnd),
            "constraint" => Ok(Self::Constraint),
            "todo" => Ok(Self::Todo),
            other => Err(format!(
                "unknown note kind `{other}` (want finding|decision|dead_end|constraint|todo)"
            )),
        }
    }
}

/// A hint about which real object a note is about. Resolved (or not) later; never trusted here.
pub type Anchor = String;

/// What a caller supplies. Everything else on [`SessionEntry`] is derived.
#[derive(Debug, Clone)]
pub struct NoteInput {
    pub kind: NoteKind,
    pub text: String,
    pub rationale: Option<String>,
    pub anchors: Vec<Anchor>,
}

/// One inbox line. `entry_id` is a content hash, so re-sending the same note is idempotent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEntry {
    pub schema: u32,
    pub entry_id: String,
    pub session_id: String,
    pub recorded_at: DateTime<Utc>,
    pub kind: NoteKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default)]
    pub anchors: Vec<Anchor>,
    pub capture: String,
    /// For `capture: "extracted"` entries, the verbatim transcript span the claim rests on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteOutcome {
    Accepted { redactions_applied: bool },
    Duplicate,
    DroppedOverCap,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionStatus {
    pub session_id: String,
    pub entries: usize,
    pub dropped: usize,
    pub pending_commit: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct InboxLimits {
    pub max_note_chars: usize,
    pub max_entries_per_session: usize,
    pub max_bytes_per_session: u64,
}

impl Default for InboxLimits {
    fn default() -> Self {
        Self {
            max_note_chars: 2000,
            max_entries_per_session: 200,
            max_bytes_per_session: 256 * 1024,
        }
    }
}

/// The single redaction seam. Fallible so a broken redactor drops the note instead of letting
/// unredacted text through.
pub trait Redactor {
    fn redact(&self, text: &str) -> Result<String, String>;
}

impl Redactor for RedactionConfig {
    fn redact(&self, text: &str) -> Result<String, String> {
        Ok(redaction::redact(text, self))
    }
}

pub struct Inbox {
    dir: PathBuf,
    workspace: PathBuf,
    limits: InboxLimits,
}

impl Inbox {
    /// `inbox_dir` is resolved against `workspace` when relative. The workspace and inbox
    /// directory are created if missing, then canonicalised; an inbox that resolves outside the
    /// workspace (`..`, an absolute path elsewhere, a symlink out) is refused.
    pub fn open(
        workspace: &Path,
        inbox_dir: &Path,
        limits: InboxLimits,
    ) -> Result<Self, InboxError> {
        let workspace = workspace.canonicalize()?;
        let joined = if inbox_dir.is_absolute() {
            inbox_dir.to_path_buf()
        } else {
            workspace.join(inbox_dir)
        };
        if joined.components().any(|c| c == Component::ParentDir) {
            return Err(InboxError::OutsideWorkspace(joined.display().to_string()));
        }
        create_private_dir(&joined)?;
        let dir = joined.canonicalize()?;
        if !dir.starts_with(&workspace) {
            return Err(InboxError::OutsideWorkspace(dir.display().to_string()));
        }
        Ok(Self {
            dir,
            workspace,
            limits,
        })
    }

    fn entries_path(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{session_id}.jsonl"))
    }

    fn dropped_path(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{session_id}.dropped"))
    }

    fn lock_path(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{session_id}.lock"))
    }

    fn committed_path(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{session_id}.committed"))
    }

    /// Redacts, validates, caps and appends one explicit note. Nothing is written on any error.
    pub fn append(
        &self,
        session_id: &str,
        input: NoteInput,
        redactor: &dyn Redactor,
    ) -> Result<NoteOutcome, InboxError> {
        self.append_with(session_id, input, redactor, "explicit", None)
    }

    /// [`Self::append`] with an explicit capture provenance (`"explicit"` or `"extracted"`) and,
    /// for extracted claims, the quoted transcript span.
    pub fn append_with(
        &self,
        session_id: &str,
        input: NoteInput,
        redactor: &dyn Redactor,
        capture: &str,
        quote: Option<String>,
    ) -> Result<NoteOutcome, InboxError> {
        validate_session_id(session_id)?;
        if input.text.trim().is_empty() {
            return Err(InboxError::InvalidNote("text is empty".into()));
        }

        let mut redactions = false;
        let mut clean = |field: &str| -> Result<String, InboxError> {
            let out = redactor.redact(field).map_err(InboxError::Redaction)?;
            redactions |= out != field;
            Ok(out)
        };
        let text = clean(&input.text)?;
        let rationale = input.rationale.as_deref().map(&mut clean).transpose()?;
        if input.anchors.len() > MAX_ANCHORS {
            return Err(InboxError::InvalidNote(format!(
                "more than {MAX_ANCHORS} anchors"
            )));
        }
        let mut anchors = Vec::with_capacity(input.anchors.len());
        for a in &input.anchors {
            self.check_anchor(a)?;
            anchors.push(clean(a)?);
        }

        if only_redaction_markers(&text) {
            return Err(InboxError::Redaction(
                "note is nothing but redacted content".into(),
            ));
        }
        let over_length = text.chars().count() > self.limits.max_note_chars
            || rationale
                .as_deref()
                .is_some_and(|r| r.chars().count() > self.limits.max_note_chars);

        let id_material =
            serde_json::to_string(&(session_id, input.kind, &text, &rationale, &anchors))?;
        let entry_id = ContentHash::of_str(&id_material).0;

        let path = self.entries_path(session_id);
        // Serialise check-then-append per session (across threads and processes): without it a
        // reader can see another writer's half-written line, decide the file "ends without a
        // newline", and tear it (found by the concurrent-writers test).
        let _lock = acquire_lock(&self.lock_path(session_id))?;
        let existing = read_entries(&path)?;
        if existing.iter().any(|e| e.entry_id == entry_id) {
            return Ok(NoteOutcome::Duplicate);
        }

        let entry = SessionEntry {
            schema: SCHEMA_VERSION,
            entry_id,
            session_id: session_id.to_string(),
            recorded_at: Utc::now(),
            kind: input.kind,
            text,
            rationale,
            anchors,
            capture: capture.to_string(),
            quote,
        };
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');

        let current_bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if over_length
            || existing.len() >= self.limits.max_entries_per_session
            || current_bytes + line.len() as u64 > self.limits.max_bytes_per_session
        {
            self.bump_dropped(session_id)?;
            return Ok(NoteOutcome::DroppedOverCap);
        }

        // A crash mid-append can leave a line without a trailing newline; start a fresh line so
        // the new entry is never glued onto the torn one.
        let needs_newline = ends_without_newline(&path);
        let mut f = open_private_append(&path)?;
        if needs_newline {
            f.write_all(b"\n")?;
        }
        f.write_all(line.as_bytes())?;
        f.flush()?;
        Ok(NoteOutcome::Accepted {
            redactions_applied: redactions,
        })
    }

    pub fn entries(&self, session_id: &str) -> Result<Vec<SessionEntry>, InboxError> {
        validate_session_id(session_id)?;
        read_entries(&self.entries_path(session_id))
    }

    pub fn status(&self, session_id: &str) -> Result<SessionStatus, InboxError> {
        validate_session_id(session_id)?;
        let entries = read_entries(&self.entries_path(session_id))?.len();
        let committed = read_count(&self.committed_path(session_id));
        Ok(SessionStatus {
            session_id: session_id.to_string(),
            entries,
            dropped: read_count(&self.dropped_path(session_id)),
            pending_commit: entries.saturating_sub(committed),
        })
    }

    /// Path of a session's entries file (evidence points back at it).
    pub fn session_file(&self, session_id: &str) -> PathBuf {
        self.entries_path(session_id)
    }

    /// The workspace-relative form of [`Self::session_file`], for evidence locations.
    pub fn session_file_rel(&self, session_id: &str) -> String {
        self.entries_path(session_id)
            .strip_prefix(&self.workspace)
            .unwrap_or(&self.entries_path(session_id))
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Entries not yet committed to the ledger, plus the total entry count.
    pub fn pending(&self, session_id: &str) -> Result<(Vec<SessionEntry>, usize), InboxError> {
        validate_session_id(session_id)?;
        let all = read_entries(&self.entries_path(session_id))?;
        let committed = read_count(&self.committed_path(session_id)).min(all.len());
        let total = all.len();
        Ok((all[committed..].to_vec(), total))
    }

    /// Records that the first `total` entries are now in the ledger.
    pub fn mark_committed(&self, session_id: &str, total: usize) -> Result<(), InboxError> {
        validate_session_id(session_id)?;
        let mut f = open_private_truncate(&self.committed_path(session_id))?;
        f.write_all(total.to_string().as_bytes())?;
        Ok(())
    }

    /// Deletes a session's inbox files. Ledger claims already committed are NOT undone.
    pub fn purge(&self, session_id: &str) -> Result<bool, InboxError> {
        validate_session_id(session_id)?;
        let mut removed = false;
        for p in [
            self.entries_path(session_id),
            self.dropped_path(session_id),
            self.committed_path(session_id),
        ] {
            match fs::remove_file(&p) {
                Ok(()) => removed = true,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(removed)
    }

    /// Age of a session's entries file, if present.
    pub fn session_age(&self, session_id: &str) -> Option<std::time::Duration> {
        fs::metadata(self.entries_path(session_id))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
    }

    /// The directory holding the inbox files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The canonical workspace root.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Session ids that have an inbox file, sorted.
    pub fn sessions(&self) -> Result<Vec<String>, InboxError> {
        let mut ids: Vec<String> = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".jsonl").map(str::to_string)
            })
            .collect();
        ids.sort();
        Ok(ids)
    }

    fn bump_dropped(&self, session_id: &str) -> Result<(), InboxError> {
        let path = self.dropped_path(session_id);
        let next = read_count(&path) + 1;
        let mut f = open_private_truncate(&path)?;
        f.write_all(next.to_string().as_bytes())?;
        Ok(())
    }

    fn check_anchor(&self, anchor: &str) -> Result<(), InboxError> {
        if anchor.is_empty() || anchor.chars().count() > MAX_ANCHOR_CHARS {
            return Err(InboxError::InvalidNote(format!(
                "anchor must be 1-{MAX_ANCHOR_CHARS} chars"
            )));
        }
        let p = Path::new(anchor);
        if p.components().any(|c| c == Component::ParentDir) {
            return Err(InboxError::OutsideWorkspace(anchor.to_string()));
        }
        if p.is_absolute() {
            let resolved = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
            if !resolved.starts_with(&self.workspace) {
                return Err(InboxError::OutsideWorkspace(anchor.to_string()));
            }
        } else if let Ok(resolved) = self.workspace.join(p).canonicalize() {
            // An existing relative path that resolves (through a symlink) outside the workspace.
            if !resolved.starts_with(&self.workspace) {
                return Err(InboxError::OutsideWorkspace(anchor.to_string()));
            }
        }
        Ok(())
    }
}

struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// A `create_new` lock file. A lock older than 10s belongs to a crashed writer and is reclaimed.
fn acquire_lock(path: &Path) -> Result<LockGuard, InboxError> {
    for _ in 0..5000 {
        match private_options().create_new(true).write(true).open(path) {
            Ok(_) => return Ok(LockGuard(path.to_path_buf())),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = fs::metadata(path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age > std::time::Duration::from_secs(10));
                if stale {
                    let _ = fs::remove_file(path);
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(InboxError::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "session inbox lock is busy",
    )))
}

fn validate_session_id(id: &str) -> Result<(), InboxError> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(InboxError::InvalidSessionId(id.to_string()))
    }
}

fn only_redaction_markers(text: &str) -> bool {
    let mut rest = text.trim();
    if rest.is_empty() {
        return true;
    }
    while let Some(start) = rest.find("[REDACTED:") {
        if !rest[..start].trim().is_empty() {
            return false;
        }
        match rest[start..].find(']') {
            Some(end) => rest = rest[start + end + 1..].trim_start(),
            None => return false,
        }
    }
    rest.is_empty()
}

fn read_count(path: &Path) -> usize {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Skips blank and undecodable lines, so a torn final line from a crash never poisons the rest.
fn read_entries(path: &Path) -> Result<Vec<SessionEntry>, InboxError> {
    let f = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<SessionEntry>(&l).ok())
        .collect())
}

fn ends_without_newline(path: &Path) -> bool {
    match fs::read(path) {
        Ok(bytes) => !bytes.is_empty() && bytes.last() != Some(&b'\n'),
        Err(_) => false,
    }
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}

fn private_options() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

fn open_private_append(path: &Path) -> std::io::Result<fs::File> {
    private_options().append(true).open(path)
}

fn open_private_truncate(path: &Path) -> std::io::Result<fs::File> {
    private_options().write(true).truncate(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn note(text: &str) -> NoteInput {
        NoteInput {
            kind: NoteKind::Finding,
            text: text.into(),
            rationale: None,
            anchors: vec![],
        }
    }

    fn inbox(dir: &TempDir, limits: InboxLimits) -> Inbox {
        Inbox::open(dir.path(), Path::new(DEFAULT_INBOX_DIR), limits).unwrap()
    }

    struct Failing;
    impl Redactor for Failing {
        fn redact(&self, _: &str) -> Result<String, String> {
            Err("boom".into())
        }
    }

    fn cfg() -> RedactionConfig {
        RedactionConfig::default()
    }

    #[test]
    fn accepted_note_is_written_and_status_counts_it() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        for t in ["a", "b", "c"] {
            assert!(matches!(
                ib.append("s1", note(t), &cfg()).unwrap(),
                NoteOutcome::Accepted { .. }
            ));
        }
        let st = ib.status("s1").unwrap();
        assert_eq!((st.entries, st.pending_commit, st.dropped), (3, 3, 0));
        let e = &ib.entries("s1").unwrap()[0];
        assert_eq!(e.schema, 1);
        assert_eq!(e.capture, "explicit");
    }

    #[test]
    fn entry_id_is_deterministic_and_resend_is_idempotent() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        ib.append("s1", note("same"), &cfg()).unwrap();
        assert_eq!(
            ib.append("s1", note("same"), &cfg()).unwrap(),
            NoteOutcome::Duplicate
        );
        let d2 = TempDir::new().unwrap();
        let ib2 = inbox(&d2, InboxLimits::default());
        ib2.append("s1", note("same"), &cfg()).unwrap();
        assert_eq!(
            ib.entries("s1").unwrap()[0].entry_id,
            ib2.entries("s1").unwrap()[0].entry_id
        );
        assert_eq!(ib.status("s1").unwrap().entries, 1);
    }

    #[test]
    fn entry_count_cap_drops_and_counts() {
        let d = TempDir::new().unwrap();
        let ib = inbox(
            &d,
            InboxLimits {
                max_entries_per_session: 2,
                ..InboxLimits::default()
            },
        );
        ib.append("s1", note("1"), &cfg()).unwrap();
        ib.append("s1", note("2"), &cfg()).unwrap();
        assert_eq!(
            ib.append("s1", note("3"), &cfg()).unwrap(),
            NoteOutcome::DroppedOverCap
        );
        ib.append("s1", note("4"), &cfg()).unwrap();
        let st = ib.status("s1").unwrap();
        assert_eq!((st.entries, st.dropped), (2, 2));
    }

    #[test]
    fn oversize_note_and_byte_cap_are_dropped() {
        let d = TempDir::new().unwrap();
        let ib = inbox(
            &d,
            InboxLimits {
                max_note_chars: 10,
                max_bytes_per_session: 400,
                ..InboxLimits::default()
            },
        );
        assert_eq!(
            ib.append("s1", note(&"x".repeat(11)), &cfg()).unwrap(),
            NoteOutcome::DroppedOverCap
        );
        ib.append("s1", note("ok"), &cfg()).unwrap();
        assert_eq!(
            ib.append("s1", note("also"), &cfg()).unwrap(),
            NoteOutcome::DroppedOverCap
        );
        assert_eq!(ib.status("s1").unwrap().dropped, 2);
    }

    #[test]
    fn truncated_last_line_is_tolerated() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        ib.append("s1", note("first"), &cfg()).unwrap();
        let path = ib.entries_path("s1");
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{\"schema\":1,\"entry_id\":\"torn").unwrap();
        assert_eq!(ib.entries("s1").unwrap().len(), 1);
        ib.append("s1", note("second"), &cfg()).unwrap();
        let texts: Vec<_> = ib
            .entries("s1")
            .unwrap()
            .into_iter()
            .map(|e| e.text)
            .collect();
        assert_eq!(texts, ["first", "second"]);
    }

    #[test]
    fn secrets_are_redacted_before_the_write() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        let secret = "AKIAIOSFODNN7EXAMPLE";
        let out = ib
            .append(
                "s1",
                NoteInput {
                    rationale: Some(format!("found key {secret}")),
                    ..note(&format!("the aws key is {secret} in config"))
                },
                &cfg(),
            )
            .unwrap();
        assert_eq!(
            out,
            NoteOutcome::Accepted {
                redactions_applied: true
            }
        );
        let raw = fs::read_to_string(ib.entries_path("s1")).unwrap();
        assert!(!raw.contains(secret), "secret reached disk: {raw}");
        assert!(raw.contains("[REDACTED:"));
    }

    #[test]
    fn failing_redactor_writes_nothing() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        assert!(matches!(
            ib.append("s1", note("anything"), &Failing),
            Err(InboxError::Redaction(_))
        ));
        assert!(!ib.entries_path("s1").exists());
    }

    #[test]
    fn note_that_is_only_a_secret_is_dropped() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        assert!(
            ib.append("s1", note("AKIAIOSFODNN7EXAMPLE"), &cfg())
                .is_err()
        );
        assert!(!ib.entries_path("s1").exists());
    }

    #[test]
    fn session_id_traversal_is_rejected() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        for bad in ["", "../x", "a/b", "a.b", &"x".repeat(65)] {
            assert!(matches!(
                ib.append(bad, note("t"), &cfg()),
                Err(InboxError::InvalidSessionId(_))
            ));
        }
    }

    #[test]
    fn anchor_traversal_is_rejected() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        for bad in ["../secret.txt", "src/../../etc/passwd", "/etc/passwd"] {
            let r = ib.append(
                "s1",
                NoteInput {
                    anchors: vec![bad.into()],
                    ..note("t")
                },
                &cfg(),
            );
            assert!(matches!(r, Err(InboxError::OutsideWorkspace(_))), "{bad}");
        }
        assert!(
            ib.append(
                "s1",
                NoteInput {
                    anchors: vec!["orders".into(), "src/lib.rs".into()],
                    ..note("t")
                },
                &cfg()
            )
            .is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_anchor_and_inbox_escaping_the_workspace_are_rejected() {
        use std::os::unix::fs::symlink;
        let d = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("f.txt"), "x").unwrap();
        symlink(outside.path(), d.path().join("link")).unwrap();
        let ib = inbox(&d, InboxLimits::default());
        let r = ib.append(
            "s1",
            NoteInput {
                anchors: vec!["link/f.txt".into()],
                ..note("t")
            },
            &cfg(),
        );
        assert!(matches!(r, Err(InboxError::OutsideWorkspace(_))));
        assert!(matches!(
            Inbox::open(d.path(), Path::new("link/inbox"), InboxLimits::default()),
            Err(InboxError::OutsideWorkspace(_))
        ));
    }

    #[test]
    fn inbox_dir_with_parent_component_is_rejected() {
        let d = TempDir::new().unwrap();
        assert!(matches!(
            Inbox::open(d.path(), Path::new("../elsewhere"), InboxLimits::default()),
            Err(InboxError::OutsideWorkspace(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn inbox_files_and_dir_are_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        ib.append("s1", note("t"), &cfg()).unwrap();
        let file = fs::metadata(ib.entries_path("s1")).unwrap();
        let dir = fs::metadata(&ib.dir).unwrap();
        assert_eq!(file.permissions().mode() & 0o777, 0o600);
        assert_eq!(dir.permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn empty_note_is_rejected() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        assert!(matches!(
            ib.append("s1", note("   "), &cfg()),
            Err(InboxError::InvalidNote(_))
        ));
    }

    #[test]
    fn sessions_lists_only_inbox_files() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        ib.append("b", note("1"), &cfg()).unwrap();
        ib.append("a", note("1"), &cfg()).unwrap();
        assert_eq!(ib.sessions().unwrap(), ["a", "b"]);
    }

    #[test]
    fn pending_tracks_committed_and_purge_removes_files() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        ib.append("s1", note("a"), &cfg()).unwrap();
        ib.append("s1", note("b"), &cfg()).unwrap();
        let (pending, total) = ib.pending("s1").unwrap();
        assert_eq!((pending.len(), total), (2, 2));
        ib.mark_committed("s1", 2).unwrap();
        ib.append("s1", note("c"), &cfg()).unwrap();
        let (pending, total) = ib.pending("s1").unwrap();
        assert_eq!((pending.len(), total), (1, 3));
        assert_eq!(ib.status("s1").unwrap().pending_commit, 1);
        assert!(ib.purge("s1").unwrap());
        assert!(ib.entries("s1").unwrap().is_empty());
        assert!(!ib.purge("s1").unwrap());
    }

    #[test]
    fn concurrent_sessions_and_writers_never_tear_lines() {
        let d = TempDir::new().unwrap();
        let ib = std::sync::Arc::new(inbox(&d, InboxLimits::default()));
        let mut hs = vec![];
        for t in 0..4 {
            let ib = ib.clone();
            hs.push(std::thread::spawn(move || {
                for i in 0..25 {
                    ib.append("shared", note(&format!("t{t}-n{i}")), &cfg())
                        .unwrap();
                }
            }));
        }
        for h in hs {
            h.join().unwrap();
        }
        let raw = fs::read_to_string(ib.entries_path("shared")).unwrap();
        let lines: Vec<_> = raw.lines().collect();
        assert!(
            lines
                .iter()
                .all(|l| serde_json::from_str::<SessionEntry>(l).is_ok())
        );
        assert_eq!(
            lines.len(),
            100,
            "every distinct note must land exactly once"
        );
    }

    #[test]
    fn a_crashed_writers_stale_lock_is_reclaimed() {
        let d = TempDir::new().unwrap();
        let ib = inbox(&d, InboxLimits::default());
        let lock = ib.lock_path("s1");
        fs::write(&lock, "").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(matches!(
            ib.append("s1", note("after a crash"), &cfg()).unwrap(),
            NoteOutcome::Accepted { .. }
        ));
        assert!(!lock.exists(), "lock is released after the append");
    }
}
