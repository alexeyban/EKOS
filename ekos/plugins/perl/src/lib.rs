//! Perl (`.pl`/`.pm`/`.t`/`.psgi`/`.cgi`) observer plugin (RFC 0147).
//!
//! Walks the workspace tree the same way `ElixirObserver`/`RustObserver` do, but for Perl source
//! files, capturing the raw source verbatim as a fact — no parsing happens here. That
//! deterministic structural step lives in `ekos_recovery::PerlAnalyzerPass`, which reads this
//! observer's output.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// Extensions that are unambiguously Perl: modules, scripts, tests, and PSGI/Plack application
/// entry points. `.cgi` is deliberately absent — see [`is_perl_cgi`].
const PERL_EXTENSIONS: &[&str] = &["pl", "pm", "t", "psgi"];

/// Observer emitting one `ObservationArtifact` per Perl source file found under the workspace
/// root.
#[derive(Debug, Default)]
pub struct PerlObserver;

impl PerlObserver {
    pub fn new() -> Self {
        Self
    }
}

/// Whether a `.cgi` file is really Perl, decided by its own `#!` shebang rather than by its
/// extension.
///
/// `.cgi` is not Perl-exclusive (it was the generic CGI extension for Python, shell and C
/// binaries too), but legacy Perl web estates are full of real `.cgi` entry points — LedgerSMB,
/// the Perl ERP motivating RFC 0147, dispatches through exactly this shape. Claiming every
/// `.cgi` as Perl would misfile other languages' scripts; requiring the real shebang is a
/// deterministic, evidence-based test with no guessing in it.
fn is_perl_cgi(source: &str) -> bool {
    let Some(first) = source.lines().next() else {
        return false;
    };
    first.starts_with("#!") && first.contains("perl")
}

#[async_trait]
impl Observer for PerlObserver {
    fn name(&self) -> &str {
        "perl"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("perl", &target);

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
                    tracing::warn!("perl observer: skipping unreadable entry: {err}");
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let abs_path = entry.path();
            let rel_path = match abs_path.strip_prefix(root) {
                // A real, valid `[observe] paths` entry can be a single bare file, not a
                // directory — `WalkDir::new(root)` then yields exactly one entry equal to
                // `root` itself, and stripping it from itself leaves an empty relative path
                // (the same real bug RFC 0088's own live verification found in `plugins/file`
                // and `plugins/localdocs` first). Falls back to the file's own name.
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

            let ext = abs_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase());
            let Some(ext) = ext else { continue };
            let is_cgi = ext == "cgi";
            if !is_cgi && !PERL_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            let source = match tokio::fs::read_to_string(abs_path).await {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!("perl observer: cannot read {}: {err}", abs_path.display());
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            // Read-then-decide for `.cgi` only: the shebang test needs the file's first line, and
            // every other extension is already unambiguous without reading anything.
            if is_cgi && !is_perl_cgi(&source) {
                continue;
            }

            let size_bytes = source.len();
            let content_sha256 = {
                let mut h = Sha256::new();
                h.update(source.as_bytes());
                hex::encode(h.finalize())
            };

            let data = serde_json::json!({
                "path": rel_path,
                "size_bytes": size_bytes,
                "content_sha256": content_sha256,
                "source": source,
            });

            let artifact =
                ObservationArtifact::new("perl", &rel_path, data).with_producer("ekos-plugin-perl");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SAMPLE_PM: &str = r#"package LedgerSMB::Payment;
use strict;

sub new {
    my ($class, %args) = @_;
    return bless {%args}, $class;
}

1;
"#;

    fn paths_of(pkg: &ObservationPackage) -> Vec<&str> {
        pkg.artifacts
            .iter()
            .map(|a| a.content.data["path"].as_str().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn observer_emits_one_artifact_per_perl_source_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("Payment.pm"), SAMPLE_PM).unwrap();
        std::fs::write(dir.path().join("setup.pl"), "print \"hi\\n\";\n").unwrap();
        std::fs::write(dir.path().join("payment.t"), "use Test::More;\n").unwrap();
        std::fs::write(dir.path().join("app.psgi"), "my $app = sub { };\n").unwrap();
        std::fs::write(dir.path().join("readme.md"), "not perl").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = PerlObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 4);
        let paths = paths_of(&pkg);
        for expected in ["Payment.pm", "setup.pl", "payment.t", "app.psgi"] {
            assert!(paths.contains(&expected), "missing {expected}");
        }
    }

    #[tokio::test]
    async fn a_cgi_file_with_a_real_perl_shebang_is_collected() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("login.cgi"),
            "#!/usr/bin/perl\nuse LedgerSMB::Auth;\n",
        )
        .unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = PerlObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(paths_of(&pkg), vec!["login.cgi"]);
    }

    #[tokio::test]
    async fn a_cgi_file_that_is_not_perl_is_not_collected() {
        // The negative case the shebang rule exists for: `.cgi` was the generic CGI extension,
        // so extension alone would misfile another language's script as Perl.
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("report.cgi"),
            "#!/usr/bin/env python3\nprint('hi')\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("legacy.cgi"), "no shebang at all\n").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = PerlObserver::new().scan(&ctx).await.unwrap();

        assert!(pkg.artifacts.is_empty());
    }

    #[tokio::test]
    async fn a_single_bare_file_observe_path_gets_its_own_real_name_not_an_empty_one() {
        // Real bug found live (RFC 0088's own verification): a real, valid `[observe] paths`
        // entry can be a single bare file, not a directory. `WalkDir::new(root)` then yields
        // exactly one entry equal to `root` itself, and stripping it from itself used to leave
        // an empty relative path.
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("Payment.pm");
        std::fs::write(&file_path, SAMPLE_PM).unwrap();

        let ctx = ScanContext::new(&file_path);
        let pkg = PerlObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg.artifacts.len(), 1);
        assert_eq!(pkg.artifacts[0].content.data["path"], "Payment.pm");
    }

    #[tokio::test]
    async fn observer_ignores_unrelated_extensions() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("script.py"), "print(1)").unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg = PerlObserver::new().scan(&ctx).await.unwrap();

        assert!(pkg.artifacts.is_empty());
    }

    #[tokio::test]
    async fn same_file_produces_same_content_hash_across_runs() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.pm"), SAMPLE_PM).unwrap();

        let ctx = ScanContext::new(dir.path());
        let pkg1 = PerlObserver::new().scan(&ctx).await.unwrap();
        let pkg2 = PerlObserver::new().scan(&ctx).await.unwrap();

        assert_eq!(pkg1.artifacts[0].id, pkg2.artifacts[0].id);
    }
}
