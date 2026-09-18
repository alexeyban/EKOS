//! Compiled-binary observer plugin (RFC 0148).
//!
//! Walks the workspace for `.dll`/`.exe`/`.class`/`.jar`/`.war`/`.ear` files and emits one
//! `ObservationArtifact` per recovered type, carrying that type's `DecompiledAst` as JSON.
//!
//! # Why this observer interprets, when the others do not
//!
//! Every source-code observer in this workspace stores the file verbatim and leaves parsing to a
//! recovery pass. A binary has no source to store. The alternatives were both worse:
//!
//! - **Store the raw bytes.** Megabytes of base64 per assembly, unsearchable, un-diffable, and —
//!   decisively — *un-redactable*: RFC 0043's pattern table cannot meaningfully scan a PE image,
//!   so a connection string compiled into an assembly would reach the ledger intact.
//! - **Store nothing and read the file again at `recover` time.** That breaks the content-address
//!   contract: the artifact would not be the evidence the facts were derived from.
//!
//! `plugins/localdocs` already set this precedent — it extracts text from PDF and DOCX at
//! observation time rather than storing the container. The `DecompiledAst` is a binary's readable
//! projection in exactly the way extracted text is a PDF's. It is JSON, so it is searchable,
//! redactable and content-addressable like every other artifact, and it carries the original
//! file's SHA-256 so the two-hop provenance chain still closes on the real bytes.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_binary::{BinaryError, Detected, detect};
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Extensions worth *opening*. Detection itself is by magic bytes — this list only narrows which
/// files are read at all, so that a workspace scan does not slurp the first bytes of every asset,
/// image and log file it passes.
///
/// Both halves matter: an extension alone never decides the format (most `.dll` files are native
/// code with no CLI metadata), and magic bytes alone would mean reading everything.
const BINARY_EXTENSIONS: &[&str] = &["dll", "exe", "class", "jar", "war", "ear"];

/// Observer emitting one artifact per type recovered from a compiled binary.
#[derive(Debug, Default)]
pub struct BinaryObserver;

impl BinaryObserver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Observer for BinaryObserver {
    fn name(&self) -> &str {
        "binary"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let mut pkg = ObservationPackage::new("binary", root.display().to_string());

        for entry in WalkDir::new(root).into_iter().filter_entry(|e| {
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
                    tracing::warn!("binary observer: skipping unreadable entry: {err}");
                    pkg.meta.error_count += 1;
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }

            let abs_path = entry.path();
            let rel_path = match abs_path.strip_prefix(root) {
                // A single bare file as an `[observe] paths` entry strips to an empty relative
                // path — the same real case `plugins/perl` and `plugins/localdocs` document.
                Ok(r) if r.as_os_str().is_empty() => abs_path
                    .file_name()
                    .map(|n| n.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default(),
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if ctx.is_ignored(&rel_path) {
                continue;
            }

            let is_candidate = abs_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .is_some_and(|e| BINARY_EXTENSIONS.contains(&e.as_str()));
            if !is_candidate {
                continue;
            }

            let bytes = match tokio::fs::read(abs_path).await {
                Ok(b) => b,
                Err(err) => {
                    tracing::warn!("binary observer: cannot read {}: {err}", abs_path.display());
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            // A native `.dll` is the overwhelmingly common case and is not an error — logging it
            // at `warn` would bury a real problem under hundreds of lines on any Windows tree.
            if detect(&bytes) == Detected::NativePe {
                tracing::debug!("binary observer: {rel_path} is native code, skipped");
                continue;
            }

            let sha256 = {
                let mut h = Sha256::new();
                h.update(&bytes);
                hex::encode(h.finalize())
            };

            let read = match ekos_binary::read(&bytes, &rel_path, &sha256) {
                Ok(r) => r,
                Err(BinaryError::UnsupportedFormat(msg)) => {
                    tracing::debug!("binary observer: {msg}");
                    continue;
                }
                Err(err) => {
                    tracing::warn!("binary observer: {rel_path}: {err}");
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            for diag in &read.diagnostics {
                tracing::warn!(
                    "binary observer: {rel_path}: {} {}",
                    diag.code,
                    diag.message
                );
            }

            for ast in read.asts {
                let ast_json = match serde_json::to_value(&ast) {
                    Ok(v) => v,
                    Err(err) => {
                        tracing::warn!("binary observer: {rel_path}: cannot serialize AST: {err}");
                        pkg.meta.error_count += 1;
                        continue;
                    }
                };
                // The artifact target is the *type*, not the file: one jar or assembly yields
                // thousands, and they must be individually addressable and individually
                // re-hashable so that changing one class does not invalidate every artifact from
                // its container. A jar member names itself; a .NET type — which has no container
                // entry, because the assembly is the file — is named by its own locator.
                let target = match (&ast.source.container_entry, ast.types.first()) {
                    (Some(member), _) => format!("{rel_path}!{member}"),
                    (None, Some(ty)) => format!("{rel_path}!{}", ty.locator),
                    (None, None) => rel_path.clone(),
                };
                let data = serde_json::json!({
                    "path": rel_path,
                    "target": target,
                    "size_bytes": bytes.len(),
                    "content_sha256": sha256,
                    "ast": ast_json,
                });
                pkg.push(
                    ObservationArtifact::new("binary", &target, data)
                        .with_producer("ekos-plugin-binary"),
                );
            }
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn targets(pkg: &ObservationPackage) -> Vec<String> {
        pkg.artifacts
            .iter()
            .map(|a| a.content.data["target"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn non_binary_files_are_ignored() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("readme.md"), "not a binary").unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        let pkg = BinaryObserver::new()
            .scan(&ScanContext::new(dir.path()))
            .await
            .unwrap();
        assert!(pkg.artifacts.is_empty());
    }

    /// A `.dll` that is not a managed assembly must be skipped silently, not counted as an error.
    /// Most `.dll` files on any real machine are native, so treating them as failures would make
    /// the error count meaningless.
    #[tokio::test]
    async fn a_file_with_a_binary_extension_but_no_recognizable_format_is_skipped() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("native.dll"), b"MZ\x90\x00 not really").unwrap();
        std::fs::write(dir.path().join("empty.jar"), b"").unwrap();
        let pkg = BinaryObserver::new()
            .scan(&ScanContext::new(dir.path()))
            .await
            .unwrap();
        assert!(pkg.artifacts.is_empty());
        assert_eq!(pkg.meta.error_count, 0, "skipping is not an error");
    }

    #[tokio::test]
    async fn ignored_directories_are_not_walked() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("target")).unwrap();
        std::fs::write(
            dir.path().join("target/x.class"),
            [0xCAu8, 0xFE, 0xBA, 0xBE],
        )
        .unwrap();
        let pkg = BinaryObserver::new()
            .scan(&ScanContext::new(dir.path()))
            .await
            .unwrap();
        assert!(pkg.artifacts.is_empty());
    }

    /// The real jar on this machine, if present: one artifact per class, each addressable by its
    /// own `archive!member` target, and each carrying the archive's real hash.
    #[tokio::test]
    async fn a_real_jar_yields_one_artifact_per_class() {
        let Ok(bytes) = std::fs::read("/usr/share/java/pdfbox.jar") else {
            eprintln!("skipping: pdfbox.jar not present on this machine");
            return;
        };
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("pdfbox.jar"), &bytes).unwrap();

        let pkg = BinaryObserver::new()
            .scan(&ScanContext::new(dir.path()))
            .await
            .unwrap();
        assert!(pkg.artifacts.len() > 500, "got {}", pkg.artifacts.len());

        let targets = targets(&pkg);
        assert!(
            targets
                .iter()
                .any(|t| t == "pdfbox.jar!org/apache/pdfbox/pdmodel/PDDocument.class"),
            "targets must name the archive member"
        );

        let a = &pkg.artifacts[0];
        assert_eq!(a.content.data["path"], "pdfbox.jar");
        assert!(a.content.data["ast"]["types"].is_array());
        assert_eq!(
            a.content.data["content_sha256"].as_str().unwrap().len(),
            64,
            "the original file hash closes the provenance chain"
        );
    }

    /// A .NET assembly must yield one artifact per type, each with a distinct target — the same
    /// unit a jar yields. Before `split_by_type`, `mscorlib.dll` produced a single 34 MB artifact
    /// that re-hashed in full on any change.
    #[tokio::test]
    async fn a_real_assembly_yields_one_artifact_per_type() {
        let mono = std::path::Path::new(
            "/home/legion/.var/app/net.lutris.Lutris/data/lutris/runners/wine/\
             wine-ge-8-25-x86_64/share/wine/mono/wine-mono-8.1.0/lib/mono/4.5",
        );
        let Ok(bytes) = std::fs::read(mono.join("mscorlib.dll")) else {
            eprintln!("skipping: no Mono corpus on this machine");
            return;
        };
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("mscorlib.dll"), &bytes).unwrap();

        let pkg = BinaryObserver::new()
            .scan(&ScanContext::new(dir.path()))
            .await
            .unwrap();
        assert!(pkg.artifacts.len() > 2_000, "got {}", pkg.artifacts.len());

        // Every artifact carries exactly one type and a target no other artifact shares.
        let targets = targets(&pkg);
        let unique: std::collections::HashSet<&String> = targets.iter().collect();
        assert_eq!(unique.len(), targets.len(), "targets must be unique");
        for a in pkg.artifacts.iter().take(50) {
            assert_eq!(a.content.data["ast"]["types"].as_array().unwrap().len(), 1);
        }

        // And no single artifact is anywhere near the size the unsplit one was.
        let largest = pkg
            .artifacts
            .iter()
            .map(|a| serde_json::to_string(&a.content.data).unwrap().len())
            .max()
            .unwrap();
        assert!(
            largest < 4 * 1024 * 1024,
            "largest artifact is {largest} bytes"
        );
    }

    /// Two scans of an unchanged tree must produce identical artifact ids, or `ekos build` is not
    /// idempotent and every run re-writes the ledger.
    #[tokio::test]
    async fn scanning_twice_produces_identical_artifact_ids() {
        let Ok(bytes) = std::fs::read("/usr/share/java/pdfbox.jar") else {
            eprintln!("skipping: pdfbox.jar not present on this machine");
            return;
        };
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("pdfbox.jar"), &bytes).unwrap();
        let ctx = ScanContext::new(dir.path());

        let first = BinaryObserver::new().scan(&ctx).await.unwrap();
        let second = BinaryObserver::new().scan(&ctx).await.unwrap();
        let ids = |p: &ObservationPackage| {
            p.artifacts
                .iter()
                .map(|a| a.id.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ids(&second));
    }
}
