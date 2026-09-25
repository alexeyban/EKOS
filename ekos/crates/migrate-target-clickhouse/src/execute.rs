//! RFC 0160 — guarded execution.
//!
//! The only component in EKOS that writes to a system outside the workspace, so the governing rule
//! is stated once and enforced in code: **nothing executes that was not parsed, classified,
//! approved and hash-matched.**

use crate::classify::{ClassifyError, StatementClass, batch_class};
use serde::{Deserialize, Serialize};

/// Where a statement runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    /// A separate database, never a prefix convention inside a real one — that is one naming
    /// mistake away from disaster.
    Sandbox,
    Staging,
    Production,
}

impl Environment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sandbox => "sandbox",
            Self::Staging => "staging",
            Self::Production => "production",
        }
    }
}

/// A generated statement, identified by the hash of its own text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub sql: String,
    pub hash: String,
}

impl Artifact {
    pub fn new(id: impl Into<String>, sql: impl Into<String>) -> Self {
        let sql = sql.into();
        let hash = content_hash(&sql);
        Self {
            id: id.into(),
            sql,
            hash,
        }
    }

    /// Whether the artifact's text still matches its recorded hash.
    ///
    /// The check exists because an artifact can be regenerated between approval and execution for a
    /// perfectly good reason — and a silently different execution is not a good reason.
    pub fn is_intact(&self) -> bool {
        content_hash(&self.sql) == self.hash
    }
}

fn content_hash(sql: &str) -> String {
    ekos_common::ContentHash::of_str(sql).as_str().to_string()
}

/// A human's decision that one specific artifact may run in one specific environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub artifact_id: String,
    /// The hash as it was when approved. Not the artifact's current hash — that is the whole point.
    pub artifact_hash: String,
    pub environment: Environment,
    pub approver: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExecuteError {
    #[error(transparent)]
    Classify(#[from] ClassifyError),
    #[error(
        "{class} in {environment} needs an approval and has none. Sandbox writes are logged; \
         everything else is gated."
    )]
    NotApproved {
        class: &'static str,
        environment: &'static str,
    },
    #[error(
        "the approval for {artifact_id} is for a different artifact ({approved}), not this one \
         ({actual}). Approve the statement that will run."
    )]
    WrongArtifact {
        artifact_id: String,
        approved: String,
        actual: String,
    },
    #[error(
        "artifact {artifact_id} has changed since it was approved (approved {approved}, now \
         {actual}). Re-approve the new statement — there is deliberately no override, because any \
         such flag becomes the documented workaround within a month."
    )]
    HashMismatch {
        artifact_id: String,
        approved: String,
        actual: String,
    },
    #[error(
        "the approval for {artifact_id} is for {approved_env}, and this is {actual_env}. An \
         approval is for one environment; promotion is a separate decision."
    )]
    WrongEnvironment {
        artifact_id: String,
        approved_env: &'static str,
        actual_env: &'static str,
    },
}

/// Decide whether an artifact may execute. The single gate every write goes through.
///
/// Returns the statement's class so the caller can record it on the execution fact.
pub fn authorize(
    artifact: &Artifact,
    environment: Environment,
    approval: Option<&Approval>,
) -> Result<StatementClass, ExecuteError> {
    // Parse and classify first: an unparseable or unplaceable statement is refused before any
    // question of approval arises.
    let class = batch_class(&artifact.sql)?;

    // A read needs no approval anywhere; a sandbox write is logged rather than gated.
    if !class.writes() || environment == Environment::Sandbox {
        return Ok(class);
    }

    let Some(a) = approval else {
        return Err(ExecuteError::NotApproved {
            class: class.as_str(),
            environment: environment.as_str(),
        });
    };
    if a.artifact_id != artifact.id {
        return Err(ExecuteError::WrongArtifact {
            artifact_id: artifact.id.clone(),
            approved: a.artifact_id.clone(),
            actual: artifact.id.clone(),
        });
    }
    if a.environment != environment {
        return Err(ExecuteError::WrongEnvironment {
            artifact_id: artifact.id.clone(),
            approved_env: a.environment.as_str(),
            actual_env: environment.as_str(),
        });
    }
    // The hash is recomputed from the text about to run, never trusted from the artifact record.
    let actual = content_hash(&artifact.sql);
    if a.artifact_hash != actual {
        return Err(ExecuteError::HashMismatch {
            artifact_id: artifact.id.clone(),
            approved: a.artifact_hash.clone(),
            actual,
        });
    }
    Ok(class)
}

/// One chunk of a load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub index: u32,
    /// Half-open `[lo, hi)`. Recorded explicitly because an off-by-one at a chunk boundary is one
    /// of RFC 0156's planted controls, and the bounds are what make it diagnosable.
    pub lo: i64,
    pub hi: i64,
}

/// Split a key range into half-open chunks.
pub fn plan_chunks(min_key: i64, max_key: i64, chunk_rows: i64) -> Vec<Chunk> {
    if chunk_rows <= 0 || max_key < min_key {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut lo = min_key;
    let mut index = 0;
    while lo <= max_key {
        // `hi` is exclusive, and the last chunk extends past `max_key` by one so the maximum key is
        // included. A closed upper bound here is exactly how a load loses its last row.
        let hi = lo.saturating_add(chunk_rows);
        out.push(Chunk { index, lo, hi });
        lo = hi;
        index += 1;
    }
    out
}

/// The statement that loads one chunk from PostgreSQL into ClickHouse.
///
/// Uses a **named collection** for the source connection. ClickHouse's `postgresql()` also accepts
/// a password positionally, and the classifier refuses that form: a generated statement is hashed,
/// pinned to an approval, printed in a dry run and pasted into tickets, and a production password
/// must not travel that road.
pub fn chunk_insert(
    target_database: &str,
    target_table: &str,
    source_schema: &str,
    source_table: &str,
    named_collection: &str,
    key_column: &str,
    chunk: &Chunk,
) -> String {
    let ident = |s: &str| format!("`{}`", s.replace('`', "\\`"));
    let lit = |s: &str| format!("'{}'", s.replace('\'', "\\'"));
    format!(
        "INSERT INTO {db}.{tbl} SELECT * FROM postgresql({nc}, schema = {schema}, table = {table}) \
         WHERE {key} >= {lo} AND {key} < {hi}",
        db = ident(target_database),
        tbl = ident(target_table),
        nc = named_collection,
        schema = lit(source_schema),
        table = lit(source_table),
        key = ident(key_column),
        lo = chunk.lo,
        hi = chunk.hi,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact() -> Artifact {
        Artifact::new("ART.1", "INSERT INTO db.t SELECT 1")
    }

    fn approval(a: &Artifact, env: Environment) -> Approval {
        Approval {
            artifact_id: a.id.clone(),
            artifact_hash: a.hash.clone(),
            environment: env,
            approver: "human:alex".into(),
        }
    }

    #[test]
    fn a_read_needs_no_approval_anywhere() {
        let a = Artifact::new("R", "SELECT count(*) FROM t");
        assert_eq!(
            authorize(&a, Environment::Production, None).unwrap(),
            StatementClass::Read
        );
    }

    #[test]
    fn a_sandbox_write_is_logged_not_gated() {
        assert_eq!(
            authorize(&artifact(), Environment::Sandbox, None).unwrap(),
            StatementClass::DmlInsert
        );
    }

    #[test]
    fn a_staging_write_without_an_approval_is_refused() {
        assert!(matches!(
            authorize(&artifact(), Environment::Staging, None),
            Err(ExecuteError::NotApproved { .. })
        ));
    }

    /// The gap this closes: an artifact regenerated between approval and execution — for a
    /// perfectly good reason — must not run on the old approval.
    #[test]
    fn an_artifact_changed_since_approval_cannot_run() {
        let original = artifact();
        let approval = approval(&original, Environment::Staging);
        let regenerated = Artifact::new("ART.1", "INSERT INTO db.t SELECT 2");
        let err = authorize(&regenerated, Environment::Staging, Some(&approval)).unwrap_err();
        assert!(matches!(err, ExecuteError::HashMismatch { .. }), "{err:?}");
        assert!(
            err.to_string().contains("no override"),
            "any override flag becomes the documented workaround: {err}"
        );
    }

    #[test]
    fn an_approval_for_one_environment_does_not_carry_to_another() {
        let a = artifact();
        let staging = approval(&a, Environment::Staging);
        assert!(matches!(
            authorize(&a, Environment::Production, Some(&staging)),
            Err(ExecuteError::WrongEnvironment { .. })
        ));
        assert!(authorize(&a, Environment::Staging, Some(&staging)).is_ok());
    }

    #[test]
    fn an_approval_for_a_different_artifact_does_not_carry() {
        let a = artifact();
        let mut other = approval(&a, Environment::Staging);
        other.artifact_id = "ART.OTHER".into();
        assert!(matches!(
            authorize(&a, Environment::Staging, Some(&other)),
            Err(ExecuteError::WrongArtifact { .. })
        ));
    }

    /// Classification happens before approval: an unparseable statement is refused even with a
    /// perfectly valid approval attached to it.
    #[test]
    fn an_unparseable_statement_is_refused_even_when_approved() {
        let a = Artifact::new("BAD", "this is not sql");
        let approval = approval(&a, Environment::Production);
        assert!(matches!(
            authorize(&a, Environment::Production, Some(&approval)),
            Err(ExecuteError::Classify(_))
        ));
    }

    /// And so is a credential-bearing one, in a sandbox, where nothing else is gated.
    #[test]
    fn a_credential_bearing_statement_is_refused_even_in_a_sandbox() {
        let a = Artifact::new(
            "CRED",
            "INSERT INTO db.t SELECT * FROM postgresql('h:5432', 'd', 't', 'u', 'pw')",
        );
        assert!(matches!(
            authorize(&a, Environment::Sandbox, None),
            Err(ExecuteError::Classify(
                ClassifyError::EmbeddedCredential { .. }
            ))
        ));
    }

    #[test]
    fn an_artifacts_hash_tracks_its_text() {
        let a = artifact();
        assert!(a.is_intact());
        let tampered = Artifact {
            sql: "DROP TABLE t".into(),
            ..a
        };
        assert!(!tampered.is_intact());
    }

    // ── chunking ─────────────────────────────────────────────────────────────

    /// Half-open bounds, and the last chunk must include the maximum key. A closed upper bound is
    /// exactly how a load loses its last row — one of RFC 0156's planted controls.
    #[test]
    fn chunks_are_half_open_and_cover_the_whole_range() {
        let chunks = plan_chunks(1, 10, 4);
        assert_eq!(
            chunks,
            vec![
                Chunk {
                    index: 0,
                    lo: 1,
                    hi: 5
                },
                Chunk {
                    index: 1,
                    lo: 5,
                    hi: 9
                },
                Chunk {
                    index: 2,
                    lo: 9,
                    hi: 13
                },
            ]
        );
        // Every key from 1 to 10 falls in exactly one chunk.
        for k in 1..=10 {
            let hits = chunks.iter().filter(|c| k >= c.lo && k < c.hi).count();
            assert_eq!(hits, 1, "key {k} landed in {hits} chunks");
        }
    }

    #[test]
    fn a_single_row_range_produces_one_chunk() {
        let c = plan_chunks(7, 7, 1000);
        assert_eq!(c.len(), 1);
        assert!(7 >= c[0].lo && 7 < c[0].hi);
    }

    #[test]
    fn a_degenerate_range_produces_nothing_rather_than_looping() {
        assert!(plan_chunks(10, 1, 100).is_empty());
        assert!(plan_chunks(1, 10, 0).is_empty());
    }

    #[test]
    fn the_chunk_statement_uses_a_named_collection_and_passes_the_classifier() {
        let sql = chunk_insert(
            "sandbox",
            "orders",
            "public",
            "orders",
            "ekos_migrate_source",
            "id",
            &Chunk {
                index: 0,
                lo: 1,
                hi: 100,
            },
        );
        assert!(sql.contains("postgresql(ekos_migrate_source"), "{sql}");
        assert!(!sql.contains("password"), "{sql}");
        assert_eq!(batch_class(&sql).unwrap(), StatementClass::DmlInsert);
        assert!(sql.contains(">= 1") && sql.contains("< 100"), "{sql}");
    }
}
