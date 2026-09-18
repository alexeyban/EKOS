//! RFC 0149: an out-of-tree extension takes part in every stage it hooks — its observer runs in
//! `build`, its pass sees that observer's artifacts in `recover` and reports back, its
//! `after_commit` runs in `commit`, and its MCP tool is listed and dispatched — while a build
//! with `Extensions::none()` behaves exactly as before.

use async_trait::async_trait;
use ekos::extension::{
    CommitContext, EkosExtension, Extensions, RecoverContext, RecoveryContribution,
};
use ekos_artifact::{ArtifactId, ObservationArtifact};
use ekos_compiler_core::EkosConfig;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_ledger::KnowledgeStore;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tempfile::TempDir;

const CONNECTOR: &str = "fake-ext";
const TOOL: &str = "fake_ext_echo";

#[derive(Default)]
struct Probe {
    observer_scans: AtomicUsize,
    artifacts_seen_by_pass: AtomicUsize,
    report_called: AtomicBool,
    after_commit_called: AtomicBool,
}

struct FakeObserver(Arc<Probe>);

#[async_trait]
impl Observer for FakeObserver {
    fn name(&self) -> &str {
        CONNECTOR
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        self.0.observer_scans.fetch_add(1, Ordering::SeqCst);
        let mut package =
            ObservationPackage::new(CONNECTOR, ctx.workspace_root.display().to_string());
        package.artifacts.push(ObservationArtifact::new(
            CONNECTOR,
            "fake-target",
            json!({ "payload": "from the fake observer" }),
        ));
        Ok(package)
    }
}

struct FakePass {
    probe: Arc<Probe>,
    ids: Vec<ArtifactId>,
}

#[async_trait]
impl CompilerPass for FakePass {
    fn name(&self) -> &str {
        "fake-ext-pass"
    }

    async fn run(&mut self, _ctx: &mut PassContext) -> Result<(), PassError> {
        self.probe
            .artifacts_seen_by_pass
            .store(self.ids.len(), Ordering::SeqCst);
        Ok(())
    }
}

struct FakeExtension(Arc<Probe>);

#[async_trait(?Send)]
impl EkosExtension for FakeExtension {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn logic_version(&self) -> u32 {
        7
    }

    fn observers(&self, _config: &EkosConfig) -> Vec<Box<dyn Observer>> {
        vec![Box::new(FakeObserver(self.0.clone()))]
    }

    fn recovery_passes(&self, ctx: &RecoverContext<'_>) -> Vec<RecoveryContribution> {
        let ids = ctx.artifact_ids_for_connector(CONNECTOR);
        let probe = self.0.clone();
        vec![RecoveryContribution {
            pass: Box::new(FakePass {
                probe: self.0.clone(),
                ids,
            }),
            report: Box::new(move || {
                probe.report_called.store(true, Ordering::SeqCst);
                vec!["Fake extension: reported".to_string()]
            }),
        }]
    }

    async fn after_commit(&self, ctx: &CommitContext<'_>) -> anyhow::Result<Vec<String>> {
        // Touch the ledger across an await point: a real post-commit step does, and this is what
        // requires the trait's `?Send` (`dyn KnowledgeStore` is not `Sync`).
        let objects = ctx.ledger.all_objects()?;
        tokio::task::yield_now().await;
        assert!(
            !objects.is_empty(),
            "commit must have written objects first"
        );
        self.0.after_commit_called.store(true, Ordering::SeqCst);
        Ok(vec!["Fake extension: committed".to_string()])
    }

    fn mcp_tools(&self, _config: &EkosConfig) -> Vec<Value> {
        vec![json!({
            "name": TOOL,
            "description": "Echoes its `text` argument (RFC 0149 test extension).",
            "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } } }
        })]
    }

    fn call_mcp_tool(
        &self,
        name: &str,
        args: &Value,
        _ledger: &dyn KnowledgeStore,
    ) -> Option<anyhow::Result<Value>> {
        (name == TOOL).then(|| Ok(json!({ "echo": args["text"] })))
    }
}

fn setup_workspace(dir: &Path) -> EkosConfig {
    std::fs::create_dir_all(dir.join("app")).unwrap();
    std::fs::write(dir.join("app/main.rs"), b"fn main() {}").unwrap();
    // api-key-env points at a variable that is never set, so recover selects the mock LLM
    // provider — no network in tests.
    std::fs::write(
        dir.join("ekos.toml"),
        b"[workspace]\nroot = \".\"\n\n\
          [observe]\npaths = [\"app\"]\nignore-patterns = [\".ekos\"]\n\n\
          [llm]\napi-key-env = \"EKOS_TEST_KEY_THAT_DOES_NOT_EXIST\"\n",
    )
    .unwrap();
    EkosConfig::from_file(&dir.join("ekos.toml")).unwrap()
}

fn rpc(
    config: &EkosConfig,
    dir: &Path,
    ext: &Extensions,
    cache: &mut ekos::commands::mcp::StoreCache,
    method: &str,
    params: Value,
) -> Value {
    let line = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string();
    let response = ekos::commands::mcp::handle_message_with(config, dir, &line, cache, ext)
        .expect("a request is always answered");
    serde_json::from_str(&response).unwrap()
}

fn tool_names(response: &Value) -> Vec<String> {
    response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn an_extension_takes_part_in_every_stage_it_hooks() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let config = setup_workspace(dir);
    let probe = Arc::new(Probe::default());
    let ext = Extensions::new(vec![Arc::new(FakeExtension(probe.clone()))]);

    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run_with(&config, dir, &ext)
        .await
        .unwrap();
    assert_eq!(probe.observer_scans.load(Ordering::SeqCst), 1);

    ekos::commands::recover::run_with(&config, dir, false, &ext)
        .await
        .unwrap();
    assert_eq!(
        probe.artifacts_seen_by_pass.load(Ordering::SeqCst),
        1,
        "the pass must see exactly the artifact its own observer wrote"
    );
    assert!(probe.report_called.load(Ordering::SeqCst));

    ekos::commands::compile::run(&config, dir).await.unwrap();
    ekos::commands::commit::run_with(&config, dir, true, &ext)
        .await
        .unwrap();
    assert!(probe.after_commit_called.load(Ordering::SeqCst));

    let mut cache = ekos::commands::mcp::StoreCache::new();
    let listed = rpc(&config, dir, &ext, &mut cache, "tools/list", json!({}));
    assert!(tool_names(&listed).contains(&TOOL.to_string()));

    let called = rpc(
        &config,
        dir,
        &ext,
        &mut cache,
        "tools/call",
        json!({ "name": TOOL, "arguments": { "text": "hello" } }),
    );
    assert_eq!(called["result"]["isError"], false);
    let body: Value =
        serde_json::from_str(called["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["echo"], "hello");

    // A tool neither the built-ins nor the extension own still fails as unknown.
    let unknown = rpc(
        &config,
        dir,
        &ext,
        &mut cache,
        "tools/call",
        json!({ "name": "no_such_tool", "arguments": {} }),
    );
    assert_eq!(unknown["result"]["isError"], true);
    assert!(
        unknown["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown tool")
    );

    // Without the extension the same server neither lists nor dispatches it.
    let listed = rpc(
        &config,
        dir,
        &Extensions::none(),
        &mut cache,
        "tools/list",
        json!({}),
    );
    assert!(!tool_names(&listed).contains(&TOOL.to_string()));
}

#[test]
fn no_extensions_leave_the_build_fingerprint_version_unchanged() {
    assert_eq!(Extensions::none().logic_version(), 0);
    let probe = Arc::new(Probe::default());
    let one = Extensions::new(vec![Arc::new(FakeExtension(probe))]);
    assert_ne!(one.logic_version(), 0);
}
