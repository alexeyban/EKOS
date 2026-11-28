//! The read engine behind the Python binding, with no Python in it: every function here is
//! plain Rust returning `serde_json::Value`, so `cargo test` covers it without an interpreter.
//!
//! Rules (RFC 0269):
//! - a store is opened **read-only** and is **never created** — a path with no compiled ledger
//!   is an error, unlike the CLI's read-only open, which bootstraps an empty store;
//! - every read is a function of `ekos-read`, the same code the MCP server runs, over
//!   [`Runtime`], which hides session memory;
//! - one handle serves one call at a time (`KnowledgeReader` is `Send`, not `Sync`);
//! - nothing here writes: no append, no writable open, no review entry point (a test scans).

use ekos_compiler_core::{EkosConfig, ProjectLocation};
use ekos_kir::KirId;
use ekos_ledger::scope::{ReadScope, ScopedStore};
use ekos_ledger::{FactLedger, KnowledgeStore};
use ekos_read::{ReadError, tools};
use ekos_runtime::Runtime;
use ekos_runtime::store::{Backend, detect, open_partitioned};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What can go wrong, by the Python exception it becomes.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The path, its config or its store cannot be opened or read.
    #[error("{0}")]
    Ledger(String),
    /// The call is well-formed but this kind of store, or this reader's scope, cannot serve it.
    #[error("{0}")]
    NotSupported(String),
    /// The caller's argument is wrong (an id that is not an id, a query that does not parse).
    #[error("{0}")]
    Query(String),
    /// The argument is well-formed and names nothing in the ledger.
    #[error("{0}")]
    NotFound(String),
}

impl From<ReadError> for EngineError {
    fn from(e: ReadError) -> Self {
        match e {
            ReadError::Argument(m) => Self::Query(m),
            ReadError::NotFound(m) => Self::NotFound(m),
            ReadError::Store(m) => Self::Ledger(m),
        }
    }
}

type Result<T> = std::result::Result<T, EngineError>;

fn ledger_err(e: impl std::fmt::Display) -> EngineError {
    EngineError::Ledger(e.to_string())
}

/// One read the engine serves: its name here, the MCP tool with the same answer (`None` when
/// there is none), and whether a reader with a restricting scope may call it.
pub struct Read {
    pub name: &'static str,
    pub mcp_tool: Option<&'static str>,
    pub scoped: bool,
}

/// Every read, in one table. `scoped` must agree with the MCP server's `SCOPED_TOOLS` for the
/// reads that are MCP tools — a test in the CLI compares the two lists.
pub const READS: &[Read] = &[
    Read {
        name: "status",
        mcp_tool: Some("ekos_status"),
        scoped: false,
    },
    Read {
        name: "search",
        mcp_tool: Some("ekos_search"),
        scoped: true,
    },
    Read {
        name: "query",
        mcp_tool: Some("ekos_query"),
        scoped: true,
    },
    Read {
        name: "retrieve",
        mcp_tool: Some("ekos_retrieve"),
        scoped: true,
    },
    Read {
        name: "ekl",
        mcp_tool: Some("ekos_ekl"),
        scoped: true,
    },
    Read {
        name: "neighborhood",
        mcp_tool: Some("ekos_neighborhood"),
        scoped: true,
    },
    Read {
        name: "state",
        mcp_tool: Some("ekos_state"),
        scoped: true,
    },
    Read {
        name: "dependents",
        mcp_tool: Some("ekos_dependents"),
        scoped: true,
    },
    Read {
        name: "impact",
        mcp_tool: Some("ekos_impact"),
        scoped: true,
    },
    Read {
        name: "diff",
        mcp_tool: Some("ekos_diff"),
        scoped: true,
    },
    Read {
        name: "graph_export",
        mcp_tool: Some("ekos_graph_export"),
        scoped: true,
    },
    Read {
        name: "audit",
        mcp_tool: Some("ekos_audit"),
        scoped: false,
    },
    // No MCP tool: both read one object through the store, which the scope filters.
    Read {
        name: "facts",
        mcp_tool: None,
        scoped: true,
    },
    Read {
        name: "history",
        mcp_tool: None,
        scoped: true,
    },
];

/// An open, read-only knowledge store.
pub struct Engine {
    store: Mutex<Box<dyn KnowledgeStore>>,
    config: EkosConfig,
    cwd: PathBuf,
    location: String,
    restricted: bool,
    warnings: Vec<String>,
}

/// What the caller should be told about a store that opened: today, only a search index
/// written under another content version. A read-only open never rebuilds it.
fn open_warnings(backend: &Backend) -> Vec<String> {
    let Backend::Facts(dir) = backend else {
        return Vec::new();
    };
    let search = dir.join("search");
    match ekos_ledger::search::recorded_content_version(&search) {
        Some(v) if v != ekos_ledger::search::CONTENT_VERSION => vec![format!(
            "the search index at {} was written by another EKOS version (content version {v}, \
             this engine reads {}); search works over the older text until a command that \
             writes to the ledger rebuilds it, for example `ekos ledger repair`",
            search.display(),
            ekos_ledger::search::CONTENT_VERSION
        )],
        _ => Vec::new(),
    }
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
    /// Open the ledger of the workspace or home project at `path`, read-only. `read_scope` is
    /// an RFC 0181 scope as JSON (`--read-scope`'s format); every read is then filtered by it.
    pub fn open(path: &Path, read_scope: Option<&str>) -> Result<Self> {
        let (config, cwd) = resolve(path)?;
        let scope = read_scope
            .map(|json| {
                ReadScope::from_json(json)
                    .map_err(|e| EngineError::Query(format!("read scope: {e}")))
            })
            .transpose()?;
        let backend = detect(&config, &cwd);
        let warnings = open_warnings(&backend);
        let (store, location): (Box<dyn KnowledgeStore>, PathBuf) = match backend {
            Backend::Distributed => {
                return Err(EngineError::NotSupported(
                    "this project reads from a distributed cluster; use ekos.connect(...)".into(),
                ));
            }
            Backend::Sqlite(file) => {
                return Err(EngineError::NotSupported(format!(
                    "{} is a SQLite ledger, which cannot be opened without writing beside it; \
                     convert it once with `ekos ledger migrate --v3`",
                    file.display()
                )));
            }
            Backend::Fresh(dir) => {
                return Err(EngineError::Ledger(format!(
                    "no compiled ledger under {} — run the EKOS pipeline first",
                    dir.parent().unwrap_or(&dir).display()
                )));
            }
            Backend::Partitioned(dir) => {
                let ledger = open_partitioned(&config, &cwd).map_err(EngineError::Ledger)?;
                (Box::new(ledger.read_only()), dir)
            }
            Backend::Facts(dir) => {
                let ledger = FactLedger::open_read_only(&dir)
                    .map_err(|e| ledger_err(format!("cannot open {}: {e}", dir.display())))?;
                (Box::new(ledger), dir)
            }
        };
        let (store, restricted): (Box<dyn KnowledgeStore>, bool) = match scope {
            Some(scope) => {
                let scoped = ScopedStore::with_scopes(store, &[scope])
                    .map_err(|e| EngineError::Query(format!("read scope: {e}")))?;
                let restricted = scoped.is_restricted();
                (Box::new(scoped), restricted)
            }
            None => (store, false),
        };
        Ok(Self {
            store: Mutex::new(store),
            config,
            cwd,
            location: location.display().to_string(),
            restricted,
            warnings,
        })
    }

    /// What the caller should know about this store, in words. Empty when all is as expected.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    fn with<T>(&self, f: impl FnOnce(&dyn KnowledgeStore, &Runtime<'_>) -> Result<T>) -> Result<T> {
        let guard = self
            .store
            .lock()
            .map_err(|_| ledger_err("an earlier call panicked while holding this ledger"))?;
        let store: &dyn KnowledgeStore = guard.as_ref();
        f(store, &Runtime::over(store))
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

    /// Run the read `name` (a row of [`READS`]) with `args`, the JSON arguments of the MCP tool
    /// of the same name. `query_embedding` is the query as a vector, for `search` in `vector` or
    /// `hybrid` mode: the engine makes no network call, so the caller supplies it.
    pub fn call(
        &self,
        name: &str,
        args: &Value,
        query_embedding: Option<Vec<f32>>,
    ) -> Result<Value> {
        let read = READS
            .iter()
            .find(|r| r.name == name)
            .ok_or_else(|| EngineError::Query(format!("unknown read {name:?}")))?;
        if self.restricted && !read.scoped {
            return Err(EngineError::NotSupported(format!(
                "`{name}` is not available to a reader with a restricted scope"
            )));
        }
        let (config, cwd) = (&self.config, self.cwd.as_path());
        self.with(|store, runtime| {
            Ok(match name {
                "status" => tools::status(store, &self.location)?,
                "search" => tools::search(runtime, args, query_embedding)?,
                "query" => tools::query(config, cwd, store, runtime, args)?,
                "retrieve" => tools::retrieve(config, cwd, store, runtime, args)?,
                "ekl" => tools::ekl(runtime, args)?,
                "neighborhood" => tools::neighborhood(runtime, args)?,
                "state" => tools::state(config, cwd, store, runtime, args)?,
                "dependents" => tools::dependents(runtime, args)?,
                "impact" => tools::impact(runtime, args)?,
                "diff" => tools::diff(store, runtime, args)?,
                "graph_export" => tools::graph_export(cwd, store, args)?,
                "audit" => tools::audit(store, args)?,
                "facts" => facts(runtime, args)?,
                "history" => history(runtime, args)?,
                other => return Err(EngineError::Query(format!("unknown read {other:?}"))),
            })
        })
    }
}

fn id_arg(args: &Value) -> Result<KirId> {
    Ok(ekos_read::args::required_id(args)?)
}

/// Everything compiled about one object as attribute → value, key-sorted after `name` and `kind`.
fn facts(runtime: &Runtime<'_>, args: &Value) -> Result<Value> {
    let id = id_arg(args)?;
    let facts = runtime.facts_of(&id).map_err(ledger_err)?;
    if facts.is_empty() {
        return Err(EngineError::NotFound(format!("object not found: {id}")));
    }
    Ok(json!({
        "id": id.to_string(),
        "facts": facts
            .into_iter()
            .map(|(attribute, value)| json!({ "attribute": attribute, "value": value }))
            .collect::<Vec<_>>(),
    }))
}

/// Every version of one object the ledger holds, oldest first. The ledger is append-only: a
/// correction is a new version, never an edit.
fn history(runtime: &Runtime<'_>, args: &Value) -> Result<Value> {
    let id = id_arg(args)?;
    let versions = runtime.object_history(&id).map_err(ledger_err)?;
    Ok(json!({ "id": id.to_string(), "versions": versions }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind};

    /// A workspace with a fact-engine ledger: `orders` depends on `customers`, plus one piece of
    /// session memory that no read may return.
    fn workspace() -> (tempfile::TempDir, KirObject, KirObject) {
        let dir = tempfile::tempdir().unwrap();
        let facts = dir.path().join(".ekos").join("ledger").join("facts");
        let writer = FactLedger::open(&facts).unwrap();
        let orders = KirObject::new("orders", ObjectKind::Table);
        let customers = KirObject::new("customers", ObjectKind::Table);
        let note = KirObject::new("session-note", ObjectKind::Custom("SessionClaim".into()));
        for object in [&orders, &customers, &note] {
            writer.append_object(object).unwrap();
        }
        writer
            .append_relationship(&KirRelationship::new(
                RelationshipKind::DependsOn,
                orders.id,
                customers.id,
            ))
            .unwrap();
        writer.find_objects("orders").unwrap(); // commits the search index
        (dir, orders, customers)
    }

    fn id(object: &KirObject) -> Value {
        json!({ "id": object.id.to_string() })
    }

    #[test]
    fn a_directory_with_no_ledger_is_an_error_and_stays_empty() {
        let dir = tempfile::tempdir().unwrap();
        let err = Engine::open(dir.path(), None)
            .err()
            .expect("no ledger to open");
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
        let dir = tempfile::tempdir().unwrap();
        let facts = dir.path().join(".ekos").join("ledger").join("facts");
        let writer = FactLedger::open(&facts).unwrap();
        writer
            .append_object(&KirObject::new("orders", ObjectKind::Table))
            .unwrap();
        writer.find_objects("orders").unwrap();

        let reader = Engine::open(dir.path(), None).unwrap();
        let objects = |r: &Engine| r.call("status", &json!({}), None).unwrap()["objects"].clone();
        assert_eq!(objects(&reader), 1);

        writer
            .append_object(&KirObject::new("invoices", ObjectKind::Table))
            .expect("an open reader must not block a writer");
        writer.find_objects("invoices").unwrap();

        assert_eq!(objects(&reader), 1, "still the snapshot");
        assert!(reader.refresh().unwrap());
        assert_eq!(objects(&reader), 2);
        let hits = reader
            .call("search", &json!({ "query": "invoices", "limit": 5 }), None)
            .unwrap();
        assert_eq!(hits["matches"][0]["name"], "invoices");
        assert!(!reader.refresh().unwrap(), "nothing new the second time");
    }

    #[test]
    fn a_search_index_of_another_content_version_is_reported_and_left_alone() {
        let (dir, _, _) = workspace();
        assert!(
            Engine::open(dir.path(), None)
                .unwrap()
                .warnings()
                .is_empty()
        );

        let version = dir.path().join(".ekos/ledger/facts/search/content_version");
        std::fs::write(&version, "1").unwrap();
        let engine = Engine::open(dir.path(), None).unwrap();
        assert!(engine.warnings()[0].contains("content version 1"));
        let hits = engine
            .call("search", &json!({ "query": "orders" }), None)
            .unwrap();
        assert_eq!(hits["matches"][0]["name"], "orders", "search still answers");
        assert_eq!(
            std::fs::read_to_string(&version).unwrap(),
            "1",
            "and nothing was rebuilt"
        );
    }

    #[test]
    fn every_read_in_the_table_answers_on_a_real_ledger() {
        let (dir, orders, customers) = workspace();
        let engine = Engine::open(dir.path(), None).unwrap();
        let call = |name: &str, args: Value| engine.call(name, &args, None).unwrap();

        assert_eq!(call("status", json!({}))["objects"], 3);
        assert_eq!(
            call("search", json!({ "query": "orders" }))["matches"][0]["name"],
            "orders"
        );
        assert_eq!(call("state", id(&orders))["object"]["name"], "orders");
        assert_eq!(call("facts", id(&orders))["facts"][0]["value"], "orders");
        assert_eq!(
            call("history", id(&orders))["versions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            call("dependents", id(&customers))["dependents"][0]["name"],
            "orders"
        );
        assert_eq!(
            call("dependents", id(&orders))["dependencies"][0]["name"],
            "customers"
        );
        assert_eq!(call("impact", id(&customers))["hops"][0]["name"], "orders");
        assert_eq!(
            call("neighborhood", id(&orders))["objects"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            call(
                "ekl",
                json!({ "query": "FIND Object WHERE kind = 'Table'" })
            )["count"],
            2
        );
        assert!(call("query", json!({ "question": "what depends on customers?" })).is_object());
        assert!(call("retrieve", json!({ "question": "orders" }))["plan"].is_object());
        assert!(call("diff", json!({ "from": "2000-01-01T00:00:00Z" }))["changed_total"].is_u64());
        assert!(call("graph_export", json!({}))["nodes"].is_array());
        assert!(call("audit", id(&orders))["writes"].is_array());

        let served: Vec<&str> = READS.iter().map(|r| r.name).collect();
        assert_eq!(
            served.len(),
            14,
            "a new read needs a line in this test: {served:?}"
        );
    }

    #[test]
    fn wrong_arguments_and_unknown_ids_are_told_apart() {
        let (dir, _, _) = workspace();
        let engine = Engine::open(dir.path(), None).unwrap();
        let err = |name: &str, args: Value| engine.call(name, &args, None).unwrap_err();
        let nobody = json!({ "id": KirId::new().to_string() });

        assert!(matches!(
            err("state", json!({ "id": "orders" })),
            EngineError::Query(_)
        ));
        assert!(matches!(
            err("state", nobody.clone()),
            EngineError::NotFound(_)
        ));
        assert!(matches!(err("facts", nobody), EngineError::NotFound(_)));
        assert!(matches!(
            err("ekl", json!({ "query": "FIND" })),
            EngineError::Query(_)
        ));
        assert!(matches!(
            err("no_such_read", json!({})),
            EngineError::Query(_)
        ));
        assert!(matches!(
            err("search", json!({ "query": "x", "mode": "hybrid" })),
            EngineError::Query(m) if m.contains("vector")
        ));
        assert!(matches!(
            err("search", json!({ "query": "x", "mode": "fuzzy" })),
            EngineError::Query(_)
        ));
    }

    /// The guarantee `Runtime` gives, checked from the outside: session memory is in the store
    /// and no read returns it.
    #[test]
    fn no_read_returns_session_memory() {
        let (dir, orders, _) = workspace();
        let engine = Engine::open(dir.path(), None).unwrap();
        let answers = [
            engine.call("search", &json!({ "query": "session note" }), None),
            engine.call("ekl", &json!({ "query": "FIND Object" }), None),
            engine.call("graph_export", &json!({}), None),
            engine.call("neighborhood", &id(&orders), None),
            engine.call("diff", &json!({ "from": "2000-01-01T00:00:00Z" }), None),
        ];
        for answer in answers {
            let text = answer.unwrap().to_string();
            assert!(!text.contains("session-note"), "{text}");
            assert!(!text.contains("SessionClaim"), "{text}");
        }
    }

    #[test]
    fn a_restricting_scope_filters_reads_and_refuses_the_reads_it_cannot_filter() {
        let (dir, orders, _) = workspace();
        // The fixture's objects carry no source, so a scope that allows one source sees none.
        let scope = r#"{"default":"deny","rules":[{"effect":"allow","source":"app"}]}"#;
        let engine = Engine::open(dir.path(), Some(scope)).unwrap();

        let hits = engine
            .call("search", &json!({ "query": "orders" }), None)
            .unwrap();
        assert!(hits["matches"].as_array().unwrap().is_empty(), "{hits}");
        assert!(matches!(
            engine.call("state", &id(&orders), None),
            Err(EngineError::NotFound(_))
        ));
        for refused in ["status", "audit"] {
            assert!(matches!(
                engine.call(refused, &id(&orders), None),
                Err(EngineError::NotSupported(_))
            ));
        }
        assert!(matches!(
            Engine::open(dir.path(), Some("{}")),
            Err(EngineError::Query(_))
        ));
    }

    #[test]
    fn stores_the_engine_cannot_read_without_writing_are_refused_by_name() {
        let sqlite = tempfile::tempdir().unwrap();
        let ledger = sqlite.path().join(".ekos").join("ledger");
        std::fs::create_dir_all(&ledger).unwrap();
        std::fs::write(ledger.join("ledger.db"), b"").unwrap();
        let err = Engine::open(sqlite.path(), None)
            .err()
            .expect("SQLite is refused");
        assert!(
            matches!(&err, EngineError::NotSupported(m) if m.contains("migrate --v3")),
            "{err:?}"
        );

        let cluster = tempfile::tempdir().unwrap();
        std::fs::write(
            cluster.path().join("ekos.toml"),
            "[storage.distributed]\ncoordinator = \"127.0.0.1:1\"\nquery-workers = [\"127.0.0.1:2\"]\n",
        )
        .unwrap();
        let err = Engine::open(cluster.path(), None)
            .err()
            .expect("a cluster is refused");
        assert!(
            matches!(&err, EngineError::NotSupported(m) if m.contains("connect")),
            "{err:?}"
        );

        let broken = tempfile::tempdir().unwrap();
        std::fs::write(broken.path().join("ekos.toml"), "[workspace\n").unwrap();
        assert!(matches!(
            Engine::open(broken.path(), None),
            Err(EngineError::Ledger(_))
        ));
        assert!(matches!(
            Engine::open(&broken.path().join("missing"), None),
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

    /// Rule 4 of the module: nothing outside the tests writes. The engine, its glue and the
    /// shared read crate name no append, no writable open and no write context.
    #[test]
    fn the_engine_and_the_read_tools_have_no_write_path() {
        let sources = [
            ("python/src/engine.rs", include_str!("engine.rs")),
            ("python/src/lib.rs", include_str!("lib.rs")),
            ("read/src/lib.rs", include_str!("../../read/src/lib.rs")),
            ("read/src/args.rs", include_str!("../../read/src/args.rs")),
            ("read/src/kinds.rs", include_str!("../../read/src/kinds.rs")),
            ("read/src/tools.rs", include_str!("../../read/src/tools.rs")),
            (
                "read/src/conflicts.rs",
                include_str!("../../read/src/conflicts.rs"),
            ),
        ];
        let forbidden = [
            concat!("append", "_"),
            concat!("FactLedger::", "open("),
            concat!("Ledger::", "open("),
            concat!("set_write", "_context"),
            concat!("fs::", "write"),
            concat!("create_dir", "_all"),
        ];
        for (name, source) in sources {
            let code = source.split("#[cfg(test)]").next().unwrap();
            for word in forbidden {
                assert!(
                    !code.contains(word),
                    "{name} names `{word}` outside its tests"
                );
            }
        }
    }
}
