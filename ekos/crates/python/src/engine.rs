//! The read engine behind the Python binding, with no Python in it: every function here is
//! plain Rust returning `serde_json::Value`, so `cargo test` covers it without an interpreter.
//!
//! Three rules (RFC 0269):
//! - a store is opened **read-only** and is **never created** — a path with no compiled ledger
//!   is an error, unlike the CLI's read-only open, which bootstraps an empty store;
//! - every read goes through [`Runtime`], which hides session memory;
//! - one handle serves one call at a time (`KnowledgeReader` is `Send`, not `Sync`).

use ekos_compiler_core::{EkosConfig, ProjectLocation};
use ekos_kir::KirId;
use ekos_ledger::{FactLedger, KnowledgeStore, RetrievalRequest};
use ekos_runtime::Runtime;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What can go wrong, by the Python exception it becomes.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The path, its config or its store cannot be opened or read.
    #[error("{0}")]
    Ledger(String),
    /// The call is well-formed but this build or this kind of store cannot serve it.
    #[error("{0}")]
    NotSupported(String),
    /// The caller's argument is wrong (an id that is not an id, an unknown mode).
    #[error("{0}")]
    Query(String),
}

type Result<T> = std::result::Result<T, EngineError>;

fn ledger_err(e: impl std::fmt::Display) -> EngineError {
    EngineError::Ledger(e.to_string())
}

/// An open, read-only knowledge store.
pub struct Engine {
    store: Mutex<Box<dyn KnowledgeStore>>,
    location: String,
}

/// The config and state directory a path names: a home project
/// (`<home>/data/<tenant>/<project>` holding `ekos.toml` and `state/`) or an in-tree workspace
/// (`.ekos/` beside an optional `ekos.toml`).
fn resolve(path: &Path) -> Result<(EkosConfig, PathBuf)> {
    if !path.is_dir() {
        return Err(EngineError::Ledger(format!(
            "{} is not a directory",
            path.display()
        )));
    }
    let config_path = path.join("ekos.toml");
    let mut config = if config_path.is_file() {
        EkosConfig::from_file(&config_path).map_err(|e| ledger_err(format!("{e:#}")))?
    } else {
        EkosConfig::default()
    };
    if !path.join(".ekos").exists() && path.join("state").is_dir() && config_path.is_file() {
        config.location = Some(home_location(path)?);
    }
    Ok((config, path.to_path_buf()))
}

fn home_location(path: &Path) -> Result<ProjectLocation> {
    let name = |p: &Path| {
        p.file_name()
            .and_then(std::ffi::OsStr::to_str)
            .map(str::to_string)
            .ok_or_else(|| {
                EngineError::Ledger(format!("{} is not a project directory", path.display()))
            })
    };
    let tenant_dir = path.parent().unwrap_or(path);
    let home = tenant_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| ledger_err(format!("{} is not inside an EKOS home", path.display())))?;
    Ok(ProjectLocation {
        home: home.to_path_buf(),
        tenant: name(tenant_dir)?,
        project: name(path)?,
    })
}

impl Engine {
    /// Open the ledger of the workspace or home project at `path`, read-only.
    pub fn open(path: &Path) -> Result<Self> {
        let (config, cwd) = resolve(path)?;
        if config.storage.distributed.is_enabled() {
            return Err(EngineError::NotSupported(
                "this project reads from a distributed cluster; use ekos.connect(...)".into(),
            ));
        }
        let ledger_dir = config.ledger_dir(&cwd);
        if ledger_dir.join("partitioned").join("catalog.json").exists() {
            return Err(EngineError::NotSupported(
                "partitioned ledgers are not served by the spike build".into(),
            ));
        }
        let facts = ledger_dir.join("facts");
        if facts.join("manifest.json").exists() {
            let store = FactLedger::open_read_only(&facts)
                .map_err(|e| ledger_err(format!("cannot open {}: {e}", facts.display())))?;
            return Ok(Self {
                store: Mutex::new(Box::new(store)),
                location: facts.display().to_string(),
            });
        }
        if config.ledger_path(&cwd).exists() {
            return Err(EngineError::NotSupported(
                "SQLite ledgers are not served by the spike build (their open is writable)".into(),
            ));
        }
        Err(EngineError::Ledger(format!(
            "no compiled ledger under {} — run the EKOS pipeline first",
            ledger_dir.display()
        )))
    }

    fn with<T>(&self, f: impl FnOnce(&dyn KnowledgeStore, &Runtime<'_>) -> Result<T>) -> Result<T> {
        let guard = self
            .store
            .lock()
            .map_err(|_| ledger_err("an earlier call panicked while holding this ledger"))?;
        let store: &dyn KnowledgeStore = guard.as_ref();
        f(store, &Runtime::over(store))
    }

    /// Counts and where the store is — the `ekos_status` tool's answer.
    pub fn status(&self) -> Result<Value> {
        self.with(|store, _| {
            Ok(json!({
                "entries": store.entry_count().map_err(ledger_err)?,
                "objects": store.object_count().map_err(ledger_err)?,
                "relationships": store.relationship_count().map_err(ledger_err)?,
                "ledger_path": self.location,
            }))
        })
    }

    /// Fold in what another process has committed since this handle was opened (or last
    /// refreshed). Reads never do this on their own: a handle is a snapshot. `true` when
    /// something changed.
    pub fn refresh(&self) -> Result<bool> {
        self.with(|store, _| {
            let outcome = store.refresh_snapshot().map_err(ledger_err)?;
            Ok(outcome != ekos_ledger::RefreshOutcome::Unchanged)
        })
    }

    /// Lexical search, best first — the `matches` of the `ekos_search` tool.
    pub fn search(&self, query: &str, mode: &str, limit: usize) -> Result<Value> {
        if mode != "lexical" {
            return Err(EngineError::NotSupported(format!(
                "search mode {mode:?} is not served by the spike build (lexical only)"
            )));
        }
        let limit = limit.clamp(1, 100);
        let mut req = RetrievalRequest::lexical(query);
        req.limit = limit;
        self.with(|_, runtime| {
            let result = runtime.retrieve(&req).map_err(ledger_err)?;
            Ok(Value::Array(
                result
                    .hits
                    .iter()
                    .take(limit)
                    .map(|h| {
                        json!({
                            "id": h.id.to_string(),
                            "name": h.name,
                            "kind": h.kind.as_ref().map(|k| json!(k)),
                            "score": h.score,
                        })
                    })
                    .collect(),
            ))
        })
    }

    /// One object with its relationships and evidence, or `null` when there is none.
    pub fn get(&self, id: &str) -> Result<Value> {
        let id: KirId = id
            .parse()
            .map_err(|e| EngineError::Query(format!("{id:?} is not an object id: {e}")))?;
        self.with(|_, runtime| {
            let state = runtime.reconstruct_state(&id).map_err(ledger_err)?;
            serde_json::to_value(state).map_err(ledger_err)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_with_no_ledger_is_an_error_and_stays_empty() {
        let dir = tempfile::tempdir().unwrap();
        let err = Engine::open(dir.path()).err().expect("no ledger to open");
        assert!(matches!(err, EngineError::Ledger(_)), "{err:?}");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "an open must never create a store"
        );
    }

    /// The spike's concurrency question: a reader stays open while a writer in the same
    /// directory appends. The writer is never blocked, the reader keeps its snapshot, and
    /// `refresh` brings it up to date.
    #[test]
    fn a_reader_is_a_snapshot_that_a_concurrent_writer_neither_blocks_nor_breaks() {
        use ekos_kir::{KirObject, ObjectKind};
        let dir = tempfile::tempdir().unwrap();
        let facts = dir.path().join(".ekos").join("ledger").join("facts");
        let writer = FactLedger::open(&facts).unwrap();
        writer
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        writer.find_objects("orders").unwrap(); // commits the search index

        let reader = Engine::open(dir.path()).unwrap();
        assert_eq!(reader.status().unwrap()["objects"], 1);

        writer
            .append_object(&KirObject::new("invoices", ObjectKind::Table))
            .expect("an open reader must not block a writer");
        writer.find_objects("invoices").unwrap();

        assert_eq!(reader.status().unwrap()["objects"], 1, "still the snapshot");
        assert!(reader.refresh().unwrap());
        assert_eq!(reader.status().unwrap()["objects"], 2);
        let hits = reader.search("invoices", "lexical", 5).unwrap();
        assert_eq!(hits[0]["name"], "invoices");
        assert!(!reader.refresh().unwrap(), "nothing new the second time");
    }

    #[test]
    fn get_returns_the_object_with_its_evidence_and_refuses_what_is_not_an_id() {
        use ekos_kir::{KirObject, ObjectKind};
        let dir = tempfile::tempdir().unwrap();
        let facts = dir.path().join(".ekos").join("ledger").join("facts");
        let table = KirObject::new("orders", ObjectKind::Table);
        FactLedger::open(&facts)
            .unwrap()
            .append_object(&table)
            .unwrap();

        let reader = Engine::open(dir.path()).unwrap();
        let state = reader.get(&table.id.to_string()).unwrap();
        assert_eq!(state["object"]["name"], "orders");
        assert!(state["evidence"].is_array() && state["relationships"].is_array());
        assert!(reader.get(&KirId::new().to_string()).unwrap().is_null());
        assert!(matches!(reader.get("orders"), Err(EngineError::Query(_))));
        assert!(matches!(
            reader.search("orders", "hybrid", 5),
            Err(EngineError::NotSupported(_))
        ));
    }

    #[test]
    fn stores_the_engine_cannot_read_without_writing_are_refused_by_name() {
        let sqlite = tempfile::tempdir().unwrap();
        let ledger = sqlite.path().join(".ekos").join("ledger");
        std::fs::create_dir_all(&ledger).unwrap();
        std::fs::write(ledger.join("ledger.db"), b"").unwrap();
        let err = Engine::open(sqlite.path())
            .err()
            .expect("SQLite is refused");
        assert!(
            matches!(&err, EngineError::NotSupported(m) if m.contains("SQLite")),
            "{err:?}"
        );

        let cluster = tempfile::tempdir().unwrap();
        std::fs::write(
            cluster.path().join("ekos.toml"),
            "[storage.distributed]\ncoordinator = \"127.0.0.1:1\"\nquery-workers = [\"127.0.0.1:2\"]\n",
        )
        .unwrap();
        let err = Engine::open(cluster.path())
            .err()
            .expect("a cluster is refused");
        assert!(
            matches!(&err, EngineError::NotSupported(m) if m.contains("connect")),
            "{err:?}"
        );

        let broken = tempfile::tempdir().unwrap();
        std::fs::write(broken.path().join("ekos.toml"), "[workspace\n").unwrap();
        assert!(matches!(
            Engine::open(broken.path()),
            Err(EngineError::Ledger(_))
        ));
        assert!(matches!(
            Engine::open(&broken.path().join("missing")),
            Err(EngineError::Ledger(_))
        ));
    }

    #[test]
    fn a_home_project_path_names_its_tenant_and_project() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("data").join("acme").join("erp");
        std::fs::create_dir_all(project.join("state")).unwrap();
        std::fs::write(project.join("ekos.toml"), "").unwrap();
        let (config, _) = resolve(&project).unwrap();
        let location = config.location.clone().expect("a home project");
        assert_eq!(
            (location.tenant.as_str(), location.project.as_str()),
            ("acme", "erp")
        );
        assert_eq!(config.ekos_dir(&project), project.join("state"));
    }
}
