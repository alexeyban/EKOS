//! File-system observer plugin.
//!
//! Walks the workspace directory tree and emits one `ObservationArtifact` per
//! regular file. The artifact data contains the file's relative path, byte size,
//! and SHA-256 content hash — enough for downstream passes to detect changes
//! and to produce deterministic `KirObject` IDs.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Reference file-system observer.
///
/// Scans the `workspace_root` recursively, skipping any directory whose name
/// matches an entry in `ctx.ignore_patterns`. Emits one `ObservationArtifact`
/// per regular file.
pub struct FileObserver;

impl FileObserver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FileObserver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Observer for FileObserver {
    fn name(&self) -> &str {
        "file"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("file", &target);

        for entry in WalkDir::new(root).into_iter().filter_entry(|e| {
            // Skip ignored directory names (e.g. .git, target).
            if e.file_type().is_dir()
                && let Some(name) = e.file_name().to_str()
            {
                return !ctx.ignore_patterns.iter().any(|p| name == p.as_str());
            }
            true
        }) {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!("file observer: skipping unreadable entry: {err}");
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let abs_path = entry.path();
            let rel_path = match abs_path.strip_prefix(root) {
                // A real, valid `[observe] paths` shape — a single bare file, not a
                // directory (`paths = ["README.md"]`, or this project's own `paths =
                // ["mix.exs", "mix.lock", ...]`). `WalkDir::new(root)` then yields exactly
                // one entry equal to `root` itself, and stripping a path from itself leaves
                // an empty relative path — silently producing a nameless, pathless object
                // downstream (found live: a real project-level LLM summary matched the
                // wrong document because the real README's own name/path were both empty).
                // Falls back to the file's own name so it's never blank.
                Ok(r) if r.as_os_str().is_empty() => abs_path
                    .file_name()
                    .map(|n| n.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default(),
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };

            // Double-check: skip if any path component is ignored.
            if ctx.is_ignored(&rel_path) {
                continue;
            }

            let content = match tokio::fs::read(abs_path).await {
                Ok(bytes) => bytes,
                Err(err) => {
                    tracing::warn!("file observer: cannot read {}: {err}", abs_path.display());
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            let size_bytes = content.len();
            let content_sha256 = {
                let mut h = Sha256::new();
                h.update(&content);
                hex::encode(h.finalize())
            };

            let mut data = serde_json::json!({
                "path": rel_path,
                "size_bytes": size_bytes,
                "content_sha256": content_sha256,
            });
            // RFC 0014: for text files, the opening excerpt is an observation
            // fact — it feeds the ledger's content FTS. Binary files get none.
            if let Some(excerpt) = text_excerpt(&content) {
                data["excerpt"] = serde_json::Value::String(excerpt);
            }
            // RFC 0019: declaration-line symbols, harvested from the *full*
            // content (unlike the 600-char excerpt above) — makes
            // `ekos_search "authenticate"` findable even when the matching
            // `fn`/`def`/`class` sits deep in a large file.
            if let Ok(text) = std::str::from_utf8(&content) {
                let symbols = harvest_symbols(text);
                if !symbols.is_empty() {
                    data["symbols"] = serde_json::Value::Array(
                        symbols.into_iter().map(serde_json::Value::String).collect(),
                    );
                }
            }

            let artifact =
                ObservationArtifact::new("file", &rel_path, data).with_producer("ekos-plugin-file");

            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

/// Cap on the excerpt captured from text files (RFC 0014). 600 chars covers
/// headings and preamble — where a document says what it is — without
/// bloating the FTS index with entire file bodies.
const EXCERPT_MAX_CHARS: usize = 600;

/// The opening excerpt of a text file, or `None` for binary content.
/// Truncates on a char boundary so the result is always valid UTF-8.
fn text_excerpt(content: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(content).ok()?;
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(EXCERPT_MAX_CHARS).collect())
}

/// Declaration keywords recognized by [`harvest_symbols`] (RFC 0019). Plain
/// prefix matching, not per-language parsing — covers the common case
/// (`fn foo(...)`, `def foo(...):`, `class Foo:`, `func foo(...)`,
/// `interface Foo {`) without a parser dependency.
///
/// RFC 0076: the Elixir forms (`defp`/`defmodule`/`defmacro`/`defmacrop`/`defdelegate`) were
/// missing entirely — `"def "` only matches a literal `def` token followed by a space, so
/// `defp foo(...)` (a private function — as common as `def` in real Elixir: 1917 vs 2509
/// occurrences in a real, large open-source Elixir codebase tested against) and `defmodule Foo
/// do` (the language's primary structural unit — 522 occurrences in that same codebase) were both
/// silently invisible to this fallback, on top of `Table.md#L164`-style ordinary code prose. Since
/// this scan has no notion of "language" at all — a Rust/Go/Python/TS project never has a `defp`
/// line to begin with — adding Elixir's forms costs nothing for every other language already
/// covered here.
const DECL_PREFIXES: &[&str] = &[
    "fn ",
    "def ",
    "defp ",
    "defmodule ",
    "defmacro ",
    "defmacrop ",
    "defdelegate ",
    "class ",
    "func ",
    "interface ",
];

/// Cap on symbols harvested per file — bounds indexed content size the same
/// way `EXCERPT_MAX_CHARS` bounds the excerpt (RFC 0019).
const SYMBOLS_MAX: usize = 50;

/// Scans every line for a recognized declaration prefix and extracts the
/// identifier that follows. Not an AST parse: a line that merely mentions
/// `fn ` in a comment or string literal is indistinguishable from a real
/// declaration — an accepted v1 false-positive rate, same tradeoff RFC 0019
/// makes for dependency-pattern matching.
fn harvest_symbols(text: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for line in text.lines() {
        if symbols.len() >= SYMBOLS_MAX {
            break;
        }
        let trimmed = line.trim_start();
        for prefix in DECL_PREFIXES {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    symbols.push(name);
                }
                break;
            }
        }
    }
    symbols
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn scan_temp(setup: impl FnOnce(&TempDir)) -> ObservationPackage {
        let dir = TempDir::new().unwrap();
        setup(&dir);
        let ctx = ScanContext::new(dir.path());
        FileObserver::new().scan(&ctx).await.unwrap()
    }

    #[tokio::test]
    async fn empty_dir_produces_no_artifacts() {
        let pkg = scan_temp(|_| {}).await;
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn a_single_bare_file_observe_path_gets_its_own_real_name_not_an_empty_one() {
        // Real bug found live (RFC 0088's own project-level LLM summary picked the wrong
        // real document because the real README's `name`/`path` were both empty strings):
        // a real, valid `[observe] paths` shape is a single bare file, not a directory
        // (`paths = ["README.md"]` — this project's own backend-only config does exactly
        // this for `mix.exs`/`mix.lock`/`README.md`/`CHANGELOG.md`). `WalkDir::new(root)`
        // then yields exactly one entry equal to `root` itself, and stripping a path from
        // itself leaves an empty relative path.
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("README.md");
        std::fs::write(&file_path, b"# Real Project\n").unwrap();

        let ctx = ScanContext::new(&file_path);
        let pkg = FileObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 1);
        assert_eq!(
            pkg.artifacts[0].content.target, "README.md",
            "a bare-file observe path must never produce an empty relative name/path"
        );
    }

    #[test]
    fn harvest_symbols_finds_known_declaration_kinds() {
        let text = "fn authenticate_user(x: u32) {}\ndef login(user):\n    pass\nclass AuthService:\n    pass\nfunc Handle(w Response) {}\ninterface Authenticator {\n";
        let symbols = harvest_symbols(text);
        assert_eq!(
            symbols,
            vec![
                "authenticate_user",
                "login",
                "AuthService",
                "Handle",
                "Authenticator"
            ]
        );
    }

    #[test]
    fn harvest_symbols_finds_elixir_declaration_forms() {
        let text = "defmodule Plausible.Auth do\n  def rate_limit(x) do\n  end\n\n  defp hash(pw) do\n  end\n\n  defmacro is_valid(x) do\n  end\nend\n";
        let symbols = harvest_symbols(text);
        assert_eq!(
            symbols,
            vec!["Plausible", "rate_limit", "hash", "is_valid"],
            "defmodule/def/defp/defmacro must all be recognized — a real Elixir codebase uses \
             defp almost as often as def (RFC 0076)"
        );
    }

    #[test]
    fn harvest_symbols_a_bare_def_line_is_not_mistaken_for_defp_or_defmodule() {
        // Regression for the prefix-matching itself: `def ` must not accidentally swallow
        // `defp `/`defmodule ` lines (or vice versa) since they share a `def` prefix.
        let text = "def login(user) do\nend\n";
        assert_eq!(harvest_symbols(text), vec!["login"]);
    }

    #[test]
    fn harvest_symbols_ignores_lines_without_a_declaration() {
        let text = "// just a comment\nlet x = 1;\nreturn foo();\n";
        assert!(harvest_symbols(text).is_empty());
    }

    #[test]
    fn harvest_symbols_is_capped() {
        let text = (0..100)
            .map(|i| format!("fn f{i}() {{}}\n"))
            .collect::<String>();
        assert_eq!(harvest_symbols(&text).len(), SYMBOLS_MAX);
    }

    #[tokio::test]
    async fn symbols_ride_on_the_artifact_alongside_excerpt() {
        let pkg = scan_temp(|dir| {
            std::fs::write(dir.path().join("auth.rs"), b"fn authenticate_user() {}\n").unwrap();
        })
        .await;
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["symbols"], serde_json::json!(["authenticate_user"]));
    }

    #[tokio::test]
    async fn files_with_no_declarations_carry_no_symbols_field() {
        let pkg = scan_temp(|dir| {
            std::fs::write(dir.path().join("notes.md"), b"just some prose\n").unwrap();
        })
        .await;
        assert!(pkg.artifacts[0].content.data.get("symbols").is_none());
    }

    #[tokio::test]
    async fn text_files_carry_an_excerpt_binary_files_do_not() {
        let long_text = "# Title\n".to_string() + &"x".repeat(2000);
        let pkg = scan_temp(move |dir| {
            std::fs::write(dir.path().join("note.md"), long_text.as_bytes()).unwrap();
            std::fs::write(dir.path().join("blob.bin"), [0xff, 0xfe, 0x00, 0x9f]).unwrap();
        })
        .await;

        let note = pkg
            .artifacts
            .iter()
            .find(|a| a.content.target == "note.md")
            .unwrap();
        let excerpt = note.content.data["excerpt"].as_str().unwrap();
        assert!(excerpt.starts_with("# Title"), "excerpt keeps the opening");
        assert_eq!(
            excerpt.chars().count(),
            EXCERPT_MAX_CHARS,
            "excerpt is capped"
        );

        let blob = pkg
            .artifacts
            .iter()
            .find(|a| a.content.target == "blob.bin")
            .unwrap();
        assert!(
            blob.content.data.get("excerpt").is_none(),
            "binary files carry no excerpt"
        );
    }

    #[tokio::test]
    async fn single_file_produces_one_artifact() {
        let pkg = scan_temp(|dir| {
            std::fs::write(dir.path().join("hello.txt"), b"hello").unwrap();
        })
        .await;
        assert_eq!(pkg.len(), 1);
        assert_eq!(pkg.artifacts[0].content.connector_name, "file");
        assert_eq!(pkg.artifacts[0].content.target, "hello.txt");
    }

    #[tokio::test]
    async fn same_file_same_artifact_id() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("f.rs"), b"fn main() {}").unwrap();
        let ctx = ScanContext::new(dir.path());
        let obs = FileObserver::new();
        let pkg1 = obs.scan(&ctx).await.unwrap();
        let pkg2 = obs.scan(&ctx).await.unwrap();
        assert_eq!(
            pkg1.artifacts[0].id, pkg2.artifacts[0].id,
            "same file must yield same artifact ID"
        );
    }

    #[tokio::test]
    async fn changed_file_changes_artifact_id() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f.rs");
        std::fs::write(&path, b"version 1").unwrap();
        let ctx = ScanContext::new(dir.path());
        let obs = FileObserver::new();
        let id1 = obs.scan(&ctx).await.unwrap().artifacts[0].id.clone();
        std::fs::write(&path, b"version 2").unwrap();
        let id2 = obs.scan(&ctx).await.unwrap().artifacts[0].id.clone();
        assert_ne!(
            id1, id2,
            "different file content must yield different artifact ID"
        );
    }

    #[tokio::test]
    async fn git_dir_is_skipped() {
        let pkg = scan_temp(|dir| {
            let git = dir.path().join(".git");
            std::fs::create_dir_all(&git).unwrap();
            std::fs::write(git.join("HEAD"), b"ref: refs/heads/main").unwrap();
            std::fs::write(dir.path().join("src.rs"), b"fn main() {}").unwrap();
        })
        .await;
        assert_eq!(pkg.len(), 1, "only src.rs, .git/HEAD must be skipped");
        assert_eq!(pkg.artifacts[0].content.target, "src.rs");
    }

    #[tokio::test]
    async fn data_contains_expected_fields() {
        let dir = TempDir::new().unwrap();
        let payload = b"hello world";
        std::fs::write(dir.path().join("readme.md"), payload).unwrap();
        let ctx = ScanContext::new(dir.path());
        let pkg = FileObserver::new().scan(&ctx).await.unwrap();
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["path"], "readme.md");
        assert_eq!(data["size_bytes"], payload.len());
        assert!(data["content_sha256"].as_str().unwrap().len() == 64);
    }
}
