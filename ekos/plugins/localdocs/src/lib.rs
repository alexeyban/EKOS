//! Local document observer plugin — PDF/DOCX text, tables, and image OCR
//! (RFC 0023).
//!
//! Walks the workspace tree the same way `FileObserver` does, but only for
//! `.pdf`/`.docx` files, parsing each into prose text, tables, and OCR'd
//! image text. Runs alongside `FileObserver` under a distinct connector
//! name (`"localdocs"`) so the two never collide in the artifact index.

use async_trait::async_trait;
use ekos_artifact::ObservationArtifact;
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use thiserror::Error;
use walkdir::WalkDir;

mod docx;
mod ocr;
mod pdf;

pub use docx::DocxParser;
pub use ocr::{MockOcr, TesseractOcr};
pub use pdf::PdfParser;

// ── Parsing types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

#[derive(Debug, Clone)]
pub struct EmbeddedImage {
    pub page: Option<u32>,
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
}

#[derive(Debug, Clone)]
pub struct ExtractedTable {
    pub page: Option<u32>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedDocument {
    pub page_count: Option<u32>,
    pub text: String,
    pub tables: Vec<ExtractedTable>,
    pub images: Vec<EmbeddedImage>,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("parse error: {0}")]
    Malformed(String),
}

/// Format-specific document parser. Constructor-injected into
/// `LocalDocsObserver`, mirroring every other connector's client-trait
/// shape — real parsers do the work, but tests can swap in fixtures.
pub trait DocumentParser: Send + Sync {
    /// Lowercase extension this parser handles, e.g. `"pdf"`.
    fn supported_extension(&self) -> &str;
    fn parse(&self, bytes: &[u8]) -> Result<ParsedDocument, ParseError>;
}

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("ocr engine unavailable: {0}")]
    Unavailable(String),
    #[error("ocr failed: {0}")]
    Failed(String),
}

/// OCR abstraction. `TesseractOcr` shells out to the `tesseract` CLI (no
/// `unsafe`, no FFI); `MockOcr` is the test double every other connector's
/// mock client plays the same role for.
pub trait OcrEngine: Send + Sync {
    fn recognize(&self, image: &EmbeddedImage) -> Result<String, OcrError>;
}

/// Cap on the excerpt captured from a document's extracted text, same
/// convention as `FileObserver`'s `EXCERPT_MAX_CHARS` (RFC 0014).
const EXCERPT_MAX_CHARS: usize = 600;
/// Cap on concatenated OCR text carried on an artifact.
const OCR_TEXT_MAX_CHARS: usize = 2000;
/// Cap on the number of tables recorded per document.
const TABLES_MAX: usize = 20;
/// Cap on the number of rows recorded per table.
const TABLE_ROWS_MAX: usize = 200;

/// Observer emitting one `ObservationArtifact` per PDF/DOCX file.
pub struct LocalDocsObserver {
    parsers: Vec<Arc<dyn DocumentParser>>,
    ocr: Arc<dyn OcrEngine>,
}

impl LocalDocsObserver {
    pub fn new(parsers: Vec<Arc<dyn DocumentParser>>, ocr: Arc<dyn OcrEngine>) -> Self {
        Self { parsers, ocr }
    }

    /// Convenience constructor wiring the real `PdfParser`/`DocxParser`.
    pub fn with_defaults(ocr: Arc<dyn OcrEngine>) -> Self {
        Self::new(vec![Arc::new(PdfParser), Arc::new(DocxParser)], ocr)
    }

    fn parser_for(&self, ext: &str) -> Option<&Arc<dyn DocumentParser>> {
        self.parsers
            .iter()
            .find(|p| p.supported_extension().eq_ignore_ascii_case(ext))
    }
}

#[async_trait]
impl Observer for LocalDocsObserver {
    fn name(&self) -> &str {
        "localdocs"
    }

    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let root = &ctx.workspace_root;
        let target = root.display().to_string();
        let mut pkg = ObservationPackage::new("localdocs", &target);

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
                    tracing::warn!("localdocs observer: skipping unreadable entry: {err}");
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            if !entry.file_type().is_file() {
                continue;
            }

            let abs_path = entry.path();
            let rel_path = match abs_path.strip_prefix(root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if ctx.is_ignored(&rel_path) {
                continue;
            }

            let Some(ext) = abs_path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            let Some(parser) = self.parser_for(ext) else {
                continue;
            };

            let content = match tokio::fs::read(abs_path).await {
                Ok(bytes) => bytes,
                Err(err) => {
                    tracing::warn!(
                        "localdocs observer: cannot read {}: {err}",
                        abs_path.display()
                    );
                    pkg.meta.error_count += 1;
                    continue;
                }
            };

            let parsed = match parser.parse(&content) {
                Ok(p) => p,
                Err(err) => {
                    tracing::warn!(
                        "localdocs observer: cannot parse {}: {err}",
                        abs_path.display()
                    );
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

            let excerpt: String = parsed.text.chars().take(EXCERPT_MAX_CHARS).collect();

            let mut ocr_text = String::new();
            let mut ocr_image_count = 0usize;
            for image in &parsed.images {
                match self.ocr.recognize(image) {
                    Ok(text) if !text.trim().is_empty() => {
                        if !ocr_text.is_empty() {
                            ocr_text.push('\n');
                        }
                        ocr_text.push_str(text.trim());
                        ocr_image_count += 1;
                    }
                    Ok(_) => {}
                    Err(OcrError::Unavailable(msg)) => {
                        tracing::warn!(
                            "localdocs observer: OCR unavailable for {}: {msg}",
                            abs_path.display()
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            "localdocs observer: OCR failed for {}: {err}",
                            abs_path.display()
                        );
                    }
                }
            }
            let ocr_text: String = ocr_text.chars().take(OCR_TEXT_MAX_CHARS).collect();

            let tables_json: Vec<serde_json::Value> = parsed
                .tables
                .iter()
                .take(TABLES_MAX)
                .map(|t| {
                    let rows: Vec<Vec<String>> =
                        t.rows.iter().take(TABLE_ROWS_MAX).cloned().collect();
                    serde_json::json!({ "page": t.page, "rows": rows })
                })
                .collect();

            let mut data = serde_json::json!({
                "path": rel_path,
                "size_bytes": size_bytes,
                "content_sha256": content_sha256,
                "doc_format": ext.to_ascii_lowercase(),
                "page_count": parsed.page_count,
                "excerpt": excerpt,
                "tables": tables_json,
                "image_count": parsed.images.len(),
                "ocr_image_count": ocr_image_count,
            });
            if !ocr_text.is_empty() {
                data["ocr_text"] = serde_json::Value::String(ocr_text);
            }

            let artifact = ObservationArtifact::new("localdocs", &rel_path, data)
                .with_producer("ekos-plugin-localdocs");
            pkg.push(artifact);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct FixedParser {
        ext: &'static str,
        doc: ParsedDocument,
    }

    impl DocumentParser for FixedParser {
        fn supported_extension(&self) -> &str {
            self.ext
        }
        fn parse(&self, _bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
            Ok(self.doc.clone())
        }
    }

    struct FailingParser {
        ext: &'static str,
    }

    impl DocumentParser for FailingParser {
        fn supported_extension(&self) -> &str {
            self.ext
        }
        fn parse(&self, _bytes: &[u8]) -> Result<ParsedDocument, ParseError> {
            Err(ParseError::Malformed("bad document".into()))
        }
    }

    struct RecordingMockOcr {
        text: String,
        calls: Mutex<usize>,
    }

    impl OcrEngine for RecordingMockOcr {
        fn recognize(&self, _image: &EmbeddedImage) -> Result<String, OcrError> {
            *self.calls.lock().unwrap() += 1;
            Ok(self.text.clone())
        }
    }

    struct UnavailableOcr;

    impl OcrEngine for UnavailableOcr {
        fn recognize(&self, _image: &EmbeddedImage) -> Result<String, OcrError> {
            Err(OcrError::Unavailable("tesseract not found on PATH".into()))
        }
    }

    async fn scan_temp(
        parsers: Vec<Arc<dyn DocumentParser>>,
        ocr: Arc<dyn OcrEngine>,
        setup: impl FnOnce(&TempDir),
    ) -> ObservationPackage {
        let dir = TempDir::new().unwrap();
        setup(&dir);
        let ctx = ScanContext::new(dir.path());
        LocalDocsObserver::new(parsers, ocr)
            .scan(&ctx)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn emits_one_artifact_per_document() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some(1),
                text: "hello world".into(),
                tables: vec![],
                images: vec![],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("spec.pdf"), b"%PDF-fake").unwrap();
            std::fs::write(dir.path().join("notes.txt"), b"ignored, not pdf/docx").unwrap();
        })
        .await;
        assert_eq!(pkg.len(), 1);
        assert_eq!(pkg.artifacts[0].content.connector_name, "localdocs");
        assert_eq!(pkg.artifacts[0].content.target, "spec.pdf");
        assert_eq!(pkg.artifacts[0].content.data["excerpt"], "hello world");
    }

    #[tokio::test]
    async fn empty_dir_produces_no_artifacts() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument::default(),
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |_| {}).await;
        assert!(pkg.is_empty());
    }

    #[tokio::test]
    async fn tables_ride_on_the_artifact() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "docx",
            doc: ParsedDocument {
                page_count: None,
                text: "a report".into(),
                tables: vec![ExtractedTable {
                    page: Some(1),
                    rows: vec![
                        vec!["Name".into(), "Value".into()],
                        vec!["a".into(), "1".into()],
                    ],
                }],
                images: vec![],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("report.docx"), b"PK-fake").unwrap();
        })
        .await;
        let tables = pkg.artifacts[0].content.data["tables"].as_array().unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0]["rows"][1][1], "1");
    }

    #[tokio::test]
    async fn ocr_text_only_present_when_images_yield_text() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some(1),
                text: "scanned doc".into(),
                tables: vec![],
                images: vec![EmbeddedImage {
                    page: Some(1),
                    bytes: vec![0xff, 0xd8],
                    format: ImageFormat::Jpeg,
                }],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: "recognized text".into(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("scan.pdf"), b"%PDF-fake").unwrap();
        })
        .await;
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["ocr_text"], "recognized text");
        assert_eq!(data["ocr_image_count"], 1);
    }

    #[tokio::test]
    async fn ocr_unavailable_soft_skips_without_failing_scan() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some(1),
                text: "scanned doc".into(),
                tables: vec![],
                images: vec![EmbeddedImage {
                    page: Some(1),
                    bytes: vec![0xff, 0xd8],
                    format: ImageFormat::Jpeg,
                }],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(UnavailableOcr);
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("scan.pdf"), b"%PDF-fake").unwrap();
        })
        .await;
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data.get("ocr_text"), None);
        assert_eq!(data["ocr_image_count"], 0);
        assert_eq!(data["image_count"], 1);
    }

    #[tokio::test]
    async fn parse_failure_soft_skips_the_file() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FailingParser { ext: "pdf" });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("broken.pdf"), b"%PDF-fake").unwrap();
        })
        .await;
        assert!(pkg.is_empty());
        assert_eq!(pkg.meta.error_count, 1);
    }

    #[tokio::test]
    async fn same_file_same_artifact_id() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some(1),
                text: "stable".into(),
                tables: vec![],
                images: vec![],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        });
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("doc.pdf"), b"%PDF-fake").unwrap();
        let ctx = ScanContext::new(dir.path());
        let observer = LocalDocsObserver::new(vec![parser], ocr);
        let id1 = observer.scan(&ctx).await.unwrap().artifacts[0].id.clone();
        let id2 = observer.scan(&ctx).await.unwrap().artifacts[0].id.clone();
        assert_eq!(id1, id2);
    }

    /// Real `pdf-extract` output captured from a public statistics course
    /// PDF, verified via an end-to-end run against a real document library
    /// (RFC 0023's devlog) — not hand-crafted synthetic prose. Confirms the
    /// excerpt carries genuine book content through unmodified (well under
    /// the 600-char cap, so nothing is truncated here).
    const REAL_STATISTICS_EXCERPT: &str = "\n\nThe normal curve\n\nMany data have histograms that look bell-shaped, e.g. heights, weights, IQ scores:\n\nHeights of 928 Fathers\n\n64 66 68 70 72\n\u{2018}The data follow the normal curve.\u{2019}\n\nBut remember that some data have histograms that look quite different, e.g. incomes,\nhouse prices.\n The empirical rule\n\n";

    /// Real `tesseract` OCR output captured from a scanned book cover in
    /// the same end-to-end run — noisy, irregularly spaced, but genuinely
    /// legible text a scanned page's raw bytes would otherwise hide from
    /// the ledger entirely.
    const REAL_OCR_COVER_TEXT: &str = "cole nussbaumer knaflic\n\nstorytelling\nwith\n\ndata\n\na data\n\nvisualization\nguide for\nbusiness\nprofessionals";

    #[tokio::test]
    async fn real_book_excerpt_and_ocr_text_ride_on_the_artifact_unmodified() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some(24),
                text: REAL_STATISTICS_EXCERPT.to_string(),
                tables: vec![],
                images: vec![EmbeddedImage {
                    page: Some(1),
                    bytes: vec![0xff, 0xd8],
                    format: ImageFormat::Jpeg,
                }],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: REAL_OCR_COVER_TEXT.to_string(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("stats.pdf"), b"%PDF-fake").unwrap();
        })
        .await;
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["excerpt"], REAL_STATISTICS_EXCERPT);
        assert_eq!(data["ocr_text"], REAL_OCR_COVER_TEXT);
    }
}
