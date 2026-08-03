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
mod email;
mod html;
mod ocr;
mod pdf;
mod sanitize;
mod text;

pub use docx::DocxParser;
pub use email::EmailParser;
pub use html::HtmlParser;
pub use ocr::{MockOcr, TesseractOcr};
pub use pdf::PdfParser;
pub use sanitize::sanitize_text;
pub use text::TextParser;

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

/// One page (PDF) or fixed-character-budget chunk (DOCX) of a document's
/// text, small enough to be fully indexed rather than sharing one
/// whole-document excerpt budget (RFC 0024).
#[derive(Debug, Clone)]
pub struct DocumentSection {
    /// 1-indexed PDF page number; `None` for DOCX — pagination is a
    /// rendering-time concept the document model doesn't expose.
    pub page: Option<u32>,
    /// 0-indexed position among this document's sections.
    pub index: usize,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedDocument {
    pub page_count: Option<u32>,
    pub text: String,
    pub tables: Vec<ExtractedTable>,
    pub images: Vec<EmbeddedImage>,
    pub sections: Vec<DocumentSection>,
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

    /// Every extension this parser handles. Defaults to just
    /// `supported_extension()`, which is all any parser registered today
    /// needs — multi-extension formats are handled by registering the same
    /// parser once per extension (RFC 0025), keeping the observer's
    /// extension→parser lookup unambiguous.
    fn supported_extensions(&self) -> Vec<&str> {
        vec![self.supported_extension()]
    }

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
/// Cap on sections captured per document (RFC 0024). See the RFC for the
/// index-growth justification (bounded by real usage: 45 books × 300 in
/// devlog 25's library is ~1.6x the pre-RFC-0024 index size).
const SECTIONS_MAX: usize = 300;
/// Cap on raw per-section text stored in the artifact. `LocalDocAnalyzerPass`
/// (crates/recovery) applies its own, tighter cap when writing the
/// searchable `excerpt` property per Section KirObject — this is just the
/// artifact-storage bound.
const SECTION_TEXT_MAX_CHARS: usize = 3000;
/// DOCX has no page concept; paragraph text accumulates into a section
/// until this character budget is hit, then a new section starts.
const DOCX_CHUNK_CHAR_BUDGET: usize = 2500;
/// Same budget for the formats that have neither pages nor paragraph
/// structure to chunk on — plain text, Markdown, HTML, email (RFC 0025).
const TEXT_CHUNK_CHAR_BUDGET: usize = 2500;

/// Observer emitting one `ObservationArtifact` per supported document file.
pub struct LocalDocsObserver {
    parsers: Vec<Arc<dyn DocumentParser>>,
    ocr: Arc<dyn OcrEngine>,
}

impl LocalDocsObserver {
    pub fn new(parsers: Vec<Arc<dyn DocumentParser>>, ocr: Arc<dyn OcrEngine>) -> Self {
        Self { parsers, ocr }
    }

    /// Convenience constructor wiring every real parser this plugin ships.
    pub fn with_defaults(ocr: Arc<dyn OcrEngine>) -> Self {
        Self::new(
            vec![
                Arc::new(PdfParser),
                Arc::new(DocxParser),
                Arc::new(TextParser::new("txt")),
                Arc::new(TextParser::new("md")),
                Arc::new(HtmlParser::new("html")),
                Arc::new(HtmlParser::new("htm")),
                Arc::new(EmailParser),
            ],
            ocr,
        )
    }

    fn parser_for(&self, ext: &str) -> Option<&Arc<dyn DocumentParser>> {
        self.parsers.iter().find(|p| {
            p.supported_extensions()
                .iter()
                .any(|e| e.eq_ignore_ascii_case(ext))
        })
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

            // Every string below flows straight into the ledger and, from
            // there, into an agent's context via ekos_search/ekos_state —
            // sanitize before any of it is captured, not after.
            let mut sanitized_count = 0usize;

            let text_clean = sanitize_text(&parsed.text);
            sanitized_count += text_clean.removed;
            let excerpt: String = text_clean.text.chars().take(EXCERPT_MAX_CHARS).collect();

            let mut ocr_text = String::new();
            let mut ocr_image_count = 0usize;
            for image in &parsed.images {
                match self.ocr.recognize(image) {
                    Ok(text) if !text.trim().is_empty() => {
                        let clean = sanitize_text(text.trim());
                        sanitized_count += clean.removed;
                        if !ocr_text.is_empty() {
                            ocr_text.push('\n');
                        }
                        ocr_text.push_str(&clean.text);
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
                    let rows: Vec<Vec<String>> = t
                        .rows
                        .iter()
                        .take(TABLE_ROWS_MAX)
                        .map(|row| {
                            row.iter()
                                .map(|cell| {
                                    let clean = sanitize_text(cell);
                                    sanitized_count += clean.removed;
                                    clean.text
                                })
                                .collect()
                        })
                        .collect();
                    serde_json::json!({ "page": t.page, "rows": rows })
                })
                .collect();

            let sections_json: Vec<serde_json::Value> = parsed
                .sections
                .iter()
                .take(SECTIONS_MAX)
                .map(|s| {
                    let clean = sanitize_text(&s.text);
                    sanitized_count += clean.removed;
                    let text: String = clean.text.chars().take(SECTION_TEXT_MAX_CHARS).collect();
                    serde_json::json!({ "index": s.index, "page": s.page, "text": text })
                })
                .collect();

            if sanitized_count > 0 {
                tracing::warn!(
                    "localdocs observer: stripped {sanitized_count} invisible/tag-block \
                     character(s) from {} — possible hidden-instruction payload",
                    abs_path.display()
                );
            }

            let mut data = serde_json::json!({
                "path": rel_path,
                "size_bytes": size_bytes,
                "content_sha256": content_sha256,
                "doc_format": ext.to_ascii_lowercase(),
                "page_count": parsed.page_count,
                "excerpt": excerpt,
                "tables": tables_json,
                "sections": sections_json,
                "image_count": parsed.images.len(),
                "ocr_image_count": ocr_image_count,
            });
            if !ocr_text.is_empty() {
                data["ocr_text"] = serde_json::Value::String(ocr_text);
            }
            if sanitized_count > 0 {
                data["sanitized_chars_removed"] = serde_json::json!(sanitized_count);
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
                sections: vec![],
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
                sections: vec![],
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
                sections: vec![],
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
                sections: vec![],
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
                sections: vec![],
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
                sections: vec![],
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

    #[tokio::test]
    async fn hidden_unicode_payload_is_stripped_from_excerpt_table_and_ocr_text() {
        // Tag-block "hidden" text (invisible when rendered) planted in the
        // prose, a table cell, and the OCR output — the three places
        // extracted text reaches the ledger.
        let hidden = "\u{E0068}\u{E0069}\u{E0064}\u{E0064}\u{E0065}\u{E006E}"; // tag-spelled "hidden"
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some(1),
                text: format!("visible prose{hidden} continues"),
                tables: vec![ExtractedTable {
                    page: None,
                    rows: vec![vec![format!("cell{hidden}"), "value".to_string()]],
                }],
                images: vec![EmbeddedImage {
                    page: Some(1),
                    bytes: vec![0xff, 0xd8],
                    format: ImageFormat::Jpeg,
                }],
                sections: vec![],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: format!("scanned text{hidden} here"),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("malicious.pdf"), b"%PDF-fake").unwrap();
        })
        .await;
        let data = &pkg.artifacts[0].content.data;

        assert_eq!(data["excerpt"], "visible prose continues");
        assert_eq!(data["ocr_text"], "scanned text here");
        assert_eq!(data["tables"][0]["rows"][0][0], "cell");
        assert!(
            data["sanitized_chars_removed"].as_u64().unwrap() > 0,
            "removal count must be reported"
        );
    }

    #[tokio::test]
    async fn clean_document_carries_no_sanitized_chars_removed_field() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some(1),
                text: "perfectly ordinary prose".into(),
                tables: vec![],
                images: vec![],
                sections: vec![],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("clean.pdf"), b"%PDF-fake").unwrap();
        })
        .await;
        assert!(
            pkg.artifacts[0]
                .content
                .data
                .get("sanitized_chars_removed")
                .is_none()
        );
    }

    #[tokio::test]
    async fn sections_are_capped_at_sections_max() {
        let sections: Vec<DocumentSection> = (0..SECTIONS_MAX + 50)
            .map(|i| DocumentSection {
                page: Some(i as u32 + 1),
                index: i,
                text: format!("page {i} text"),
            })
            .collect();
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some((SECTIONS_MAX + 50) as u32),
                text: "whole doc".into(),
                tables: vec![],
                images: vec![],
                sections,
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("big.pdf"), b"%PDF-fake").unwrap();
        })
        .await;
        let sections = pkg.artifacts[0].content.data["sections"]
            .as_array()
            .unwrap();
        assert_eq!(sections.len(), SECTIONS_MAX);
    }

    #[tokio::test]
    async fn section_page_numbers_pass_through() {
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some(1),
                text: "doc".into(),
                tables: vec![],
                images: vec![],
                sections: vec![DocumentSection {
                    page: Some(3),
                    index: 0,
                    text: "page three content".into(),
                }],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("doc.pdf"), b"%PDF-fake").unwrap();
        })
        .await;
        let sections = &pkg.artifacts[0].content.data["sections"];
        assert_eq!(sections[0]["page"], 3);
        assert_eq!(sections[0]["text"], "page three content");
    }

    #[tokio::test]
    async fn section_text_is_sanitized_and_truncated() {
        let hidden = "\u{E0068}\u{E0069}"; // tag-h, tag-i — invisible
        let long_text = format!("start{hidden}{}", "x".repeat(SECTION_TEXT_MAX_CHARS + 100));
        let parser: Arc<dyn DocumentParser> = Arc::new(FixedParser {
            ext: "pdf",
            doc: ParsedDocument {
                page_count: Some(1),
                text: "doc".into(),
                tables: vec![],
                images: vec![],
                sections: vec![DocumentSection {
                    page: Some(1),
                    index: 0,
                    text: long_text,
                }],
            },
        });
        let ocr: Arc<dyn OcrEngine> = Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        });
        let pkg = scan_temp(vec![parser], ocr, |dir| {
            std::fs::write(dir.path().join("doc.pdf"), b"%PDF-fake").unwrap();
        })
        .await;
        let data = &pkg.artifacts[0].content.data;
        let section_text = data["sections"][0]["text"].as_str().unwrap();
        assert!(!section_text.contains('\u{E0068}'));
        assert!(section_text.chars().count() <= SECTION_TEXT_MAX_CHARS);
        assert!(data["sanitized_chars_removed"].as_u64().unwrap() > 0);
    }

    // ── RFC 0025: text/Markdown, HTML, email ────────────────────────────

    fn default_parsers() -> Vec<Arc<dyn DocumentParser>> {
        vec![
            Arc::new(TextParser::new("txt")),
            Arc::new(TextParser::new("md")),
            Arc::new(HtmlParser::new("html")),
            Arc::new(HtmlParser::new("htm")),
            Arc::new(EmailParser),
        ]
    }

    fn silent_ocr() -> Arc<dyn OcrEngine> {
        Arc::new(RecordingMockOcr {
            text: String::new(),
            calls: Mutex::new(0),
        })
    }

    #[test]
    fn with_defaults_registers_a_parser_for_every_rfc_0025_extension() {
        let observer = LocalDocsObserver::with_defaults(silent_ocr());
        for ext in ["pdf", "docx", "txt", "md", "html", "htm", "eml"] {
            assert!(
                observer.parser_for(ext).is_some(),
                "no parser registered for .{ext}"
            );
        }
        assert!(observer.parser_for("xlsx").is_none());
    }

    #[test]
    fn extension_lookup_is_case_insensitive() {
        let observer = LocalDocsObserver::with_defaults(silent_ocr());
        assert!(observer.parser_for("MD").is_some());
        assert!(observer.parser_for("HTML").is_some());
        assert!(observer.parser_for("EML").is_some());
    }

    #[tokio::test]
    async fn mixed_format_directory_produces_one_artifact_per_file() {
        let pkg = scan_temp(default_parsers(), silent_ocr(), |dir| {
            let p = dir.path();
            std::fs::write(
                p.join("handover.txt"),
                include_bytes!("../tests/fixtures/handover.txt"),
            )
            .unwrap();
            std::fs::write(
                p.join("notes.md"),
                include_bytes!("../tests/fixtures/notes.md"),
            )
            .unwrap();
            std::fs::write(
                p.join("runbook.html"),
                include_bytes!("../tests/fixtures/runbook.html"),
            )
            .unwrap();
            std::fs::write(
                p.join("copy.htm"),
                include_bytes!("../tests/fixtures/runbook.html"),
            )
            .unwrap();
            std::fs::write(
                p.join("incident.eml"),
                include_bytes!("../tests/fixtures/incident-thread.eml"),
            )
            .unwrap();
            // Unsupported by any registered parser — must be skipped, not
            // counted as an error.
            std::fs::write(p.join("sheet.xlsx"), b"PK-fake").unwrap();
        })
        .await;

        assert_eq!(pkg.len(), 5);
        assert_eq!(pkg.meta.error_count, 0);
        let formats: std::collections::BTreeSet<String> = pkg
            .artifacts
            .iter()
            .map(|a| a.content.data["doc_format"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            formats,
            ["eml", "htm", "html", "md", "txt"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        );
    }

    #[tokio::test]
    async fn markdown_artifact_carries_sections_beyond_the_excerpt_budget() {
        let pkg = scan_temp(default_parsers(), silent_ocr(), |dir| {
            std::fs::write(
                dir.path().join("notes.md"),
                include_bytes!("../tests/fixtures/notes.md"),
            )
            .unwrap();
        })
        .await;
        let data = &pkg.artifacts[0].content.data;

        // The whole-document excerpt is still capped at 600 chars...
        assert!(data["excerpt"].as_str().unwrap().chars().count() <= EXCERPT_MAX_CHARS);
        // ...but content past that cap survives on a Section.
        let sections_text: String = data["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["text"].as_str().unwrap())
            .collect();
        assert!(sections_text.contains("Exceptions expire after one year"));
        assert!(
            !data["excerpt"]
                .as_str()
                .unwrap()
                .contains("Exceptions expire")
        );
    }

    #[tokio::test]
    async fn hidden_unicode_in_a_markdown_file_is_stripped_from_excerpt_and_sections() {
        let hidden = "\u{E0068}\u{E0069}\u{E0064}"; // tag-spelled, invisible
        let pkg = scan_temp(default_parsers(), silent_ocr(), |dir| {
            std::fs::write(
                dir.path().join("payload.md"),
                format!("# Heading{hidden}\n\nvisible prose{hidden} continues\n"),
            )
            .unwrap();
        })
        .await;
        let data = &pkg.artifacts[0].content.data;
        assert!(!data["excerpt"].as_str().unwrap().contains('\u{E0068}'));
        assert!(
            !data["sections"][0]["text"]
                .as_str()
                .unwrap()
                .contains('\u{E0068}')
        );
        assert!(data["sanitized_chars_removed"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn email_artifact_carries_header_and_body_sections() {
        let pkg = scan_temp(default_parsers(), silent_ocr(), |dir| {
            std::fs::write(
                dir.path().join("incident.eml"),
                include_bytes!("../tests/fixtures/incident-thread.eml"),
            )
            .unwrap();
        })
        .await;
        let data = &pkg.artifacts[0].content.data;
        assert_eq!(data["doc_format"], "eml");
        assert_eq!(data["page_count"], serde_json::Value::Null);
        let sections = data["sections"].as_array().unwrap();
        assert!(sections[0]["text"].as_str().unwrap().contains("Subject:"));
        assert!(sections[0]["page"].is_null());
        let body: String = sections[1..]
            .iter()
            .map(|s| s["text"].as_str().unwrap())
            .collect();
        assert!(body.contains("ingest_orders"));
    }
}
